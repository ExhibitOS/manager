// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing OES light types and producer units. These are renderer inputs,
/// not a calibrated illuminance or artwork-conservation measurement.
public enum NativeLightType: String, CaseIterable, Sendable {
    case point, spot, directional, area
    public var unit: String {
        switch self { case .point, .spot: "candela"; case .directional: "lux"; case .area: "lumen" }
    }
}

/// One native-list selection, separate from the strict v1 viewport selection.
/// This adapter does not grant transport authority or widen geometry actions.
public struct LightCommand: Equatable, Sendable {
    public let lightId: String
    public let type: NativeLightType
    public let color: [Double] // Linear RGB, as consumed by the shared renderer.
    public let intensity: Double
    public let position: [Double] // Room-local metres.
    public let beamAngle: Double? // Full beam angle, radians in OES.
    public let areaSize: [Double]?

    public init(lightId: String, type: NativeLightType, color: [Double], intensity: Double,
                position: [Double], beamAngle: Double? = nil, areaSize: [Double]? = nil) {
        self.lightId = lightId; self.type = type; self.color = color; self.intensity = intensity
        self.position = position; self.beamAngle = beamAngle; self.areaSize = areaSize
    }
    fileprivate static func invalid() -> AuthoringFailure { AuthoringFailure("LIGHT_FIELDS_INVALID") }
    fileprivate static func row(in draft: NativeExhibitionDraft, id: String, selectedLightId: String?) throws -> (Int, DocumentValue) {
        guard UUID(uuidString: id) != nil, selectedLightId == id,
              let rows = draft.candidate["lights"].items else { throw AuthoringFailure("LIGHT_SELECTION_CHANGED") }
        let matches = rows.indices.filter { rows[$0]["id"].string?.lowercased() == id.lowercased() }
        guard matches.count == 1, let index = matches.first, rows[index]["id"].string == id,
              case .object = rows[index], let room = rows[index]["roomId"].string,
              draft.rooms.contains(where: { $0["id"].string?.lowercased() == room.lowercased() }) else {
            throw AuthoringFailure("LIGHT_SELECTION_CHANGED")
        }
        return (index, rows[index])
    }
    private func replacement(in draft: NativeExhibitionDraft, selectedLightId: String?) throws -> (Int, DocumentValue) {
        let (index, original) = try Self.row(in: draft, id: lightId, selectedLightId: selectedLightId)
        guard color.count == 3, color.allSatisfy({ $0.isFinite && (0...1).contains($0) }),
              intensity.isFinite, (0...1_000_000).contains(intensity), position.count == 3,
              position.allSatisfy({ $0.isFinite && abs($0) <= 10_000 }),
              case .object = original["transform"],
              let oldType = original["type"].string.flatMap(NativeLightType.init(rawValue:)),
              original["unit"].string == oldType.unit,
              (oldType == .spot) == (original["beamAngle"] != .null),
              (oldType == .area) == (original["dimensions"] != .null) else { throw Self.invalid() }
        if type == .spot {
            guard let beamAngle, beamAngle.isFinite, beamAngle > 0, beamAngle <= .pi / 2, areaSize == nil else { throw Self.invalid() }
        } else if beamAngle != nil { throw Self.invalid() }
        if type == .area {
            guard let areaSize, areaSize.count == 2,
                  areaSize.allSatisfy({ $0.isFinite && (0.01...100).contains($0) }) else { throw Self.invalid() }
        } else if areaSize != nil { throw Self.invalid() }

        var row = original
        func set(_ key: String, _ value: DocumentValue) { if row[key] != value { row[key] = value } }
        set("type", .string(type.rawValue)); set("unit", .string(type.unit))
        set("color", .vector(color)); set("intensity", .number(intensity))
        if row["transform"]["position"] != .vector(position) { row["transform"]["position"] = .vector(position) }
        if let beamAngle { set("beamAngle", .number(beamAngle)) }
        if let areaSize {
            if row["dimensions"] == .null { row["dimensions"] = .object([:]) }
            guard case .object = row["dimensions"] else { throw Self.invalid() }
            if row["dimensions"]["width"] != .number(areaSize[0]) { row["dimensions"]["width"] = .number(areaSize[0]) }
            if row["dimensions"]["height"] != .number(areaSize[1]) { row["dimensions"]["height"] = .number(areaSize[1]) }
        }
        guard case .object(var fields) = row else { throw Self.invalid() }
        if type != .spot { fields.removeValue(forKey: "beamAngle") }
        if type != .area {
            // A type switch must not silently erase future dimension metadata.
            if case .object(let dimensions) = original["dimensions"], !Set(dimensions.keys).isSubset(of: ["width", "height"]) { throw Self.invalid() }
            fields.removeValue(forKey: "dimensions")
        }
        return (index, .object(fields))
    }
    /// Local typed proposal only. A caller must obtain shared validation before
    /// preview/apply; this method neither persists nor contacts a service.
    public func proposedCandidate(in draft: NativeExhibitionDraft, selectedLightId: String?) throws -> DocumentValue {
        let (index, row) = try replacement(in: draft, selectedLightId: selectedLightId)
        var candidate = draft.candidate, rows = candidate["lights"].items!
        rows[index] = row; candidate["lights"] = .array(rows)
        let template = "org.exhibitos.studio/template"
        if candidate["extensions"][template] != .null {
            guard case .object = candidate["extensions"][template], case .bool = candidate["extensions"][template]["modified"] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            candidate["extensions"][template]["modified"] = .bool(true)
        }
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
    /// Correspondence only, after the shared validator. Reject every unrelated
    /// change, including other lights, targets, poses, approved assets and rights.
    public func checkProposal(_ candidate: DocumentValue, base: NativeExhibitionDraft, selectedLightId: String?) throws {
        guard try candidate == proposedCandidate(in: base, selectedLightId: selectedLightId) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
    }
}

/// No draft mutation while typing, including invalid or incomplete numbers.
public struct LightFormBuffer: Equatable, Sendable {
    public let lightId: String
    public var type: NativeLightType
    public var red: String, green: String, blue: String
    public var intensity: String
    public var x: String, y: String, z: String
    public var beamDegrees: String
    public var width: String, height: String
    private let originalBeamRadians: Double?
    private let originalBeamDegrees: String

