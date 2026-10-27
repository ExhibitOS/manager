// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Protected catalogue metadata only. Parsing a descriptor does not admit or
/// decode its image; the shared preview byte/profile contract is checked by the
/// existing authenticated loader when that contract is qualified.
public struct NativeSurfaceMaterialCatalog: Equatable, Sendable {
    public struct Preview: Equatable, Sendable {
        public let url: String
        public let mime: String
        public let bytes: Int64
        public let sha256: String
        public let width: Int64
        public let height: Int64
    }
    public struct Item: Equatable, Sendable {
        public let id: String
        public let version: Int
        public let name: String
        public let material: SurfaceMaterial
        public let texture: SurfaceMaterialTextureBinding
        public let preview: Preview?
    }
    public let items: [Item]

    public init(value: DocumentValue, tenantId: String) throws {
        guard UUID(uuidString: tenantId) != nil,
              case .object(let envelope) = value, Set(envelope.keys) == ["version", "items"],
              envelope["version"] == .integer(1), let rows = envelope["items"]?.items,
              rows.count == 2 else { throw AuthoringFailure("MATERIAL_CATALOG_INVALID") }
        var checked: [Item] = []
        var seen = Set<String>()
        for row in rows {
            guard case .object(let fields) = row,
                  Set(fields.keys) == ["id", "version", "name", "defaults", "preview"],
                  let id = fields["id"]?.string, seen.insert(id).inserted,
                  fields["version"] == .integer(1), let name = fields["name"]?.string,
                  !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                  case .object(let defaults) = fields["defaults"], Set(defaults.keys) == ["material", "texture"],
                  let materialValue = defaults["material"], let textureValue = defaults["texture"],
                  let previewValue = fields["preview"] else { throw AuthoringFailure("MATERIAL_CATALOG_INVALID") }
            let texture = try SurfaceMaterialTextureBinding(value: textureValue)
            let material = try SurfaceMaterial(value: materialValue)
            let recommended = try SurfaceMaterialTextureBinding.recommendedRepeat(for: id)
            guard texture.catalogId == id, texture.repeatMeters == [recommended, recommended],
                  texture.rotation == 0, texture.normalScale == 1,
                  material.color == "#ffffff", material.roughness == 1, material.metalness == 0 else { throw AuthoringFailure("MATERIAL_CATALOG_INVALID") }
            checked.append(Item(id: id, version: 1, name: name, material: material, texture: texture,
                                preview: try Self.preview(previewValue, tenantId: tenantId, id: id)))
        }
        guard seen == [SurfaceMaterialTextureBinding.paint, SurfaceMaterialTextureBinding.plaster] else { throw AuthoringFailure("MATERIAL_CATALOG_INVALID") }
        items = checked
    }
    private static func preview(_ value: DocumentValue, tenantId: String, id: String) throws -> Preview? {
        if value == .null { return nil }
        guard case .object(let preview) = value,
              Set(preview.keys) == ["url", "mime", "bytes", "sha256", "width", "height"],
              let url = preview["url"]?.string,
              url == "/api/v1/tenants/\(tenantId.lowercased())/studio/materials/\(id)/1/preview",
              preview["mime"] == .string("image/png"),
              let bytes = preview["bytes"]?.integer, bytes > 0,
              let sha = preview["sha256"]?.string,
              sha.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil,
              let width = preview["width"]?.integer, width > 0,
              let height = preview["height"]?.integer, height > 0 else { throw AuthoringFailure("MATERIAL_CATALOG_INVALID") }
        return Preview(url: url, mime: "image/png", bytes: bytes, sha256: sha, width: width, height: height)
    }
}

/// The closed first-two-material binding agreed with the shared Studio owner.
/// It carries catalogue identity and physical UV controls, never file paths.
public struct SurfaceMaterialTextureBinding: Equatable, Sendable {
    public static let capability = "surface-material-textures-v1"
    public static let paint = "museum-matte-paint"
    public static let plaster = "fine-lime-plaster"
    public let catalogId: String
    public let catalogVersion: Int
    public let repeatMeters: [Double]
    public let rotation: Double
    public let normalScale: Double

