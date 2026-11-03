// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A read-only image from the existing scene. The host separately rechecks its
/// clean local document and the authenticated saved response before/after use.
public struct SceneSnapshotViewportRequest: Sendable {
    public static let width = 640, height = 360, maximumPNGBytes = 512 * 1024
    public static let maximumBase64Bytes = 699_052
    public let frame: ViewportFrame
    public let value: DocumentValue
    public let saved: NativeStudioResponse
    public let viewpoint: DocumentValue
    public let placement: DocumentValue
    public let artwork: DocumentValue

    public init(frame: ViewportFrame, saved: NativeStudioResponse, viewpointId: String, placementId: String) throws {
        guard !frame.disabled, !frame.transparent, frame.presentation != .plan, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId, viewpointId, placementId].allSatisfy({ UUID(uuidString: $0) != nil }),
              saved.draft == frame.draft, saved.revision > 0,
              saved.etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil,
              saved.etag.hasPrefix("\"studio-r\(saved.revision)-"), frame.baseETag == nil || frame.baseETag == saved.etag,
              frame.draft.placements.allSatisfy({ placed in
                  guard let art = frame.draft.artworks.first(where: { $0["revisionId"] == placed["artworkRevisionId"] }) else { return false }
                  return art["rights"]["permissions"]["display"] == .bool(true) && art["rights"]["permissions"]["export"] == .bool(true)
              }),
              let placement = frame.draft.placements.first(where: { $0["id"].string == placementId }),
              let artwork = frame.draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] }),
              let viewpoint = frame.draft.candidate["extensions"]["org.exhibitos.studio/presentation"]["viewpoints"].items?.first(where: { $0["id"].string == viewpointId }),
              viewpoint["roomId"] == placement["roomId"] else { throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY") }
        self.frame = frame; self.saved = saved; self.viewpoint = viewpoint; self.placement = placement; self.artwork = artwork
        value = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                         "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased()),
                         "expectedETag": .string(saved.etag), "viewpointId": .string(viewpointId), "placementId": .string(placementId),
                         "width": .integer(Int64(Self.width)), "height": .integer(Int64(Self.height))])
    }

    public func checkedReply(_ reply: DocumentValue) throws -> SceneSnapshotResult {
        func closed(_ value: DocumentValue, _ keys: [String]) -> Bool {
            guard case .object(let fields) = value else { return false }; return Set(fields.keys) == Set(keys)
        }
        func point(_ value: DocumentValue) -> Bool {
            guard let numbers = value.items, numbers.count == 3 else { return false }
            return numbers.allSatisfy { $0.double.map { $0.isFinite && abs($0) <= 1_000_000 } == true }
        }
        guard closed(reply, ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "expectedETag", "saved", "candidateSha256", "camera", "width", "height", "target", "pngBase64"]),
              closed(reply["saved"], ["exhibitionId", "draftId", "revision", "etag"]),
              closed(reply["camera"], ["viewpointId", "roomId", "position", "target", "fov"]),
              closed(reply["target"], ["placementId", "artworkRevisionId", "assetId", "ready", "inFrustum", "centerRay"]) else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "expectedETag", "width", "height"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
        }
        guard reply["saved"]["exhibitionId"].string == saved.draft.id,
              reply["saved"]["draftId"] == saved.draft.document["id"],
              reply["saved"]["revision"].integer == Int64(saved.revision), reply["saved"]["etag"].string == saved.etag,
              reply["camera"]["viewpointId"] == value["viewpointId"], reply["camera"]["roomId"] == placement["roomId"],
              point(reply["camera"]["position"]), point(reply["camera"]["target"]), reply["camera"]["fov"] == viewpoint["fov"],
              reply["target"]["placementId"] == value["placementId"], reply["target"]["artworkRevisionId"] == placement["artworkRevisionId"],
              let asset = reply["target"]["assetId"].string, artwork["assets"].items?.contains(where: { $0["id"].string == asset }) == true,
              reply["target"]["ready"] == .bool(true), case .bool = reply["target"]["inFrustum"],
              let ray = reply["target"]["centerRay"].string, ["clear", "blocked", "unknown"].contains(ray),
              let sha = reply["candidateSha256"].string, sha.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil,
              let encoded = reply["pngBase64"].string, encoded.utf8.count <= Self.maximumBase64Bytes,
              let bytes = Data(base64Encoded: encoded), bytes.base64EncodedString() == encoded,
              (33...Self.maximumPNGBytes).contains(bytes.count),
              bytes.prefix(16) == Data([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82]) else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
        func dimension(_ offset: Int) -> Int { bytes[offset..<offset + 4].reduce(0) { ($0 << 8) | Int($1) } }
        guard dimension(16) == Self.width, dimension(20) == Self.height else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
        // The shared viewpointPose resolver supplies WORLD coordinates, including
        // the authored room transform. Do not compare these to room-local values
        // or duplicate the shared affine engine in Swift.
        return SceneSnapshotResult(png: bytes, candidateSha256: sha, camera: reply["camera"], target: reply["target"],
                                   savedRevision: saved.revision, savedETag: saved.etag,
                                   localEditVersion: saved.draft.document["editVersion"].integer!)
    }
}

public struct SceneSnapshotResult: Sendable {
    public let png: Data
    public let candidateSha256: String
    public let camera: DocumentValue
    public let target: DocumentValue
    public let savedRevision: Int
    public let savedETag: String
    public let localEditVersion: Int64
}
