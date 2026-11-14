// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Replaces only an existing backed seat's linked registered visual. The
/// functional seat, viewpoint, pose, axes and collision settings remain exact.
public struct WorkspaceBenchVisualReplacement: Sendable {
    public let benchId: String, placementId: String, expectedModelSHA256: String
    public let furnishingId: String, modelSHA256: String
    public init(benchId: String, placementId: String, expectedModelSHA256: String,
                furnishingId: String, modelSHA256: String) throws {
        guard [benchId, placementId, furnishingId].allSatisfy(workspaceFurnishingID),
              Set([benchId, placementId, furnishingId]).count == 3,
              nativeImportDigest(expectedModelSHA256), nativeImportDigest(modelSHA256),
              expectedModelSHA256 != modelSHA256 else { throw Self.invalid() }
        self.benchId = benchId; self.placementId = placementId
        self.expectedModelSHA256 = expectedModelSHA256
        self.furnishingId = furnishingId; self.modelSHA256 = modelSHA256
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("WORKSPACE_BENCH_VISUAL_REPLACEMENT_INVALID") }

    func applying(_ furnishing: WorkspaceFurnishing, to draft: NativeExhibitionDraft,
                  supportedExtensionVersion: Int64) throws -> NativeExhibitionDraft {
        let benches = try BenchSettings.list(in: draft)
        guard benches.filter({ $0.id == benchId }).count == 1,
              let selected = benches.first(where: { $0.id == benchId }), selected.kind == .backed,
              let visual = selected.workspaceVisual, visual.placementId == placementId,
              visual.modelSha256 == expectedModelSHA256,
              benches.filter({ $0.workspaceVisual?.placementId == placementId }).count == 1,
              furnishing.id == furnishingId, furnishing.version == 1,
              furnishing.details.kind == .bench, furnishing.details.model.sha256 == modelSHA256 else { throw Self.invalid() }
        var candidate = draft.candidate
        let namespace = WorkspaceFurnishingPlacement.namespace
        var ext = candidate["extensions"][namespace]
        guard let version = ext["version"].integer,
              [Int64(1),2,3,4,5,6,7,9,10,11].contains(version), version <= supportedExtensionVersion,
              var assets = ext["furnishings"].items, (1...64).contains(assets.count),
              var placements = ext["placements"].items, (1...128).contains(placements.count),
              placements.filter({ $0["id"].string == placementId }).count == 1,
              let index = placements.firstIndex(where: { $0["id"].string == placementId }) else { throw Self.invalid() }
        let oldPlacement = try JSONDecoder().decode(WorkspaceFurnishingPlacement.self,
            from: JSONEncoder().encode(placements[index]))
        guard oldPlacement.roomId == selected.roomId, oldPlacement.position == selected.position,
              oldPlacement.yaw == selected.yaw, oldPlacement.scale == 1,
              assets.filter({ $0["id"].string == oldPlacement.furnishingId }).count == 1,
              let old = assets.first(where: { $0["id"].string == oldPlacement.furnishingId }),
              old["version"].integer == Int64(oldPlacement.furnishingVersion), old["kind"] == .string("bench"),
              old["model"]["sha256"] == .string(expectedModelSHA256),
              !placements.contains(where: { $0["id"].string == furnishingId }),
              !assets.contains(where: { $0["id"].string == placementId }) else { throw Self.invalid() }
        for collection in ["rooms", "surfaces", "openings", "placements", "lights", "mediaAssets", "artworks"] {
            for row in candidate[collection].items ?? [] {
                let ids = [row["id"], row["revisionId"]] + (row["assets"].items ?? []).map { $0["id"] }
                guard !ids.contains(where: { $0.string == furnishingId }) else { throw Self.invalid() }
            }
        }
        let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(furnishing))
        let matches = assets.filter { $0["id"].string == furnishingId }
        guard matches.isEmpty || matches == [descriptor] else { throw Self.invalid() }
        placements[index]["furnishingId"] = .string(furnishingId)
        placements[index]["furnishingVersion"] = .integer(1)
        ext["placements"] = .array(placements)
        let references = placements + (ext["frameAttachments"].items ?? [])
            + (ext["openingAttachments"].items ?? []) + (ext["ceilingAttachments"].items ?? [])
        if !references.contains(where: { $0["furnishingId"].string == oldPlacement.furnishingId }) {
            assets.removeAll { $0["id"].string == oldPlacement.furnishingId }
        }
        if matches.isEmpty { assets.append(descriptor) }
        guard assets.count <= 64 else { throw Self.invalid() }
        ext["furnishings"] = .array(assets)
        candidate["extensions"][namespace] = ext
        var benchExt = candidate["extensions"][BenchSettings.namespace]
        guard var rows = benchExt["benches"].items,
              let benchIndex = rows.firstIndex(where: { $0["id"].string == benchId }) else { throw Self.invalid() }
        rows[benchIndex]["workspaceVisual"]["modelSha256"] = .string(modelSHA256)
        benchExt["benches"] = .array(rows); candidate["extensions"][BenchSettings.namespace] = benchExt
        if version == 11 { try WorkspaceCeilingRail.validate(candidate: candidate) }
        var next = draft; try next.replaceCandidate(candidate)
        _ = try BenchSettings.list(in: next)
        return next
    }
}