    public init(catalogId: String, catalogVersion: Int = 1, repeatMeters: [Double], rotation: Double, normalScale: Double) throws {
        guard [Self.paint, Self.plaster].contains(catalogId), catalogVersion == 1,
              repeatMeters.count == 2, repeatMeters.allSatisfy({ $0.isFinite && (0.05...20).contains($0) }),
              rotation.isFinite, (-Double.pi...Double.pi).contains(rotation),
              normalScale.isFinite, (0...2).contains(normalScale) else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
        self.catalogId = catalogId; self.catalogVersion = catalogVersion
        self.repeatMeters = repeatMeters; self.rotation = rotation; self.normalScale = normalScale
    }
    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == ["pack", "repeatMeters", "rotation", "normalScale"],
              let packValue = fields["pack"], case .object(let pack) = packValue, Set(pack.keys) == ["id", "version"],
              let id = pack["id"]?.string, pack["version"] == .integer(1),
              let repeats = fields["repeatMeters"]?.items, repeats.count == 2, repeats.allSatisfy({ $0.double != nil }),
              let rotation = fields["rotation"]?.double, let normal = fields["normalScale"]?.double else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
        try self.init(catalogId: id, repeatMeters: repeats.compactMap(\.double), rotation: rotation, normalScale: normal)
    }
    public var value: DocumentValue {
        .object(["pack": .object(["id": .string(catalogId), "version": .integer(catalogVersion)]),
                 "repeatMeters": .vector(repeatMeters), "rotation": .number(rotation), "normalScale": .number(normalScale)])
    }
    public static func recommendedRepeat(for id: String) throws -> Double {
        guard [paint, plaster].contains(id) else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
        return id == paint ? 1 : 2
    }
    static func assignments(_ value: DocumentValue, in draft: NativeExhibitionDraft) throws -> [String: DocumentValue] {
        guard case .object(let entries) = value, entries.count <= 64 else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
        var seen = Set<String>()
        for (id, value) in entries {
            guard UUID(uuidString: id) != nil, seen.insert(id.lowercased()).inserted,
                  let surface = draft.candidate["surfaces"].items?.first(where: { $0["id"].string?.lowercased() == id.lowercased() }),
                  ["wall", "ceiling"].contains(surface["type"].string ?? "") else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
            _ = try Self(value: value)
        }
        return entries
    }
}

