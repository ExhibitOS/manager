// SPDX-License-Identifier: Apache-2.0
import Foundation

/// One held wall-texture command over an exact native frame. The shared
/// reducer supplies a proposal; existing native texture domain checks own it.
public struct SurfaceMaterialTextureRequest: Sendable {
    public let frame: ViewportFrame
    public let command: SurfaceMaterialTextureCommand
    public let value: DocumentValue
    public static func requireEditingPolicy(_ policy: NativeViewportPolicy, document: NativeDocumentState) throws {
        guard !document.dirty, document.pending == nil, let draft = document.draft, let etag = document.etag else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
        guard policy.exhibitionId == draft.id, policy.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        // Bounded provenance may hold an ordinary-size document. No bounded
        // edit-required capability or missing policy field is admitted here.
        guard policy.boundedProfile != nil, policy.boundedEditRequired == false, !policy.boundedReadOnly,
              policy.editingCapability == nil else { throw BridgeFailure("NATIVE_SURFACE_CAPABILITY_UNAVAILABLE") }
    }
    public init(frame: ViewportFrame, input: DocumentValue) throws {
        guard !frame.disabled, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
              case .object(let fields) = input, Set(fields.keys) == ["type", "surfaceId", "texture"],
              input["type"].string == "set-material-texture", let id = input["surfaceId"].string,
              frame.selection == ViewportSelection(kind: "surface", id: id),
              frame.draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == id && $0["type"].string == "wall" }) == true else { throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED") }
        let scalars = try SurfaceMaterialCommand.assignments(in: frame.draft)
        let existing = scalars.first(where: { $0.key.lowercased() == id.lowercased() })?.value
        command = SurfaceMaterialTextureCommand(surfaceId: id, material: try existing.map(SurfaceMaterial.init(value:)),
            texture: try SurfaceMaterialTextureBinding(value: input["texture"]))
        guard case .object(var request) = try NativeGeometryAction.surfaceMaterialTexture(command).encodedValue(in: frame.draft, selected: frame.selection) else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
        request["version"] = .integer(1); request["epoch"] = .string(frame.epoch); request["sequence"] = .integer(Int64(frame.sequence))
        request["tenantId"] = .string(frame.tenantId); request["principalId"] = .string(frame.principalId)
        request["requestId"] = .string(UUID().uuidString.lowercased())
        value = .object(request); self.frame = frame
        guard try JSONEncoder().encode(value).count <= 65_536 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
    }
    public func checkedReply(_ reply: DocumentValue) throws -> SurfaceMaterialTexturePreview {
        guard case .object(let fields) = reply,
              Set(fields.keys) == ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "surfaceId", "candidate"],
              reply["surfaceId"].string == command.surfaceId,
              try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        }
        return try SurfaceMaterialTexturePreview(candidate: reply["candidate"], base: frame.draft, command: command, selected: frame.selection)
    }
    public func applyReply(_ reply: DocumentValue, to document: NativeDocumentState) throws -> NativeExhibitionDraft {
        guard !document.dirty, document.pending == nil, let draft = document.draft,
              draft == frame.draft, document.etag == frame.baseETag, document.workspaceId == frame.workspaceId,
              document.tenantId == frame.tenantId, document.principalId == frame.principalId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return try checkedReply(reply).apply(to: draft, selected: frame.selection, command: command)
    }
}
