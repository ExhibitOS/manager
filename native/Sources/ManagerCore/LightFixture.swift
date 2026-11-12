// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing shared light-details fixture values. Geometry, aiming and room
/// clearance stay with the negotiated shared validator; no native mesh copy.
public enum NativeLightFixture: String, CaseIterable, Sendable {
    case none, spotCan = "spot-can", disc, panel
    public static let namespace = "org.exhibitos.studio/light-details"
    public func supports(_ type: NativeLightType) -> Bool {
        switch self {
        case .none: true
        case .spotCan: type == .spot
        case .disc: type == .point
        case .panel: type == .area
        }
    }
    private static func invalid() -> AuthoringFailure { AuthoringFailure("LIGHT_FIELDS_INVALID") }
    static func bindings(in candidate: DocumentValue) throws -> [DocumentValue] {
        if candidate["extensions"] != .null {
            guard case .object = candidate["extensions"] else { throw invalid() }
        }
        let value = candidate["extensions"][namespace]
        if value == .null { return [] }
        guard case .object(let fields) = value, Set(fields.keys) == Set(["version", "lights"]),
              value["version"].integer == 1, let rows = value["lights"].items, rows.count <= 32,
              try JSONEncoder().encode(value).count <= 16_384 else { throw invalid() }
        var seen = Set<String>()
        for row in rows {
            guard case .object = row, let id = row["lightId"].string, UUID(uuidString: id) != nil,
                  seen.insert(id.lowercased()).inserted else { throw invalid() }
            try RoomBaseLightIntent.checkMarker(row)
        }
        return rows
    }
    /// Preserve existing aim, K intent, range and beam softness byte values.
    /// Unsupported selected metadata refuses fixture editing, never normalizes.
    public static func settings(in draft: NativeExhibitionDraft, lightId: String) throws -> DocumentValue {
        guard UUID(uuidString: lightId) != nil else { throw invalid() }
        guard let row = try bindings(in: draft.candidate).first(where: { $0["lightId"].string?.lowercased() == lightId.lowercased() }) else {
            return .object(["fixture": .string(Self.none.rawValue)])
        }
        guard case .object(var fields) = row,
              Set(fields.keys).isSubset(of: ["lightId", "fixture", "aim", "penumbra", "range", "temperatureKelvin", "purpose", "basePlacement"]),
              fields["purpose"].map({ $0 == .string("room-base") }) ?? true,
              row["fixture"].string.flatMap(Self.init(rawValue:)) != nil else { throw invalid() }
        try RoomBaseLightIntent.checkMarker(row)
        fields.removeValue(forKey: "lightId")
        return .object(fields)
    }
    public func settings(in draft: NativeExhibitionDraft, lightId: String, type: NativeLightType) throws -> DocumentValue {
        guard supports(type) else { throw Self.invalid() }
        var value = try Self.settings(in: draft, lightId: lightId)
        value["fixture"] = .string(rawValue)
        return value
    }
    func applying(to candidate: DocumentValue, base: NativeExhibitionDraft, lightId: String, type: NativeLightType) throws -> DocumentValue {
        try Self.applying(settings: settings(in: base, lightId: lightId, type: type), to: candidate, base: base, lightId: lightId)
    }
    static func applying(settings input: DocumentValue, to candidate: DocumentValue, base: NativeExhibitionDraft, lightId: String) throws -> DocumentValue {
        guard let light = candidate["lights"].items?.first(where: { $0["id"].string?.lowercased() == lightId.lowercased() }) else { throw Self.invalid() }
        var settings: DocumentValue
        if let original = base.candidate["lights"].items?.first(where: { $0["id"].string?.lowercased() == lightId.lowercased() }) {
            settings = try RoomBaseLightIntent.settingsForEdit(input, light: light, original: original, base: base.candidate)
        } else {
            // The existing admitted light-addition path has no previous intent.
            // It must not borrow metadata from another light or infer placement.
            try RoomBaseLightIntent.checkMarker(input); settings = input
        }
        settings["lightId"] = .string(lightId)
        var rows = try Self.bindings(in: base.candidate)
        if let index = rows.firstIndex(where: { $0["lightId"].string?.lowercased() == lightId.lowercased() }) { rows[index] = settings }
        else { rows.append(settings) }
        let value: DocumentValue = .object(["version": .integer(1), "lights": .array(rows)])
        guard rows.count <= 32, try JSONEncoder().encode(value).count <= 16_384 else { throw Self.invalid() }
        var next = candidate
        if next["extensions"] == .null { next["extensions"] = .object([:]) }
        next["extensions"][Self.namespace] = value
        return next
    }
}
