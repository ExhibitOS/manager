// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct ViewportSelection: Codable, Equatable, Sendable {
    public let kind: String
    public let id: String
    public init(kind: String, id: String) { self.kind = kind; self.id = id }
    public func belongs(to draft: NativeExhibitionDraft) -> Bool {
        let collection = ["room":"rooms", "surface":"surfaces", "opening":"openings", "placement":"placements"]
        guard let key = collection[kind], UUID(uuidString: id) != nil else { return false }
        return draft.candidate[key].items?.contains(where: { $0["id"].string == id }) == true
    }
}
public enum NativeGeometryAction: Sendable {
    case whiteCube(roomId: String), addRoom(name: String), addLight(roomId: String)
    case installation(InstallationCommand)
    case architecture(ArchitectureCommand)
    case surfaceMaterial(SurfaceMaterialCommand)
    public func encodedValue(in draft: NativeExhibitionDraft? = nil, selected: ViewportSelection? = nil) throws -> DocumentValue {
        switch self {
        case .whiteCube(let id): return .object(["type":.string("white-cube"),"roomId":.string(id)])
        case .addRoom(let name): return .object(["type":.string("add-room"),"name":.string(name)])
        case .addLight(let id): return .object(["type":.string("add-light"),"roomId":.string(id)])
        case .installation(let command): return try command.encodedValue()
        case .architecture(let command):
            guard let draft else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
            return try command.encodedValue(in: draft, selected: selected)
        case .surfaceMaterial(let command):
            guard let draft else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
            return try command.encodedValue(in: draft, selected: selected)
        }
    }
}
/// Display capability only; it grants no editing or account authority.
public enum ViewportPresentation: String, Codable, Hashable, Sendable {
    case threeD = "3d", plan = "plan"
}
public struct ViewportAcceptance: Decodable, Sendable {
    public let accepted: Bool
    public let presentations: [ViewportPresentation]?
    public let capabilities: [String]?
    public var supportsArtworkThumbnail: Bool { accepted && capabilities?.contains("artwork-thumbnail-v1") == true }
    public var supportsFindSelection: Bool { accepted && capabilities?.contains("find-selection-v1") == true }
    public var supportsArtworkPlacement: Bool { accepted && capabilities?.contains("place-artwork-v1") == true }
    public var supportsFurnishingVisuals: Bool { accepted && capabilities?.contains("furnishing-visuals-v1") == true }
    public var supportsSpaceSettings: Bool { accepted && capabilities?.contains("space-settings-v1") == true }
    public var supportsLightInspector: Bool { accepted && capabilities?.contains("light-inspector-v1") == true }
    public var supportsBenchInspector: Bool { accepted && capabilities?.contains("bench-inspector-v1") == true }
    public var supportsPlaqueInspector: Bool { accepted && capabilities?.contains("plaque-inspector-v1") == true }
    public var supportsPlan: Bool {
        accepted && presentations?.contains(.threeD) == true && presentations?.contains(.plan) == true
    }
}
public struct ViewportFrame: Encodable, Sendable {
    public let version = 1
    public let epoch: String
    public let sequence: Int
    public let tenantId: String
    public let principalId: String
    public let draft: NativeExhibitionDraft
    public let selection: ViewportSelection?
    public let mode: String
    public let view: String
    public let cameraSequence: Int
    public let transparent: Bool
    public let disabled: Bool
    public let presentation: ViewportPresentation
    public init(epoch: String, sequence: Int, tenantId: String, principalId: String, draft: NativeExhibitionDraft, selection: ViewportSelection?, mode: String, view: String, disabled: Bool, cameraSequence: Int = 0, transparent: Bool = true, presentation: ViewportPresentation = .threeD) {
        self.epoch = epoch; self.sequence = sequence; self.tenantId = tenantId; self.principalId = principalId; self.draft = draft; self.selection = selection; self.mode = mode; self.view = view; self.disabled = disabled; self.cameraSequence = cameraSequence; self.transparent = transparent; self.presentation = presentation
    }
    private enum CodingKeys: String, CodingKey { case version, epoch, sequence, tenantId, principalId, draft, selection, mode, view, disabled, cameraSequence, transparent, presentation }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(version, forKey: .version); try c.encode(epoch, forKey: .epoch); try c.encode(sequence, forKey: .sequence); try c.encode(tenantId, forKey: .tenantId); try c.encode(principalId, forKey: .principalId); try c.encode(draft, forKey: .draft)
        if let selection { try c.encode(selection, forKey: .selection) } else { try c.encodeNil(forKey: .selection) }
        if presentation == .plan { try c.encode(presentation, forKey: .presentation) }
        try c.encode(mode, forKey: .mode); try c.encode(view, forKey: .view); try c.encode(disabled, forKey: .disabled); try c.encode(cameraSequence, forKey: .cameraSequence); try c.encode(transparent, forKey: .transparent)
    }
}
extension NativeExhibitionDraft {
    /// The viewport may propose geometry only. Native identity, approved asset
    /// inventory/rights, metadata, scripts, extensions and accessibility survive.
    /// Platform's shared full validator runs before a proposal; save revalidates.
    public mutating func acceptGeometry(_ candidate: DocumentValue) throws {
        guard case .object(let old) = self.candidate, case .object(let next) = candidate,
              Set(old.keys) == Set(next.keys) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        let allowed = Set(["rooms", "surfaces", "openings", "placements", "lights"])
        for key in old.keys where !allowed.contains(key) {
            guard old[key] == next[key] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        }
        for key in allowed { guard next[key]?.items != nil else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") } }
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        try replaceCandidate(candidate)
    }
}
