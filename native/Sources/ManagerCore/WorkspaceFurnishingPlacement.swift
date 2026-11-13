// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct WorkspaceFurnishingPlacement: Codable, Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/workspace-furnishings"
    public let id: String
    public let roomId: String
    public let furnishingId: String
    public let furnishingVersion: Int
    public let position: [Double]
    public let yaw: Double
    public let scale: Double

    public init(id: String, roomId: String, furnishingId: String, furnishingVersion: Int,
                position: [Double], yaw: Double, scale: Double) {
        self.id = id
        self.roomId = roomId
        self.furnishingId = furnishingId
        self.furnishingVersion = furnishingVersion
        self.position = position
        self.yaw = yaw
        self.scale = scale
    }

    func applying(_ furnishing: WorkspaceFurnishing, to draft: NativeExhibitionDraft,
                  supportedExtensionVersion: Int64 = 1) throws -> NativeExhibitionDraft? {
        var version = furnishing.details.kind.minimumExtensionVersion
        guard furnishing.details.kind != .artworkFrame, furnishing.details.kind != .portalFinish, version <= supportedExtensionVersion else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        let matches = draft.rooms.filter { $0["id"].string == roomId }
        guard [id, roomId, furnishingId].allSatisfy(workspaceFurnishingID), id != furnishingId,
              furnishingId == furnishing.id, furnishingVersion == furnishing.version,
              matches.count == 1, position.count == 3, position.allSatisfy({ (-10_000...10_000).contains($0) }),
              (-Double.pi...Double.pi).contains(yaw), (0.1...4).contains(scale) else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
        }
        let room = matches[0]["dimensions"]
        guard let width = room["width"].double, let height = room["height"].double, let depth = room["depth"].double,
              [width, height, depth].allSatisfy({ $0.isFinite && $0 > 0 }),
              abs(position[0]) <= width / 2, (0...height).contains(position[1]), abs(position[2]) <= depth / 2 else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
        }

        var candidate = draft.candidate
        let collections = ["rooms", "surfaces", "openings", "placements", "lights", "mediaAssets", "artworks"]
        for name in collections {
            for item in candidate[name].items ?? [] {
                let keys = [item["id"], item["revisionId"]] + (item["assets"].items ?? []).map { $0["id"] }
                guard !keys.contains(where: { $0.string?.lowercased() == id || $0.string?.lowercased() == furnishingId }) else {
                    throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
                }
            }
        }

        let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(furnishing))
        let placement = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(self))
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        guard case .object(let extensions) = candidate["extensions"] else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
        }
        var furnishings: [DocumentValue] = []
        var placements: [DocumentValue] = []
        var frameAttachments: [DocumentValue]?
        var openingAttachments: [DocumentValue]?
        if let existing = extensions[Self.namespace] {
            guard case .object(let fields) = existing,
                  let existingVersion = existing["version"].integer, ((1...7).contains(existingVersion) || existingVersion == 9),
                  let assets = existing["furnishings"].items,
                  let rows = existing["placements"].items, !assets.isEmpty,
                  assets.count <= 64, rows.count <= 128 else {
                throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
            }
            var expected: Set<String> = ["version", "furnishings", "placements"]
            if existingVersion >= 4 { expected.insert("frameAttachments") }
            if existingVersion >= 6 { expected.insert("openingAttachments") }
            openingAttachments = existing["openingAttachments"].items
            guard Set(fields.keys) == expected, existingVersion < 6 || openingAttachments != nil,
                  existingVersion != 6 || openingAttachments?.isEmpty == false else {
                throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
            }
            frameAttachments = existing["frameAttachments"].items
            guard existingVersion >= 4 ? frameAttachments != nil : !rows.isEmpty else {
                throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
            }
            guard existingVersion <= supportedExtensionVersion else {
                throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            guard assets.allSatisfy({ row in
                guard let value = row["kind"].string, let kind = WorkspaceFurnishingKind(rawValue: value) else { return false }
                return kind.minimumExtensionVersion <= existingVersion
            }) else { throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID") }
            furnishings = assets
            placements = rows
            version = max(version, existingVersion)
        }
        let matching = furnishings.filter { $0["id"].string == furnishingId }
        guard matching.isEmpty || matching == [descriptor] else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        let replay = placements.filter { $0["id"].string == id }
        if replay == [placement], matching == [descriptor] { return nil }
        guard replay.isEmpty, !furnishings.contains(where: { $0["id"].string == id }),
              !placements.contains(where: { $0["id"].string == furnishingId }), placements.count < 128,
              !matching.isEmpty || furnishings.count < 64 else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID")
        }
        if matching.isEmpty { furnishings.append(descriptor) }
        placements.append(placement)
        var fields: [String: DocumentValue] = [
            "version": .integer(version), "furnishings": .array(furnishings), "placements": .array(placements)
        ]
        if let frameAttachments { fields["frameAttachments"] = .array(frameAttachments) }
        if let openingAttachments { fields["openingAttachments"] = .array(openingAttachments) }
        candidate["extensions"][Self.namespace] = .object(fields)
        var result = draft
        try result.replaceCandidate(candidate)
        return result
    }
}
