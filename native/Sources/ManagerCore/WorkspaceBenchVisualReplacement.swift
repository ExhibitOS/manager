// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Replaces one explicitly selected backed bench's visual, preserving its seat
/// and authored viewpoint. The server independently checks the complete delta.
public struct WorkspaceBenchVisualReplacement: Sendable {
    public static let profile = "workspace-bench-visual-replacement-v1"
    public let benchId: String, placementId: String
    public let expectedFurnishingId: String, expectedModelSha256: String
    public let furnishingId: String, modelSha256: String
    public let expectedFurnishingVersion: Int, furnishingVersion: Int

    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == ["benchId", "placementId", "expectedFurnishingId", "expectedFurnishingVersion",
                                   "expectedModelSha256", "furnishingId", "furnishingVersion", "modelSha256"],
              let bench = value["benchId"].string, let placement = value["placementId"].string,
              let old = value["expectedFurnishingId"].string, let target = value["furnishingId"].string,
              [bench, placement, old, target].allSatisfy({ BenchSettings.uuid($0) }),
              Set([bench, placement, old, target]).count == 4,
              value["expectedFurnishingVersion"] == .integer(1), value["furnishingVersion"] == .integer(1),
              let oldHash = value["expectedModelSha256"].string, nativeImportDigest(oldHash),
              let newHash = value["modelSha256"].string, nativeImportDigest(newHash) else { throw Self.invalid() }
        benchId = bench; placementId = placement; expectedFurnishingId = old; furnishingId = target
        expectedFurnishingVersion = 1; furnishingVersion = 1
        expectedModelSha256 = oldHash; modelSha256 = newHash
    }

    static func invalid() -> AuthoringFailure { AuthoringFailure("WORKSPACE_BENCH_VISUAL_REPLACEMENT_INVALID") }

    func applying(_ furnishing: WorkspaceFurnishing, to draft: NativeExhibitionDraft,
                  supportedExtensionVersion: Int64) throws -> NativeExhibitionDraft {
        let benches = try BenchSettings.list(in: draft)
        guard benches.filter({ $0.id == benchId }).count == 1,
              let bench = benches.first(where: { $0.id == benchId }), bench.kind == .backed,
              bench.workspaceVisual?.placementId == placementId,
              bench.workspaceVisual?.modelSha256 == expectedModelSha256,
              benches.filter({ $0.workspaceVisual?.placementId == placementId }).count == 1,
              !benches.contains(where: { $0.id == furnishingId || $0.viewpointId == furnishingId }),
              furnishing.id == furnishingId, furnishing.version == 1, furnishing.details.kind == .bench,
              furnishing.details.model.sha256 == modelSha256 else { throw Self.invalid() }
        let namespace = WorkspaceFurnishingPlacement.namespace
        var candidate = draft.candidate
        var ext = candidate["extensions"][namespace]
        guard case .object(let fields) = ext, let version = ext["version"].integer,
              (1...7).contains(version), version <= supportedExtensionVersion,
              let assets = ext["furnishings"].items, (1...64).contains(assets.count),
              var rows = ext["placements"].items, (1...128).contains(rows.count) else { throw Self.invalid() }
        var expected: Set<String> = ["version", "furnishings", "placements"]
        if version >= 4 { expected.insert("frameAttachments") }
        if version >= 6 { expected.insert("openingAttachments") }
        guard Set(fields.keys) == expected,
              version < 4 || ext["frameAttachments"].items != nil,
              version < 6 || ext["openingAttachments"].items != nil,
              version != 6 || ext["openingAttachments"].items?.isEmpty == false,
              rows.filter({ $0["id"].string == placementId }).count == 1,
              let index = rows.firstIndex(where: { $0["id"].string == placementId }),
              rows[index]["furnishingId"] == .string(expectedFurnishingId),
              rows[index]["furnishingVersion"] == .integer(1), rows[index]["scale"].double == 1,
              rows[index]["roomId"] == .string(bench.roomId),
              rows[index]["position"] == .vector(bench.position), rows[index]["yaw"].double == bench.yaw,
              assets.filter({ $0["id"].string == expectedFurnishingId }).count == 1,
              let old = assets.first(where: { $0["id"].string == expectedFurnishingId }),
              old["kind"] == .string("bench"), old["version"] == .integer(1),
              old["model"]["sha256"] == .string(expectedModelSha256),
              !rows.contains(where: { $0["id"].string == furnishingId }) else { throw Self.invalid() }
        for name in ["rooms", "surfaces", "openings", "placements", "lights", "mediaAssets", "artworks"] {
            for row in candidate[name].items ?? [] {
                let ids = [row["id"], row["revisionId"]] + (row["assets"].items ?? []).map { $0["id"] }
                guard !ids.contains(where: { $0.string == furnishingId }) else { throw Self.invalid() }
            }
        }
        let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(furnishing))
        let matches = assets.filter { $0["id"].string == furnishingId }
        guard matches.isEmpty || matches == [descriptor] else { throw Self.invalid() }
        rows[index]["furnishingId"] = .string(furnishingId)
        ext["placements"] = .array(rows)
        let references = rows + (ext["frameAttachments"].items ?? []) + (ext["openingAttachments"].items ?? [])
        var nextAssets = references.contains(where: { $0["furnishingId"].string == expectedFurnishingId })
            ? assets : assets.filter { $0["id"].string != expectedFurnishingId }
        if matches.isEmpty { nextAssets.append(descriptor) }
        guard nextAssets.count <= 64 else { throw Self.invalid() }
        ext["furnishings"] = .array(nextAssets)
        candidate["extensions"][namespace] = ext
        guard var benchRows = candidate["extensions"][BenchSettings.namespace]["benches"].items,
              let benchIndex = benchRows.firstIndex(where: { $0["id"].string == benchId }) else { throw Self.invalid() }
        benchRows[benchIndex]["workspaceVisual"]["modelSha256"] = .string(modelSha256)
        candidate["extensions"][BenchSettings.namespace]["benches"] = .array(benchRows)
        var next = draft
        try next.replaceCandidate(candidate)
        _ = try BenchSettings.list(in: next)
        return next
    }
}
