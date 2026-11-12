// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Read only from the owned helper's verified payload inventory.
public struct NativeGPUPreviewResources: Decodable, Sendable {
    public let format: Int
    public let payloadManifestSha256: String
    public let paths: [String]
    public init(from decoder: Decoder) throws {
        let raw = try decoder.container(keyedBy: NativeGPUModelCodingKey.self)
        guard Set(raw.allKeys.map(\.stringValue)) == ["format", "payloadManifestSha256", "paths"] else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        enum Keys: String, CodingKey { case format, payloadManifestSha256, paths }
        let c = try decoder.container(keyedBy: Keys.self)
        format = try c.decode(Int.self, forKey: .format)
        payloadManifestSha256 = try c.decode(String.self, forKey: .payloadManifestSha256)
        paths = try c.decode([String].self, forKey: .paths)
    }
    public func checked(expectedManifestSHA256: String) throws -> Set<String> {
        let fixed: Set<String> = ["native-viewport.html", "basis/basis_transcoder.js", "basis/basis_transcoder.wasm", "basis/exhibitos-basis-worker.js"]
        let all = Set(paths)
        guard format == 1, NativeGPUModelPreparation.digest(expectedManifestSHA256), payloadManifestSha256 == expectedManifestSHA256,
              all.count == paths.count, (5...260).contains(paths.count), fixed.isSubset(of: all) else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        let assets = all.subtracting(fixed)
        for path in assets {
            guard path.hasPrefix("assets/"), !path.contains("/../"), !path.contains("//"),
                  path.utf8.count <= 200,
                  path.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || [45, 46, 47, 95].contains($0) }),
                  !path.dropFirst("assets/".count).contains("/"),
                  path.hasSuffix(".js") || path.hasSuffix(".css") else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        }
        return assets
    }
}

/// The host must obtain `verifiedAssets` from the selected runtime's verified
/// asset inventory. This function never admits other API routes or origins.
public enum NativeGPUCandidatePreviewPolicy {
    public static func encodedRules(origin: String, request: NativeGPUCandidatePreviewRequest,
                                    verifiedAssets: Set<String>) throws -> String {
        guard let url = URL(string: origin), let port = url.port, (1024...65535).contains(port),
              origin == "http://127.0.0.1:\(port)", !verifiedAssets.isEmpty,
              verifiedAssets.count <= 256 else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        for path in verifiedAssets {
            guard path.hasPrefix("assets/"), !path.contains("/../"), !path.contains("//"),
                  path.utf8.count <= 200,
                  path.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || [45, 46, 47, 95].contains($0) }),
                  !path.dropFirst("assets/".count).contains("/"),
                  path.hasSuffix(".js") || path.hasSuffix(".css") else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        }
        let v = request.value
        guard let tenant = v["tenantId"].string, let artwork = v["artworkId"].string, let job = v["jobId"].string,
              let receipt = v["receiptSHA256"].string, let candidate = v["candidate"]["sha256"].string,
              [tenant, artwork, job].allSatisfy({ UUID(uuidString: $0)?.uuidString.lowercased() == $0 }),
              NativeGPUModelPreparation.digest(receipt), NativeGPUModelPreparation.digest(candidate) else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        let cms = "api/v1/tenants/\(tenant)/cms/artworks/\(artwork)/gpu-preparations/\(job)"
        let routes = ["native-viewport.html", "api/v1/auth/session", "api/v1/tenants/\(tenant)/imports/\(job)",
                      cms + "/review", "basis/basis_transcoder.js", "basis/basis_transcoder.wasm", "basis/exhibitos-basis-worker.js"]
        let model = cms + "/model"
        let queries = ["receiptSHA256=\(receipt)&candidateSHA256=\(candidate)", "candidateSHA256=\(candidate)&receiptSHA256=\(receipt)"]
        let base = "^http://127\\.0\\.0\\.1:\(port)/"
        // All dynamic parts are validated UUID/hex/asset ASCII. A dot is the
        // only regex metacharacter they can contain; keep the rules within
        // WebKit's content-blocker regex subset.
        func escape(_ s: String) -> String { s.replacingOccurrences(of: ".", with: "\\.") }
        let patterns = (routes + verifiedAssets.sorted()).map { base + escape($0) + "$" }
            + queries.map { base + escape(model) + "\\?" + escape($0) + "$" }
        var rules: [[String: Any]] = [["trigger": ["url-filter": ".*"], "action": ["type": "block"]]]
        for pattern in patterns {
            rules.append(["trigger": ["url-filter": pattern, "url-filter-is-case-sensitive": true], "action": ["type": "ignore-previous-rules"]])
        }
        return String(decoding: try JSONSerialization.data(withJSONObject: rules), as: UTF8.self)
    }
}
