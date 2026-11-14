// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Optional renderer evidence, bound to every furnishing host in the saved graph.
/// A valid fallback/pending/failed row is evidence, never readiness admission.
enum SceneSnapshotFurnishings {
    static let maximumBytes = SceneSnapshotViewportRequest.maximumFurnishingEvidenceBytes
    private static var invalid: AuthoringFailure { AuthoringFailure("SCENE_SNAPSHOT_INVALID") }

    static func checked(_ value: DocumentValue, draft: NativeExhibitionDraft) throws -> DocumentValue {
        do {
            guard case .object(let fields) = value, Set(fields.keys) == ["catalog", "workspace"],
                  try JSONEncoder().encode(value).count <= maximumBytes else { throw invalid }
            let extensions = draft.candidate["extensions"]
            var catalog: [String: [String: DocumentValue]] = [:]
            let shared = extensions["org.exhibitos.studio/furnishing-visuals"]
            if shared != .null {
                guard let assets = shared["assets"].items, assets.count <= 8,
                      let bindings = shared["bindings"].items, bindings.count <= 32 else { throw invalid }
                for binding in bindings {
                    guard let kind = binding["hostKind"].string, ["pedestal", "bench", "light", "opening"].contains(kind),
                          let id = binding["hostId"].string, UUID(uuidString: id) != nil,
                          let assetId = binding["assetId"].string, UUID(uuidString: assetId) != nil else { throw invalid }
                    let matching = assets.filter { $0["id"].string?.lowercased() == assetId.lowercased() }
                    guard matching.count == 1, let declaredId = matching[0]["id"].string, UUID(uuidString: declaredId) != nil,
                          let digest = matching[0]["sha256"].string, nativeImportDigest(digest) else { throw invalid }
                    let key = kind + ":" + id.lowercased()
                    guard catalog[key] == nil else { throw invalid }
                    catalog[key] = ["kind": .string(kind), "id": .string(id), "assetId": .string(declaredId), "modelSha256": .string(digest)]
                }
            }
            var workspace: [String: [String: DocumentValue]] = [:]
            let stored = extensions[WorkspaceFurnishingPlacement.namespace]
            if stored != .null {
                guard let descriptors = stored["furnishings"].items, descriptors.count <= 64 else { throw invalid }
                for (collection, identity) in [("placements", "id"), ("frameAttachments", "placementId"), ("openingAttachments", "openingId")] {
                    let rows: [DocumentValue]
                    if stored[collection] == .null, collection != "placements" { rows = [] }
                    else { guard let items = stored[collection].items else { throw invalid }; rows = items }
                    guard rows.count <= 128 else { throw invalid }
                    for row in rows {
                        guard let id = row[identity].string, UUID(uuidString: id) != nil,
                              let furnishingId = row["furnishingId"].string, UUID(uuidString: furnishingId) != nil,
                              let version = row["furnishingVersion"].integer, version > 0 else { throw invalid }
                        let matching = descriptors.filter { $0["id"].string == furnishingId && $0["version"].integer == version }
                        guard matching.count == 1, let digest = matching[0]["model"]["sha256"].string,
                              nativeImportDigest(digest), workspace[id] == nil else { throw invalid }
                        workspace[id] = ["placementId": .string(id), "furnishingId": .string(furnishingId),
                            "furnishingVersion": .integer(version), "modelSha256": .string(digest)]
                    }
                }
            }
            try rows(value["catalog"], expected: catalog, statuses: ["ready", "fallback"], catalog: true)
            try rows(value["workspace"], expected: workspace, statuses: ["ready", "pending", "failed"], catalog: false)
            return value
        } catch { throw invalid }
    }

    private static func rows(_ value: DocumentValue, expected: [String: [String: DocumentValue]], statuses: Set<String>, catalog: Bool) throws {
        guard let items = value.items, items.count == expected.count else { throw invalid }
        var seen = Set<String>()
        for row in items {
            guard case .object(var fields) = row, let status = fields.removeValue(forKey: "status")?.string,
                  statuses.contains(status) else { throw invalid }
            let key: String
            if catalog {
                guard let kind = fields["kind"]?.string, let id = fields["id"]?.string else { throw invalid }
                key = kind + ":" + id.lowercased()
            } else {
                guard let id = fields["placementId"]?.string else { throw invalid }; key = id
            }
            guard seen.insert(key).inserted, let original = expected[key], fields == original else { throw invalid }
        }
    }
}