/// Model-owned raw text for both modes. Choosing another pack does not erase
/// incomplete or advanced input; only an explicit recommended-values action does.
public struct SurfaceMaterialTextureFormBuffer: Equatable, Sendable {
    public private(set) var catalogId: String?
    public var repeatU: String, repeatV: String, rotationDegrees: String, normalScale: String
    private var hasChosenPack: Bool
    private let originalRotation: Double?
    private let originalRotationText: String
    public init(draft: NativeExhibitionDraft, selection: ViewportSelection) throws {
        guard selection.kind == "surface", selection.belongs(to: draft),
              let surface = draft.candidate["surfaces"].items?.first(where: { $0["id"].string == selection.id }),
              ["wall", "ceiling"].contains(surface["type"].string ?? "") else { throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED") }
        let fields = try SurfaceMaterialCommand.namespaceFields(in: draft)
        var binding: SurfaceMaterialTextureBinding?
        if case .object(let textures) = fields?["surfaceTextures"],
           let value = textures.first(where: { $0.key.lowercased() == selection.id.lowercased() })?.value {
            binding = try SurfaceMaterialTextureBinding(value: value)
        }
        self.init(binding: binding)
    }
    public init(binding: SurfaceMaterialTextureBinding?) {
        catalogId = binding?.catalogId; hasChosenPack = binding != nil
        repeatU = binding.map { String($0.repeatMeters[0]) } ?? ""
        repeatV = binding.map { String($0.repeatMeters[1]) } ?? ""
        rotationDegrees = binding.map { String($0.rotation * 180 / .pi) } ?? "0"
        normalScale = binding.map { String($0.normalScale) } ?? "1"
        originalRotation = binding?.rotation; originalRotationText = rotationDegrees
    }
    public mutating func select(_ id: String) throws {
        _ = try SurfaceMaterialTextureBinding.recommendedRepeat(for: id)
        catalogId = id
        if !hasChosenPack { try useRecommendedValues(); hasChosenPack = true }
    }
    public mutating func remove() { catalogId = nil }
    public mutating func useRecommendedValues() throws {
        guard let catalogId else { return }
        let repeatMeters = try SurfaceMaterialTextureBinding.recommendedRepeat(for: catalogId)
        repeatU = String(repeatMeters); repeatV = String(repeatMeters); rotationDegrees = "0"; normalScale = "1"
    }
    public func binding() throws -> SurfaceMaterialTextureBinding? {
        guard let catalogId else { return nil }
        guard let u = Double(repeatU.trimmingCharacters(in: .whitespacesAndNewlines)),
              let v = Double(repeatV.trimmingCharacters(in: .whitespacesAndNewlines)),
              let degrees = Double(rotationDegrees.trimmingCharacters(in: .whitespacesAndNewlines)),
              let normal = Double(normalScale.trimmingCharacters(in: .whitespacesAndNewlines)) else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
        let rotation = rotationDegrees == originalRotationText ? (originalRotation ?? degrees * .pi / 180) : degrees * .pi / 180
        return try .init(catalogId: catalogId, repeatMeters: [u, v], rotation: rotation, normalScale: normal)
    }
}

/// Explicit atomic texture/scalar action. Shared Studio owns rendering and
/// export expansion; editing must not add resources or change other surfaces.
public struct SurfaceMaterialTextureCommand: Equatable, Sendable {
    public let surfaceId: String
    public let material: SurfaceMaterial?
    public let texture: SurfaceMaterialTextureBinding?
    public init(surfaceId: String, material: SurfaceMaterial?, texture: SurfaceMaterialTextureBinding?) {
        self.surfaceId = surfaceId; self.material = material; self.texture = texture
    }
    public var selection: ViewportSelection { .init(kind: "surface", id: surfaceId) }
    public func encodedValue(in draft: NativeExhibitionDraft, selected: ViewportSelection?) throws -> DocumentValue {
        _ = try expectedCandidate(in: draft, selected: selected)
        return .object(["type": .string("set-material-texture"), "surfaceId": .string(surfaceId),
                        "material": material?.value ?? .null, "texture": texture?.value ?? .null])
    }
    private func expectedCandidate(in draft: NativeExhibitionDraft, selected: ViewportSelection?) throws -> DocumentValue {
        guard selected == selection, selection.belongs(to: draft),
              let surface = draft.candidate["surfaces"].items?.first(where: { $0["id"].string == surfaceId }),
              ["wall", "ceiling"].contains(surface["type"].string ?? "") else { throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED") }
        var fields = try SurfaceMaterialCommand.namespaceFields(in: draft) ?? ["version": .integer(1), "surfaces": .object([:])]
        var scalars = try SurfaceMaterialCommand.assignments(in: draft)
        var textures: [String: DocumentValue] = [:]
        if let raw = fields["surfaceTextures"] { textures = try SurfaceMaterialTextureBinding.assignments(raw, in: draft) }
        for key in scalars.keys.filter({ $0.lowercased() == surfaceId.lowercased() }) { scalars.removeValue(forKey: key) }
        for key in textures.keys.filter({ $0.lowercased() == surfaceId.lowercased() }) { textures.removeValue(forKey: key) }
        if let material { scalars[surfaceId] = material.value }
        if let texture { textures[surfaceId] = texture.value }
        fields["surfaces"] = .object(scalars)
        if textures.isEmpty { fields.removeValue(forKey: "surfaceTextures") }
        else { fields["surfaceTextures"] = .object(textures) }
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        candidate["extensions"][SurfaceMaterialCommand.namespace] = .object(fields)
        var checked = draft; try checked.replaceCandidate(candidate)
        _ = try SurfaceMaterialCommand.namespaceFields(in: checked)
        try SurfaceMaterialCommand.markTemplateModified(in: &candidate)
        return candidate
    }
    public func checkProposal(_ candidate: DocumentValue, base: NativeExhibitionDraft, selected: ViewportSelection?) throws {
        guard try candidate == expectedCandidate(in: base, selected: selected) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
    }
}

public struct SurfaceMaterialTexturePreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft
    public let command: SurfaceMaterialTextureCommand
    public let candidate: DocumentValue
    public let displayDraft: NativeExhibitionDraft
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: SurfaceMaterialTextureCommand, selected: ViewportSelection?) throws {
        try command.checkProposal(candidate, base: base, selected: selected)
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, selected: ViewportSelection?, command: SurfaceMaterialTextureCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        try command.checkProposal(candidate, base: draft, selected: selected)
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}