    public init(draft: NativeExhibitionDraft, selectedLightId: String) throws {
        let (_, row) = try LightCommand.row(in: draft, id: selectedLightId, selectedLightId: selectedLightId)
        guard let type = row["type"].string.flatMap(NativeLightType.init(rawValue:)), row["unit"].string == type.unit,
              (type == .spot) == (row["beamAngle"] != .null),
              (type == .area) == (row["dimensions"] != .null),
              let color = row["color"].items, color.count == 3,
              let position = row["transform"]["position"].items, position.count == 3 else { throw LightCommand.invalid() }
        func text(_ value: DocumentValue) throws -> String {
            guard let n = value.double, n.isFinite else { throw LightCommand.invalid() }; return String(n)
        }
        lightId = selectedLightId; self.type = type
        red = try text(color[0]); green = try text(color[1]); blue = try text(color[2])
        intensity = try text(row["intensity"])
        x = try text(position[0]); y = try text(position[1]); z = try text(position[2])
        originalBeamRadians = row["beamAngle"].double
        originalBeamDegrees = originalBeamRadians.map { String($0 * 180 / .pi) } ?? "45.0"
        beamDegrees = originalBeamDegrees
        width = type == .area ? try text(row["dimensions"]["width"]) : "1.0"
        height = type == .area ? try text(row["dimensions"]["height"]) : "1.0"
        // Refuse unsupported existing data without normalizing or writing it.
        _ = try command().proposedCandidate(in: draft, selectedLightId: selectedLightId)
    }
    /// Match existing newLight defaults; changing units is not a conversion.
    public mutating func changeType(_ value: NativeLightType) {
        guard type != value else { return }
        if type.unit != value.unit { intensity = "100.0" }
        type = value
    }
    public func command() throws -> LightCommand {
        func number(_ value: String) throws -> Double {
            guard let n = Double(value.trimmingCharacters(in: .whitespacesAndNewlines)), n.isFinite else { throw LightCommand.invalid() }; return n
        }
        let beam: Double?
        if type == .spot {
            // Opening and applying unchanged settings must not round-trip a
            // stored radian value through a degree display and alter its bits.
            beam = beamDegrees == originalBeamDegrees && originalBeamRadians != nil ? originalBeamRadians : try number(beamDegrees) * .pi / 180
        } else { beam = nil }
        return LightCommand(lightId: lightId, type: type, color: try [number(red), number(green), number(blue)],
                            intensity: try number(intensity), position: try [number(x), number(y), number(z)],
                            beamAngle: beam, areaSize: type == .area ? try [number(width), number(height)] : nil)
    }
}

/// Transient result of shared validation. Not wired into an unvalidated save or
/// strict-v1 action. The future owner must also fence account/access identity.
public struct LightPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft
    public let command: LightCommand
    public let candidate: DocumentValue
    public let displayDraft: NativeExhibitionDraft
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: LightCommand, selectedLightId: String?) throws {
        try command.checkProposal(candidate, base: base, selectedLightId: selectedLightId)
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, selectedLightId: String?, command: LightCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        try command.checkProposal(candidate, base: draft, selectedLightId: selectedLightId)
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}
