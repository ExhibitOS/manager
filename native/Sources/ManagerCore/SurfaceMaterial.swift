// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing Platform StudioMaterials v1; no presets, textures or audio policy.
public struct SurfaceMaterial: Equatable, Sendable {
    public let color: String
    public let roughness: Double
    public let metalness: Double
    public init(color: String, roughness: Double, metalness: Double) throws {
        guard color.range(of: "^#[0-9a-f]{6}$", options: .regularExpression) != nil,
              [roughness, metalness].allSatisfy({ $0.isFinite && (0...1).contains($0) }) else {
            throw AuthoringFailure("SURFACE_MATERIAL_INVALID")
        }
        self.color = color; self.roughness = roughness; self.metalness = metalness
    }
    fileprivate init(value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == Set(["color", "roughness", "metalness"]),
              let color = fields["color"]?.string, let roughness = fields["roughness"]?.double,
              let metalness = fields["metalness"]?.double else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
        try self.init(color: color, roughness: roughness, metalness: metalness)
    }
    public var value: DocumentValue { .object(["color": .string(color), "roughness": .number(roughness), "metalness": .number(metalness)]) }
}

public struct SurfaceMaterialCommand: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/materials"
    public let surfaceId: String
    public let material: SurfaceMaterial?
    public init(surfaceId: String, material: SurfaceMaterial?) { self.surfaceId = surfaceId; self.material = material }
    public var selection: ViewportSelection { .init(kind: "surface", id: surfaceId) }

    /// Refuse unsupported known metadata rather than silently discarding it.
    fileprivate static func assignments(in draft: NativeExhibitionDraft) throws -> [String: DocumentValue] {
        guard case .object(let candidate) = draft.candidate else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
        guard let rawExtensions = candidate["extensions"] else { return [:] }
        guard case .object(let extensions) = rawExtensions else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
        guard let raw = extensions[namespace] else { return [:] }
        guard case .object(let fields) = raw, Set(fields.keys) == Set(["version", "surfaces"]),
              fields["version"] == .integer(1), case .object(let entries) = fields["surfaces"], entries.count <= 64,
              try JSONEncoder().encode(raw).count <= 16_384 else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
        let known = Set((draft.candidate["surfaces"].items ?? []).compactMap { $0["id"].string?.lowercased() })
        var seen = Set<String>()
        for (id, value) in entries {
            guard UUID(uuidString: id) != nil, known.contains(id.lowercased()), seen.insert(id.lowercased()).inserted else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
            _ = try SurfaceMaterial(value: value)
        }
        return entries
    }
    private func requireSelection(_ selected: ViewportSelection?, in draft: NativeExhibitionDraft) throws {
        guard selected == selection, selection.belongs(to: draft) else { throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED") }
    }
    public func encodedValue(in draft: NativeExhibitionDraft, selected: ViewportSelection?) throws -> DocumentValue {
        try requireSelection(selected, in: draft)
        _ = try expectedCandidate(in: draft, selected: selected)
        return .object(["type": .string("set-material"), "surfaceId": .string(surfaceId), "material": material?.value ?? .null])
    }
    private func expectedCandidate(in draft: NativeExhibitionDraft, selected: ViewportSelection?) throws -> DocumentValue {
        try requireSelection(selected, in: draft)
        var entries = try Self.assignments(in: draft)
        for key in Array(entries.keys) where key.lowercased() == surfaceId.lowercased() { entries.removeValue(forKey: key) }
        if let material { entries[surfaceId] = material.value }
        guard entries.count <= 64 else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        candidate["extensions"][Self.namespace] = .object(["version": .integer(1), "surfaces": .object(entries)])
        guard try JSONEncoder().encode(candidate["extensions"][Self.namespace]).count <= 16_384 else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
        let template = "org.exhibitos.studio/template"
        if candidate["extensions"][template] != .null {
            guard case .object = candidate["extensions"][template], case .bool = candidate["extensions"][template]["modified"] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            candidate["extensions"][template]["modified"] = .bool(true)
        }
        return candidate
    }
    public func checkProposal(_ candidate: DocumentValue, base: NativeExhibitionDraft, selected: ViewportSelection?) throws {
        guard try candidate == expectedCandidate(in: base, selected: selected) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
    }
}

/// A shared-validator result is transient until explicit Apply. No local draft
/// is changed by Preview or Cancel; stale base, selection or input is refused.
public struct SurfaceMaterialPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft
    public let command: SurfaceMaterialCommand
    public let candidate: DocumentValue
    public let displayDraft: NativeExhibitionDraft
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: SurfaceMaterialCommand, selected: ViewportSelection?) throws {
        try command.checkProposal(candidate, base: base, selected: selected)
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); self.displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, selected: ViewportSelection?, command: SurfaceMaterialCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        try command.checkProposal(candidate, base: draft, selected: selected)
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}

public struct SurfaceMaterialFormBuffer: Equatable, Sendable {
    public let surfaceId: String
    public var usesOverride: Bool
    public var color: String
    public var roughness: String
    public var metalness: String
    public init(draft: NativeExhibitionDraft, selection: ViewportSelection) throws {
        guard selection.kind == "surface", selection.belongs(to: draft) else { throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED") }
        let entries = try SurfaceMaterialCommand.assignments(in: draft)
        let value = entries.first { $0.key.lowercased() == selection.id.lowercased() }?.value
        let material = try value.map { try SurfaceMaterial(value: $0) }
        surfaceId = selection.id; usesOverride = value != nil
        color = material?.color ?? "#f4f1e8"; roughness = String(material?.roughness ?? 0.8); metalness = String(material?.metalness ?? 0)
    }
    public func command() throws -> SurfaceMaterialCommand {
        var value: SurfaceMaterial?
        if usesOverride {
            guard let roughness = Double(roughness.trimmingCharacters(in: .whitespacesAndNewlines)),
                  let metalness = Double(metalness.trimmingCharacters(in: .whitespacesAndNewlines)) else { throw AuthoringFailure("SURFACE_MATERIAL_INVALID") }
            value = try .init(color: color.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(), roughness: roughness, metalness: metalness)
        }
        return .init(surfaceId: surfaceId, material: value)
    }
}
