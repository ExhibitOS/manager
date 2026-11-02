// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Metadata correspondence only. The shared architectural-finishes-v1 producer
/// owns generated geometry, opening clearance, collisions and artwork bounds.
public struct WallFinishSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/architectural-finishes"
    public let surfaceId: String
    public let depth: Double, preset: String, trimWidth: Double, baseboardHeight: Double
    public let ceilingBand: Bool
    public init(surfaceId: String, depth: Double, preset: String, trimWidth: Double, baseboardHeight: Double, ceilingBand: Bool) throws {
        guard UUID(uuidString: surfaceId) != nil, depth.isFinite, (0.02...0.2).contains(depth),
              ["plain", "painted-edge", "dark-edge"].contains(preset), trimWidth.isFinite, (0.02...0.08).contains(trimWidth),
              baseboardHeight.isFinite, (0.04...0.15).contains(baseboardHeight) else { throw Self.invalid() }
        self.surfaceId = surfaceId; self.depth = depth; self.preset = preset; self.trimWidth = trimWidth
        self.baseboardHeight = baseboardHeight; self.ceilingBand = ceilingBand
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("WALL_FINISH_INVALID") }
    public var value: DocumentValue {
        .object(["surfaceId": .string(surfaceId), "depth": .number(depth), "preset": .string(preset),
                 "trimWidth": .number(trimWidth), "baseboardHeight": .number(baseboardHeight), "ceilingBand": .bool(ceilingBand)])
    }
    private init(_ value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == ["surfaceId", "depth", "preset", "trimWidth", "baseboardHeight", "ceilingBand"],
              let id = value["surfaceId"].string, let depth = value["depth"].double, let preset = value["preset"].string,
              let trim = value["trimWidth"].double, let base = value["baseboardHeight"].double,
              case .bool(let band) = value["ceilingBand"] else { throw Self.invalid() }
        try self.init(surfaceId: id, depth: depth, preset: preset, trimWidth: trim, baseboardHeight: base, ceilingBand: band)
    }
    static func rows(in draft: NativeExhibitionDraft) throws -> [DocumentValue] {
        let value = draft.candidate["extensions"][namespace]
        if value == .null { return [] }
        guard case .object(let fields) = value, Set(fields.keys) == ["version", "walls"], value["version"] == .integer(1),
              let rows = value["walls"].items, rows.count <= 8, try JSONEncoder().encode(value).count <= 8192 else { throw invalid() }
        var seen = Set<String>()
        for row in rows {
            let settings = try Self(row)
            guard seen.insert(settings.surfaceId.lowercased()).inserted else { throw invalid() }
        }
        return rows
    }
    public static func read(in draft: NativeExhibitionDraft, surfaceId: String) throws -> Self? {
        try rows(in: draft).first { $0["surfaceId"].string?.lowercased() == surfaceId.lowercased() }.map { try Self($0) }
    }
}

