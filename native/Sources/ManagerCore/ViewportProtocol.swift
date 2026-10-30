// SPDX-License-Identifier: Apache-2.0
import Foundation
import CoreGraphics

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
    case surfaceMaterialTexture(SurfaceMaterialTextureCommand)
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
        case .surfaceMaterialTexture(let command):
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
    public var supportsSharedFurnishingVisuals: Bool { accepted && capabilities?.contains("furnishing-visuals-v2") == true }
    public var supportsSpaceSettings: Bool { accepted && capabilities?.contains("space-settings-v1") == true }
    public var supportsLightInspector: Bool { accepted && capabilities?.contains("light-inspector-v1") == true }
    public var supportsLightFixture: Bool { accepted && capabilities?.contains("light-fixture-aim-v1") == true }
    public var supportsBenchInspector: Bool { accepted && capabilities?.contains("bench-inspector-v1") == true }
    public var supportsPlaqueInspector: Bool { accepted && capabilities?.contains("plaque-inspector-v1") == true }
    public var supportsSurfaceMaterialTextures: Bool { accepted && capabilities?.contains(SurfaceMaterialTextureBinding.capability) == true }
    public var supportsEntrancePreview: Bool { accepted && capabilities?.contains("entrance-preview-v1") == true }
    public var supportsPlan: Bool {
        accepted && presentations?.contains(.threeD) == true && presentations?.contains(.plan) == true
    }
}

public enum EntrancePreviewAction: String, Sendable { case show, replay, walk, thumbnail, close }
public enum EntrancePreviewDevice: String, CaseIterable, Sendable { case desktop, mobile }
public enum EntrancePreviewScenario: String, CaseIterable, Sendable { case normal, slow, imageFailed = "image-failed" }

/// Read-only authoring signals. None are inserted into the draft or OEX data.
public struct ViewportEntrancePreviewRequest: Sendable {
    public let frame: ViewportFrame
    public let value: DocumentValue
    public let action: EntrancePreviewAction
    public let device: EntrancePreviewDevice
    public init(frame: ViewportFrame, action: EntrancePreviewAction, device: EntrancePreviewDevice, scenario: EntrancePreviewScenario, reducedMotion: Bool, theme: CoverEntryTheme? = nil, locale: String? = nil) throws {
        guard frame.sequence > 0, [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        if let locale {
            guard locale.utf16.count <= 35, locale.range(of: "^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$", options: .regularExpression) != nil else { throw AuthoringFailure("VIEWER_ENTRANCE_INVALID") }
        }
        self.frame = frame; self.action = action; self.device = device
        value = .object([
            "version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
            "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased()),
            "type": .string(action.rawValue), "device": .string(device.rawValue), "scenario": .string(scenario.rawValue), "reducedMotion": .bool(reducedMotion),
            "theme": theme.map { .string($0.rawValue) } ?? .null, "locale": locale.map { .string($0) } ?? .null
        ])
    }
    public func checkedReply(_ reply: DocumentValue) throws -> ViewportEntrancePreviewReply {
        let binding: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
        let extra: Set<String> = ["baseDraft", "theme", "locale", "device", "scenario", "reducedMotion", "status", "mode", "renderGeneration", "imageStatus", "viewport", "snapshotRect"]
        guard case .object(let fields) = reply, Set(fields.keys) == binding.union(extra),
              try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in binding { guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") } }
        for key in ["theme", "locale", "device", "scenario", "reducedMotion"] { guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") } }
        guard reply["baseDraft"] == frame.draft.document, reply["status"].string == "ready",
              reply["mode"].string == (action == .close ? "editor" : action == .walk ? "walk" : "cover"),
              let renderGeneration = reply["renderGeneration"].integer, (1...9_007_199_254_740_991).contains(renderGeneration),
              ["absent", "ready", "failed"].contains(reply["imageStatus"].string ?? ""),
              case .object(let viewport) = reply["viewport"], Set(viewport.keys) == Set(["width", "height"]),
              let width = reply["viewport"]["width"].double, let height = reply["viewport"]["height"].double,
              width.isFinite, height.isFinite, (1...4096).contains(width), (1...4096).contains(height),
              device != .mobile || (width == 360 && height == 740),
              device != .desktop || width >= 768,
              case .object(let rectangle) = reply["snapshotRect"], Set(rectangle.keys) == Set(["x", "y", "width", "height"]),
              let x = rectangle["x"]?.double, let y = rectangle["y"]?.double, let w = rectangle["width"]?.double, let h = rectangle["height"]?.double,
              [x, y, w, h].allSatisfy({ $0.isFinite }), x >= 0, y >= 0, w > 0, h > 0, x + w <= width, y + h <= height else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return ViewportEntrancePreviewReply(value: reply, renderGeneration: renderGeneration, snapshotRect: CGRect(x: x, y: y, width: w, height: h))
    }
    /// A late result may only populate the currently requested preview context.
    public func matchesContext(draft: NativeExhibitionDraft, device: EntrancePreviewDevice, scenario: EntrancePreviewScenario, reducedMotion: Bool, theme: CoverEntryTheme?, locale: String?) -> Bool {
        guard frame.draft == draft, value["device"].string == device.rawValue,
              value["scenario"].string == scenario.rawValue, value["reducedMotion"] == .bool(reducedMotion) else { return false }
        return value["theme"] == (theme.map { .string($0.rawValue) } ?? .null) && value["locale"] == (locale.map { .string($0) } ?? .null)
    }
}

public struct ViewportEntrancePreviewReply: Sendable {
    public let value: DocumentValue
    public let renderGeneration: Int64
    public let snapshotRect: CGRect
    public func recheck(_ reply: DocumentValue, request: ViewportEntrancePreviewRequest) throws {
        let current = try request.checkedReply(reply)
        guard current.value == value else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
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
    /// Used only with the controller's actually acknowledged frame. An old
    /// epoch or a preview-only draft cannot acknowledge host restoration.
    public func confirmsHostRecovery(replacing previousEpoch: String, ownerEpoch: String?, acceptedSequence: Int, expectedDraft: NativeExhibitionDraft, tenantId: String, principalId: String) -> Bool {
        epoch != previousEpoch && epoch == ownerEpoch && UUID(uuidString: epoch) != nil && sequence > 0 && sequence == acceptedSequence
        && self.tenantId == tenantId && self.principalId == principalId && draft == expectedDraft
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
