// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A current-frame failure sample, not a history, resource estimate or readiness claim.
/// The caller must retain the existing authenticated frame/ACK checks around sampling.
public enum WorkspaceFurnishingFailureReadout {
    public static let maximumBytes = 32_768
    public static let maximumEntries = 32
    private static let maximumInteger: Int64 = 9_007_199_254_740_991
    private static let stages: Set<String> = [
        "byte-reserve", "display-check", "descriptor-match", "model-fetch", "integrity", "preflight",
        "decode", "loaded-validation", "scene-reserve", "placement", "unknown"
    ]
    private static let codes: Set<String> = [
        "WORKSPACE_MODEL_AUTHORITY", "WORKSPACE_MODEL_FETCH", "WORKSPACE_MODEL_INTEGRITY",
        "WORKSPACE_MODEL_INVALID", "WORKSPACE_MODEL_BUDGET", "WORKSPACE_MODEL_UNSUPPORTED",
        "WORKSPACE_SCENE_BUDGET", "WORKSPACE_PLACEMENT_INVALID", "DECODE_BUDGET_EXCEEDED",
        "ASSET_BUDGET_EXCEEDED", "ASSET_TRANSITION_PEAK", "WORKSPACE_FURNISHING_UNSUPPORTED", "WORKSPACE_MODEL_INACTIVE",
        "ABORTED", "NETWORK_ERROR", "OTHER_FAILURE"
    ]
    private static let rootKeys: Set<String> = ["version", "settled", "loaded", "failed", "truncated", "failures"]
    private static let identityKeys = ["placementId", "furnishingId", "furnishingVersion", "modelSha256"]
    private static let counterKeys = ["remainingDraws", "remainingTriangles", "byteLimit", "bytesUsed", "bytesReserved"]
    private static let requestedKeys = ["requestedDraws", "requestedTriangles", "requestedBytes"]
    private static var rowKeys: Set<String> {
        Set(identityKeys + counterKeys + requestedKeys + ["stage", "code", "httpKind", "httpStatus"])
    }

    public static func projected(_ json: String?, in draft: NativeExhibitionDraft) -> DocumentValue {
        guard let json, json.utf8.count <= maximumBytes,
              let value = try? JSONDecoder().decode(DocumentValue.self, from: Data(json.utf8)),
              case .object(let fields) = value, Set(fields.keys) == rootKeys,
              fields["version"]?.integer == 1, fields["settled"] == .bool(true),
              let loaded = count(fields["loaded"]), let failed = count(fields["failed"]),
              let rows = fields["failures"]?.items, rows.count <= maximumEntries,
              let bindings = bindings(in: draft), loaded + failed == Int64(bindings.count),
              rows.count == min(Int(failed), maximumEntries), fields["truncated"] == .bool(failed > Int64(maximumEntries)) else { return .null }
        var seen = Set<String>()
        for row in rows {
            guard case .object(let fields) = row, Set(fields.keys) == rowKeys,
                  let id = fields["placementId"]?.string, seen.insert(id).inserted,
                  let binding = bindings[id], identityKeys.allSatisfy({ fields[$0] == binding[$0] }),
                  let version = count(fields["furnishingVersion"]), version > 0,
                  let stage = fields["stage"]?.string, stages.contains(stage),
                  let code = fields["code"]?.string, codes.contains(code),
                  counterKeys.allSatisfy({ count(fields[$0]) != nil }),
                  requestedKeys.allSatisfy({ fields[$0] == .null || count(fields[$0]) != nil }) else { return .null }
            let kind = fields["httpKind"]
            guard kind == .null || ["display-check", "model"].contains(kind?.string ?? "") else { return .null }
            if fields["httpStatus"] != .null {
                guard kind != .null, let status = fields["httpStatus"]?.integer, (100...599).contains(status) else { return .null }
            }
        }
        return value
    }

