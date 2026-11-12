// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Declares the model-local facing of one already registered backed bench.
public struct WorkspaceBenchModelForward: Sendable {
    public static let profile = "workspace-bench-model-forward-v1"
    public let benchId: String, placementId: String, furnishingId: String, modelSha256: String
    public let expectedModelForward: BenchSettings.WorkspaceVisual.ModelForward
    public let modelForward: BenchSettings.WorkspaceVisual.ModelForward

    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == ["benchId", "placementId", "furnishingId", "furnishingVersion", "modelSha256", "expectedModelForward", "modelForward"],
              let benchId = value["benchId"].string, let placementId = value["placementId"].string,
              let furnishingId = value["furnishingId"].string,
              [benchId, placementId, furnishingId].allSatisfy({ BenchSettings.uuid($0) }),
              value["furnishingVersion"] == .integer(1), let hash = value["modelSha256"].string, nativeImportDigest(hash),
              let old = value["expectedModelForward"].string.flatMap(BenchSettings.WorkspaceVisual.ModelForward.init(rawValue:)),
              let next = value["modelForward"].string.flatMap(BenchSettings.WorkspaceVisual.ModelForward.init(rawValue:)) else {
            throw AuthoringFailure("BENCH_MODEL_FORWARD_INVALID")
        }
        self.benchId = benchId; self.placementId = placementId; self.furnishingId = furnishingId; modelSha256 = hash
        expectedModelForward = old; modelForward = next
    }

    func applying(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft? {
        func same(_ left: String?, _ right: String) -> Bool { left?.lowercased() == right.lowercased() }
        let benches = try BenchSettings.list(in: draft)
        let matching = benches.filter { same($0.id, benchId) }
        guard matching.count == 1, let bench = matching.first, bench.kind == .backed,
              let visual = bench.workspaceVisual, same(visual.placementId, placementId), visual.modelSha256 == modelSha256,
              benches.filter({ same($0.workspaceVisual?.placementId, placementId) }).count == 1 else {
            throw AuthoringFailure("BENCH_MODEL_FORWARD_REFERENCE")
        }
        let workspace = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]
        guard let placements = workspace["placements"].items, let assets = workspace["furnishings"].items,
              placements.filter({ same($0["id"].string, placementId) }).count == 1,
              let placement = placements.first(where: { same($0["id"].string, placementId) }),
              same(placement["furnishingId"].string, furnishingId), placement["furnishingVersion"] == .integer(1),
              placement["scale"].double == 1,
              assets.filter({ same($0["id"].string, furnishingId) }).count == 1,
              let asset = assets.first(where: { same($0["id"].string, furnishingId) }),
              asset["version"] == .integer(1), asset["kind"] == .string("bench"), asset["model"]["sha256"] == .string(modelSha256) else {
            throw AuthoringFailure("BENCH_MODEL_FORWARD_REFERENCE")
        }
        guard (visual.modelForward ?? .negativeZ) == expectedModelForward else { throw AuthoringFailure("BENCH_MODEL_FORWARD_STALE") }
        guard expectedModelForward != modelForward else { return nil }
        var candidate = draft.candidate
        guard var rows = candidate["extensions"][BenchSettings.namespace]["benches"].items,
              let index = rows.firstIndex(where: { same($0["id"].string, benchId) }) else { throw AuthoringFailure("BENCH_MODEL_FORWARD_REFERENCE") }
        rows[index]["workspaceVisual"]["modelForward"] = .string(modelForward.rawValue)
        candidate["extensions"][BenchSettings.namespace]["benches"] = .array(rows)
        var next = draft; try next.replaceCandidate(candidate); _ = try BenchSettings.list(in: next)
        return next
    }
}
