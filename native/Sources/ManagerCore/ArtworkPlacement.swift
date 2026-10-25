// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum ArtworkPlacementTargetKind: String, Equatable, Sendable { case wall, floor }

/// One transient operation over the current draft, never a second draft/store.
/// The authenticated Studio snapshot is pinned before the renderer sees intent.
public struct ArtworkPlacementIntent: Equatable, Sendable {
    public let requestId: String
    public let placementId: String
    public let artworkId: String
    public let expectedRevision: Int
    public let roomId: String
    public let targetKind: ArtworkPlacementTargetKind
    public let artwork: DocumentValue
    public let base: NativeExhibitionDraft
    public init(artworkId: String, expectedRevision: Int, tenantId: String, artwork: DocumentValue, roomId: String, base: NativeExhibitionDraft) throws {
        let cms = artwork["extensions"]["org.exhibitos.studio/cms"]
        guard UUID(uuidString: artworkId) != nil, UUID(uuidString: tenantId) != nil,
              (1...2_147_483_647).contains(expectedRevision), artwork["id"].string == artworkId,
              cms["tenantId"].string == tenantId, cms["artworkId"].string == artworkId,
              cms["revision"].integer == Int64(expectedRevision),
              artwork["rights"]["permissions"]["display"] == .bool(true),
              base.rooms.contains(where: { $0["id"].string == roomId }),
              ["image", "painting", "sculpture"].contains(artwork["artworkType"].string ?? "") else { throw AuthoringFailure("ARTWORK_SCOPE_CHANGED") }
        requestId = UUID().uuidString.lowercased(); placementId = UUID().uuidString.lowercased()
        self.artworkId = artworkId; self.expectedRevision = expectedRevision; self.roomId = roomId
        self.artwork = artwork; self.base = base
        targetKind = artwork["artworkType"].string == "sculpture" ? .floor : .wall
    }
    /// Shared geometry owns hit solving, transformed artwork bounds and openings.
    /// Native admits only a bounded pose for this exact source/room/surface, then
    /// applies the existing approved-add command. No browser document is accepted.
    public func applying(_ placement: DocumentValue, surfaceId: String, offset: [Double], to current: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
        guard current == base, UUID(uuidString: surfaceId) != nil,
              offset.count == 2, offset.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }),
              let surface = base.candidate["surfaces"].items?.first(where: { $0["id"].string == surfaceId }),
              surface["roomId"].string == roomId, surface["type"].string == targetKind.rawValue,
              let sw = surface["dimensions"]["width"].double, let sh = surface["dimensions"]["height"].double,
              sw.isFinite, sh.isFinite, sw > 0, sh > 0, abs(offset[0]) <= sw / 2 + 1e-9, abs(offset[1]) <= sh / 2 + 1e-9,
              case .object(let row) = placement, Set(row.keys) == Set(["id", "roomId", "artworkRevisionId", "assetId", "transform"]),
              placement["id"].string == placementId, placement["roomId"].string == roomId,
              placement["artworkRevisionId"] == artwork["revisionId"], placement["assetId"] == artwork["primaryAssetId"],
              let room = base.rooms.first(where: { $0["id"].string == roomId }),
              let p = placement["transform"]["position"].items, p.count == 3, p.allSatisfy({ $0.double?.isFinite == true }),
              let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double, let depth = room["dimensions"]["depth"].double,
              abs(p[0].double!) <= width / 2 + 1e-9, p[1].double! >= -1e-9, p[1].double! <= height + 1e-9, abs(p[2].double!) <= depth / 2 + 1e-9 else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
        let expected = targetKind == .wall ? surface["transform"]["rotation"] : NativeExhibitionDraft.identity["rotation"]
        guard let rotation = placement["transform"]["rotation"].items, let expected = expected.items, rotation.count == 4, expected.count == 4,
              zip(rotation, expected).allSatisfy({ a, b in a.double?.isFinite == true && b.double?.isFinite == true }),
              [1.0, -1.0].contains(where: { sign in zip(rotation, expected).allSatisfy { abs($0.double! - sign * $1.double!) <= 1e-6 } }) else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
        var next = current
        try next.addApprovedArtwork(artwork, roomId: roomId, placementId: placementId, transform: placement["transform"])
        guard try JSONEncoder().encode(next.candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return next
    }
}