    private static func count(_ value: DocumentValue?) -> Int64? {
        guard let integer = value?.integer, (0...maximumInteger).contains(integer) else { return nil }
        return integer
    }

    /// Used only inside the existing display readout's one synchronous sample.
    /// Strip no fields silently: unknown shape or non-finite values are unavailable.
    public static var samplingScript: String {
        func array(_ values: Set<String>) -> String { "[" + values.sorted().map { "'\($0)'" }.joined(separator: ",") + "]" }
        return """
        result.workspaceFurnishingFailures = null;
        const failureRaw = c.dataset.workspaceFurnishingFailures;
        if (typeof failureRaw === 'string' && failureRaw.length <= \(maximumBytes)) {
          try {
            const value = JSON.parse(failureRaw);
            const exact = (v, keys) => v && typeof v === 'object' && !Array.isArray(v) &&
              Object.keys(v).length === keys.length && keys.every(k => Object.hasOwn(v, k));
            const count = v => Number.isSafeInteger(v) && v >= 0;
            const uuid = v => typeof v === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i.test(v);
            if (!exact(value, \(array(rootKeys))) || value.version !== 1 ||
                value.settled !== true || typeof value.truncated !== 'boolean' ||
                !count(value.loaded) || !count(value.failed) || !Array.isArray(value.failures) ||
                value.failures.length > \(maximumEntries)) throw new Error('UNAVAILABLE');
            for (const row of value.failures) {
              if (!exact(row, \(array(rowKeys))) || !uuid(row.placementId) || !uuid(row.furnishingId) ||
                  !count(row.furnishingVersion) || row.furnishingVersion < 1 ||
                  typeof row.modelSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(row.modelSha256) ||
                  !\(array(stages)).includes(row.stage) || !\(array(codes)).includes(row.code) ||
                  !\(array(Set(counterKeys))).every(k => count(row[k])) ||
                  !\(array(Set(requestedKeys))).every(k => row[k] === null || count(row[k])) ||
                  ![null, 'display-check', 'model'].includes(row.httpKind) ||
                  !(row.httpStatus === null || (row.httpKind !== null && Number.isInteger(row.httpStatus) && row.httpStatus >= 100 && row.httpStatus <= 599))) throw new Error('UNAVAILABLE');
            }
            const encoded = JSON.stringify(value);
            // Every admitted key/string is ASCII; encoded length equals UTF-8 bytes.
            if (encoded.length <= \(maximumBytes)) result.workspaceFurnishingFailures = encoded;
          } catch { /* Missing or malformed failure evidence remains unavailable. */ }
        }
        """
    }

    private static func bindings(in draft: NativeExhibitionDraft) -> [String: [String: DocumentValue]]? {
        let stored = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]
        if stored == .null { return [:] }
        guard let descriptors = stored["furnishings"].items, descriptors.count <= 64 else { return nil }
        var result: [String: [String: DocumentValue]] = [:]
        for (collection, identity) in [("placements", "id"), ("frameAttachments", "placementId"), ("openingAttachments", "openingId")] {
            let rows: [DocumentValue]
            if stored[collection] == .null, collection != "placements" { rows = [] }
            else { guard let items = stored[collection].items else { return nil }; rows = items }
            guard rows.count <= 128 else { return nil }
            for row in rows {
                guard let id = row[identity].string, UUID(uuidString: id) != nil, result[id] == nil,
                      let furnishing = row["furnishingId"].string, UUID(uuidString: furnishing) != nil,
                      let version = row["furnishingVersion"].integer, version > 0 else { return nil }
                let matches = descriptors.filter { $0["id"].string == furnishing && $0["version"].integer == version }
                guard matches.count == 1, let digest = matches[0]["model"]["sha256"].string,
                      nativeImportDigest(digest) else { return nil }
                result[id] = ["placementId": .string(id), "furnishingId": .string(furnishing),
                              "furnishingVersion": .integer(version), "modelSha256": .string(digest)]
            }
        }
        return result
    }
}