public struct WallFinishCommand: Equatable, Sendable {
    public let surfaceId: String
    public let settings: WallFinishSettings?
    public init(surfaceId: String, settings: WallFinishSettings?) throws {
        guard UUID(uuidString: surfaceId) != nil, settings == nil || settings?.surfaceId == surfaceId else { throw WallFinishSettings.invalid() }
        self.surfaceId = surfaceId; self.settings = settings
    }
    public var value: DocumentValue {
        if let settings { return .object(["type": .string("set-wall-finish"), "wall": settings.value]) }
        return .object(["type": .string("remove-wall-finish"), "surfaceId": .string(surfaceId)])
    }
    public func proposedCandidate(in draft: NativeExhibitionDraft, selectedSurfaceId: String) throws -> DocumentValue {
        guard selectedSurfaceId == surfaceId,
              let surface = draft.candidate["surfaces"].items?.first(where: { $0["id"].string == surfaceId && $0["type"].string == "wall" }),
              draft.rooms.contains(where: { $0["id"] == surface["roomId"] }) else { throw WallFinishSettings.invalid() }
        var rows = try WallFinishSettings.rows(in: draft)
        let index = rows.firstIndex { $0["surfaceId"].string?.lowercased() == surfaceId.lowercased() }
        if let settings { if let index { rows[index] = settings.value } else { rows.append(settings.value) } }
        else { guard let index else { throw WallFinishSettings.invalid() }; rows.remove(at: index) }
        let value: DocumentValue = .object(["version": .integer(1), "walls": .array(rows)])
        guard rows.count <= 8, try JSONEncoder().encode(value).count <= 8192 else { throw WallFinishSettings.invalid() }
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        guard case .object = candidate["extensions"] else { throw WallFinishSettings.invalid() }
        candidate["extensions"][WallFinishSettings.namespace] = value
        let template = "org.exhibitos.studio/template"
        if candidate["extensions"][template] != .null {
            guard case .object = candidate["extensions"][template], case .bool = candidate["extensions"][template]["modified"] else { throw WallFinishSettings.invalid() }
            candidate["extensions"][template]["modified"] = .bool(true)
        }
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
}

public struct WallFinishFormBuffer: Equatable, Sendable {
    public let surfaceId: String
    public var enabled: Bool, ceilingBand: Bool
    public var depth: String, preset: String, trimWidth: String, baseboardHeight: String
    public init(draft: NativeExhibitionDraft, surfaceId: String) throws {
        let settings = try WallFinishSettings.read(in: draft, surfaceId: surfaceId)
        self.surfaceId = surfaceId; enabled = settings != nil; ceilingBand = settings?.ceilingBand ?? true
        depth = String(settings?.depth ?? 0.12); preset = settings?.preset ?? "plain"
        trimWidth = String(settings?.trimWidth ?? 0.04); baseboardHeight = String(settings?.baseboardHeight ?? 0.1)
        guard draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == surfaceId && $0["type"].string == "wall" }) == true else { throw WallFinishSettings.invalid() }
    }
    public func command() throws -> WallFinishCommand {
        if !enabled { return try .init(surfaceId: surfaceId, settings: nil) }
        func number(_ text: String) throws -> Double {
            guard let value = Double(text.trimmingCharacters(in: .whitespacesAndNewlines)), value.isFinite else { throw WallFinishSettings.invalid() }; return value
        }
        return try .init(surfaceId: surfaceId, settings: .init(surfaceId: surfaceId, depth: number(depth), preset: preset,
            trimWidth: number(trimWidth), baseboardHeight: number(baseboardHeight), ceilingBand: ceilingBand))
    }
}

public struct WallFinishPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft, displayDraft: NativeExhibitionDraft
    public let command: WallFinishCommand, candidate: DocumentValue
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: WallFinishCommand, selectedSurfaceId: String) throws {
        guard try candidate == command.proposedCandidate(in: base, selectedSurfaceId: selectedSurfaceId) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, command: WallFinishCommand, selectedSurfaceId: String) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command,
              try candidate == command.proposedCandidate(in: draft, selectedSurfaceId: selectedSurfaceId) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}

public struct WallFinishViewportRequest: Sendable {
    public let frame: ViewportFrame, command: WallFinishCommand, value: DocumentValue
    public init(frame: ViewportFrame, command: WallFinishCommand) throws {
        guard !frame.disabled, frame.sequence > 0, frame.selection?.kind == "surface", frame.selection?.id == command.surfaceId,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        _ = try command.proposedCandidate(in: frame.draft, selectedSurfaceId: command.surfaceId)
        guard case .object(var fields) = command.value else { throw WallFinishSettings.invalid() }
        fields.merge(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                      "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased())]) { _, new in new }
        value = .object(fields); self.frame = frame; self.command = command
    }
    public func checkedReply(_ reply: DocumentValue) throws -> WallFinishPreview {
        guard case .object(let fields) = reply, Set(fields.keys) == ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "surfaceId", "candidate"],
              try JSONEncoder().encode(reply).count <= 1_100_000, reply["surfaceId"].string == command.surfaceId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        }
        return try .init(candidate: reply["candidate"], base: frame.draft, command: command, selectedSurfaceId: command.surfaceId)
    }
}
