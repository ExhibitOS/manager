// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Explicitly selected existing stand/plaque pair. No relationship is inferred
/// from proximity; the artwork source and all caption content remain unchanged.
public struct WorkspaceInfoStandReplacement: Equatable, Sendable {
    public static let profile = "workspace-info-stand-replacement-v1"
    public let placement: WorkspaceFurnishingPlacement
    public let plaqueId: String
    public let plaquePosition: [Double], plaqueRotation: [Double]
    public let plaqueWidth: Double, plaqueHeight: Double

    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == ["placement", "plaque"],
              case .object(let pose) = value["placement"],
              Set(pose.keys) == ["id", "roomId", "furnishingId", "furnishingVersion", "position", "yaw", "scale"],
              case .object(let plaque) = value["plaque"], Set(plaque.keys) == ["id", "position", "rotation", "dimensions"],
              let id = value["plaque"]["id"].string,
              let position = BenchSettings.numbers(value["plaque"]["position"], count: 3),
              let rotation = BenchSettings.numbers(value["plaque"]["rotation"], count: 4),
              case .object(let size) = value["plaque"]["dimensions"], Set(size.keys) == ["width", "height"],
              let width = size["width"]?.double, let height = size["height"]?.double else { throw Self.invalid() }
        let placement = try JSONDecoder().decode(WorkspaceFurnishingPlacement.self, from: JSONEncoder().encode(value["placement"]))
        guard try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(placement)) == value["placement"] else { throw Self.invalid() }
        self.placement = placement; plaqueId = id; plaquePosition = position; plaqueRotation = rotation
        plaqueWidth = width; plaqueHeight = height
    }

    static func invalid() -> AuthoringFailure { AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_INVALID") }

    func applying(_ furnishing: WorkspaceFurnishing, to draft: NativeExhibitionDraft,
                  supportedExtensionVersion: Int64) throws -> NativeExhibitionDraft? {
        func id(_ value: String) -> Bool { workspaceFurnishingID(value) && UUID(uuidString: value) != nil }
        let p = placement
        guard [p.id, p.roomId, p.furnishingId, plaqueId].allSatisfy(id),
              p.id != p.furnishingId, p.id != plaqueId, p.furnishingId != plaqueId,
              furnishing.details.kind == .infoStand, furnishing.id == p.furnishingId,
              furnishing.version == 1, p.furnishingVersion == 1,
              p.position.count == 3, p.position.allSatisfy({ $0.isFinite && (-10_000...10_000).contains($0) }),
              p.yaw.isFinite, (-Double.pi...Double.pi).contains(p.yaw), p.scale.isFinite, (0.1...4).contains(p.scale),
              let room = draft.rooms.first(where: { $0["id"].string == p.roomId }),
              draft.rooms.filter({ $0["id"].string == p.roomId }).count == 1,
              let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double,
              let depth = room["dimensions"]["depth"].double,
              [width, height, depth].allSatisfy({ $0.isFinite && $0 > 0 }),
              abs(p.position[0]) <= width / 2, (0...height).contains(p.position[1]), abs(p.position[2]) <= depth / 2 else { throw Self.invalid() }
        let namespace = WorkspaceFurnishingPlacement.namespace
        var candidate = draft.candidate
        var ext = candidate["extensions"][namespace]
        guard case .object(let fields) = ext, let version = ext["version"].integer,
              (3...7).contains(version), version <= supportedExtensionVersion,
              let assets = ext["furnishings"].items, (1...64).contains(assets.count),
              var rows = ext["placements"].items, (1...128).contains(rows.count) else { throw Self.invalid() }
        var expected: Set<String> = ["version", "furnishings", "placements"]
        if version >= 4 { expected.insert("frameAttachments") }
        if version >= 6 { expected.insert("openingAttachments") }
        guard Set(fields.keys) == expected,
              version < 4 || ext["frameAttachments"].items != nil,
              version < 6 || ext["openingAttachments"].items != nil,
              version != 6 || ext["openingAttachments"].items?.isEmpty == false,
              rows.filter({ $0["id"].string == p.id }).count == 1,
              let index = rows.firstIndex(where: { $0["id"].string == p.id }),
              rows[index]["roomId"].string == p.roomId,
              let oldId = rows[index]["furnishingId"].string,
              let oldVersion = rows[index]["furnishingVersion"].integer,
              assets.filter({ $0["id"].string == oldId && $0["version"].integer == oldVersion }).count == 1,
              assets.first(where: { $0["id"].string == oldId })?["kind"].string == "info-stand" else { throw Self.invalid() }
        let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(furnishing))
        let matches = assets.filter { $0["id"].string == p.furnishingId }
        guard matches.isEmpty || matches == [descriptor],
              !rows.contains(where: { $0["id"].string == p.furnishingId }),
              !assets.contains(where: { $0["id"].string == p.id }) else { throw Self.invalid() }
        for collection in ["rooms", "surfaces", "openings", "placements", "lights", "mediaAssets", "artworks"] {
            for row in candidate[collection].items ?? [] {
                let ids = [row["id"], row["revisionId"]] + (row["assets"].items ?? []).map { $0["id"] }
                guard !ids.contains(where: { $0.string == p.furnishingId }) else { throw Self.invalid() }
            }
        }
        let plaques = try PlaqueSettings.list(in: draft)
        guard let selected = plaques.first(where: { $0.id == plaqueId }), selected.roomId == p.roomId,
              case .artwork = selected.source else { throw Self.invalid() }
        var plaqueExt = candidate["extensions"][PlaqueSettings.namespace]
        guard var plaqueRows = plaqueExt["plaques"].items,
              plaqueRows.filter({ $0["id"].string == plaqueId }).count == 1,
              let plaqueIndex = plaqueRows.firstIndex(where: { $0["id"].string == plaqueId }) else { throw Self.invalid() }
        var plaque = plaqueRows[plaqueIndex]
        plaque["position"] = .vector(plaquePosition); plaque["rotation"] = .vector(plaqueRotation)
        plaque["dimensions"] = .object(["width": .number(plaqueWidth), "height": .number(plaqueHeight)])
        _ = try PlaqueSettings(value: plaque)
        plaqueRows[plaqueIndex] = plaque; plaqueExt["plaques"] = .array(plaqueRows)
        rows[index] = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(p))
        ext["placements"] = .array(rows)
        // Registered versions remain immutable in the workspace. The scene
        // snapshot, however, may contain only descriptors still in use.
        let references = rows + (ext["frameAttachments"].items ?? []) + (ext["openingAttachments"].items ?? [])
        let oldStillUsed = references.contains { $0["furnishingId"].string == oldId }
        var nextAssets = oldStillUsed ? assets : assets.filter { $0["id"].string != oldId }
        if matches.isEmpty { nextAssets.append(descriptor) }
        guard nextAssets.count <= 64 else { throw Self.invalid() }
        ext["furnishings"] = .array(nextAssets)
        candidate["extensions"][namespace] = ext; candidate["extensions"][PlaqueSettings.namespace] = plaqueExt
        if candidate == draft.candidate { return nil }
        var next = draft
        try next.replaceCandidate(candidate)
        _ = try PlaqueSettings.list(in: next)
        return next
    }
}
