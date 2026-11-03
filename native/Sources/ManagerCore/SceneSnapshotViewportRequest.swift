// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Ephemeral room-local camera input. It is never inserted into the document,
/// presentation extension, undo history or saved viewing plan.
public struct SceneSnapshotInspectionCamera: Sendable {
    public let roomId: String
    public let position: [Double]
    public let target: [Double]
    public let fov: Double
    public init(roomId: String, position: [Double], target: [Double], fov: Double) throws {
        guard UUID(uuidString: roomId) != nil, position.count == 3, target.count == 3,
              (position + target).allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }),
              zip(position, target).reduce(0, { $0 + pow($1.0 - $1.1, 2) }) > 1e-12,
              fov.isFinite, (10...120).contains(fov) else { throw AuthoringFailure("SCENE_SNAPSHOT_TARGET_INVALID") }
        self.roomId = roomId; self.position = position; self.target = target; self.fov = fov
    }
    public var value: DocumentValue {
        .object(["roomId": .string(roomId), "position": .vector(position), "target": .vector(target), "fov": .number(fov)])
    }
    fileprivate func belongs(to room: DocumentValue) -> Bool {
        guard room["id"].string == roomId,
              let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double,
              let depth = room["dimensions"]["depth"].double,
              [width, height, depth].allSatisfy({ $0.isFinite && $0 > 0 }) else { return false }
        return [position, target].allSatisfy { abs($0[0]) <= width / 2 && (0...height).contains($0[1]) && abs($0[2]) <= depth / 2 }
    }
}

public enum SceneSnapshotCamera: Sendable {
    case savedViewpoint(String)
    case inspection(SceneSnapshotInspectionCamera)
    public var isInspection: Bool { if case .inspection = self { true } else { false } }
}

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
    public let camera: SceneSnapshotCamera

    public init(frame: ViewportFrame, saved: NativeStudioResponse, viewpointId: String, placementId: String) throws {
        try self.init(frame: frame, saved: saved, camera: .savedViewpoint(viewpointId), placementId: placementId)
    }

    public init(frame: ViewportFrame, saved: NativeStudioResponse, camera: SceneSnapshotCamera, placementId: String) throws {
        guard !frame.disabled, !frame.transparent, frame.presentation != .plan, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId, placementId].allSatisfy({ UUID(uuidString: $0) != nil }),
              saved.draft == frame.draft, saved.revision > 0,
              saved.etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil,
              saved.etag.hasPrefix("\"studio-r\(saved.revision)-"), frame.baseETag == nil || frame.baseETag == saved.etag,
              frame.draft.placements.allSatisfy({ placed in
                  guard let art = frame.draft.artworks.first(where: { $0["revisionId"] == placed["artworkRevisionId"] }) else { return false }
                  return art["rights"]["permissions"]["display"] == .bool(true) && art["rights"]["permissions"]["export"] == .bool(true)
              }),
              let placement = frame.draft.placements.first(where: { $0["id"].string == placementId }),
              let artwork = frame.draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] }) else { throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY") }
        var fields: [String: DocumentValue] = ["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                         "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased()),
                         "expectedETag": .string(saved.etag), "placementId": .string(placementId),
                         "width": .integer(Int64(Self.width)), "height": .integer(Int64(Self.height))]
        switch camera {
        case .savedViewpoint(let id):
            guard UUID(uuidString: id) != nil,
                  let viewpoint = frame.draft.candidate["extensions"]["org.exhibitos.studio/presentation"]["viewpoints"].items?.first(where: { $0["id"].string == id }),
                  viewpoint["roomId"] == placement["roomId"] else { throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY") }
            self.viewpoint = viewpoint; fields["viewpointId"] = .string(id)
        case .inspection(let input):
            guard input.roomId == placement["roomId"].string,
                  let room = frame.draft.rooms.first(where: { $0["id"].string == input.roomId }), input.belongs(to: room) else { throw AuthoringFailure("SCENE_SNAPSHOT_TARGET_INVALID") }
            self.viewpoint = .null; fields["inspectionCamera"] = input.value
        }
        self.frame = frame; self.saved = saved; self.camera = camera; self.placement = placement; self.artwork = artwork
        value = .object(fields)
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
              closed(reply["target"], ["placementId", "artworkRevisionId", "assetId", "ready", "inFrustum", "centerRay"]) else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
        switch camera {
        case .savedViewpoint:
            guard closed(reply["camera"], ["viewpointId", "roomId", "position", "target", "fov"]),
                  reply["camera"]["viewpointId"] == value["viewpointId"], reply["camera"]["fov"] == viewpoint["fov"] else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
        case .inspection(let input):
            guard closed(reply["camera"], ["inspection", "roomId", "position", "target", "fov", "localPosition", "localTarget"]),
                  reply["camera"]["inspection"] == .bool(true), reply["camera"]["fov"].double == input.fov,
                  reply["camera"]["localPosition"] == input.value["position"], reply["camera"]["localTarget"] == input.value["target"],
                  point(reply["camera"]["position"]), point(reply["camera"]["target"]),
                  let p = reply["camera"]["position"].items, let t = reply["camera"]["target"].items,
                  zip(p, t).reduce(0, { $0 + pow($1.0.double! - $1.1.double!, 2) }) > 1e-12 else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
        }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "expectedETag", "width", "height"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
        }
        guard reply["saved"]["exhibitionId"].string == saved.draft.id,
              reply["saved"]["draftId"] == saved.draft.document["id"],
              reply["saved"]["revision"].integer == Int64(saved.revision), reply["saved"]["etag"].string == saved.etag,
              reply["camera"]["roomId"] == placement["roomId"],
              point(reply["camera"]["position"]), point(reply["camera"]["target"]),
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
