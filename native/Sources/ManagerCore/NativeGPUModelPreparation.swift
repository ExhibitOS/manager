// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit

/// Structural bindings for a server-owned derivative. They do not advertise a
/// capability or validate GLB/KTX bytes; those remain server/runtime duties.
public enum NativeGPUModelPreparation {
    public static let profile = "embedded-model-basecolor-uastc-512-mips-v1"
    public static let purpose = "approved-display-gpu-derivative"
    // Retained for identifying historical vendor receipts; never active acceptance.
    static let historicalVendorRecipe = "6739a173b5a3af38f3fb02b250a2fa4d3315972f62ea8592f1c5695ab5848d98"
    public static let recipe = "886e3815a386a46462e46d35ccb640498ebefcc853cefba61f76560b62cae4bc"
    public static let geometryAlgorithm = "glb-source-preservation-v1"
    static func invalid() -> AuthoringFailure { AuthoringFailure("GPU_PREPARATION_INVALID") }
    static func id(_ s: String?) -> Bool { guard let s else { return false }; return UUID(uuidString: s) != nil }
    static func digest(_ s: String?) -> Bool {
        guard let bytes = s?.utf8, bytes.count == 64 else { return false }
        return bytes.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    static func token(_ s: String?) -> Bool {
        guard let bytes = s?.utf8, (1...128).contains(bytes.count), let first = bytes.first else { return false }
        func alphanumeric(_ c: UInt8) -> Bool { (48...57).contains(c) || (65...90).contains(c) || (97...122).contains(c) }
        return alphanumeric(first) && bytes.allSatisfy { alphanumeric($0) || $0 == 46 || $0 == 95 || $0 == 45 }
    }
    static func exact(_ v: DocumentValue, _ keys: Set<String>) -> Bool {
        guard case .object(let f) = v else { return false }; return Set(f.keys) == keys
    }
    static func integer(_ v: DocumentValue, _ range: ClosedRange<Int64>) -> Bool {
        guard let n = v.integer else { return false }; return range.contains(n)
    }
    static func sameID(_ a: String?, _ b: String?) -> Bool {
        guard let a, let b else { return false }; return a.lowercased() == b.lowercased()
    }
    static func source(_ v: DocumentValue) -> Bool {
        exact(v, ["assetId", "mime", "sha256", "bytes", "approvalSHA256", "rightsSHA256"])
            && id(v["assetId"].string) && v["mime"] == .string("model/gltf-binary")
            && digest(v["sha256"].string) && integer(v["bytes"], 28...33_554_432)
            && digest(v["approvalSHA256"].string) && digest(v["rightsSHA256"].string)
    }
    static func candidate(_ v: DocumentValue) -> Bool {
        exact(v, ["assetId", "mime", "sha256", "bytes"]) && id(v["assetId"].string)
            && v["mime"] == .string("model/gltf-binary") && digest(v["sha256"].string)
            && integer(v["bytes"], 28...Int64.max)
    }
    static func producer(_ p: DocumentValue) -> Bool {
        guard exact(p, ["version", "tool", "geometry", "samplers", "roles", "ktx"]), p["version"] == .integer(1) else { return false }
        let t = p["tool"], g = p["geometry"], s = p["samplers"], k = p["ktx"]
        guard exact(t, ["name", "version", "executableSHA256", "librarySHA256", "recipeSHA256"]),
              t["name"] == .string("toktx"), t["version"] == .string("4.4.2"), t["recipeSHA256"] == .string(recipe),
              digest(t["executableSHA256"].string), digest(t["librarySHA256"].string),
              exact(g, ["algorithm", "sourceSHA256", "outputSHA256", "triangles", "vertices"]),
              g["algorithm"] == .string(geometryAlgorithm), digest(g["sourceSHA256"].string),
              g["sourceSHA256"] == g["outputSHA256"], integer(g["triangles"], 0...Int64.max), integer(g["vertices"], 0...Int64.max),
              exact(s, ["sourceSHA256", "outputSHA256", "semantics"]), digest(s["sourceSHA256"].string), s["sourceSHA256"] == s["outputSHA256"],
              s["semantics"] == .string("source-preserved-implicit-repeat-mipmapped"),
              let roles = p["roles"].items, roles.count == 3 else { return false }
        for (index, name) in ["baseColor", "normal", "occlusion"].enumerated() {
            let r = roles[index]
            guard exact(r, ["role", "textureIndex", "sourceImageIndex", "sourceSHA256", "sourceBytes", "outputPngImageIndex", "outputPngSHA256", "outputPngBytes", "width", "height", "colorSpace"]),
                  r["role"] == .string(name), integer(r["textureIndex"], 0...Int64.max), integer(r["sourceImageIndex"], 0...Int64.max),
                  r["sourceImageIndex"] == r["outputPngImageIndex"], digest(r["sourceSHA256"].string), r["sourceSHA256"] == r["outputPngSHA256"],
                  integer(r["sourceBytes"], 1...Int64.max), r["sourceBytes"] == r["outputPngBytes"],
                  r["width"] == .integer(512), r["height"] == .integer(512), r["colorSpace"] == .string(index == 0 ? "srgb" : "linear") else { return false }
        }
        return exact(k, ["imageIndex", "sha256", "bytes", "width", "height", "levels", "format", "transfer", "opaque", "levelDimensions"])
            && integer(k["imageIndex"], 0...Int64.max) && digest(k["sha256"].string) && integer(k["bytes"], 1...Int64.max)
            && k["width"] == .integer(512) && k["height"] == .integer(512) && k["levels"] == .integer(10)
            && k["format"] == .string("uastc") && k["transfer"] == .string("srgb") && k["opaque"] == .bool(true)
            && k["levelDimensions"] == .array([512,256,128,64,32,16,8,4,2,1].map { .integer(Int64($0)) })
    }
}

public struct NativeGPUModelPreparationRequest: Codable, Equatable, Sendable {
    public let revision: Int, assetId: String, sha256: String, bytes: Int
    public let idempotencyKey: String, generation: String, profile: String
    enum CodingKeys: String, CodingKey { case revision, assetId, sha256, bytes, idempotencyKey, generation, profile }
    public init(from decoder: Decoder) throws {
        let raw = try decoder.container(keyedBy: NativeGPUModelCodingKey.self)
        guard Set(raw.allKeys.map(\.stringValue)) == ["revision", "assetId", "sha256", "bytes", "idempotencyKey", "generation", "profile"] else { throw NativeGPUModelPreparation.invalid() }
        let values = try decoder.container(keyedBy: CodingKeys.self)
        try self.init(revision: values.decode(Int.self, forKey: .revision), assetId: values.decode(String.self, forKey: .assetId), sha256: values.decode(String.self, forKey: .sha256), bytes: values.decode(Int.self, forKey: .bytes), idempotencyKey: values.decode(String.self, forKey: .idempotencyKey), generation: values.decode(String.self, forKey: .generation))
        guard try values.decode(String.self, forKey: .profile) == profile else { throw NativeGPUModelPreparation.invalid() }
    }
    public init(revision: Int, assetId: String, sha256: String, bytes: Int, idempotencyKey: String, generation: String) throws {
        guard (1...2_147_483_647).contains(revision), NativeGPUModelPreparation.id(assetId), NativeGPUModelPreparation.digest(sha256),
              (28...33_554_432).contains(bytes), NativeGPUModelPreparation.token(idempotencyKey), NativeGPUModelPreparation.token(generation) else { throw NativeGPUModelPreparation.invalid() }
        self.revision = revision; self.assetId = assetId; self.sha256 = sha256; self.bytes = bytes
        self.idempotencyKey = idempotencyKey; self.generation = generation; self.profile = NativeGPUModelPreparation.profile
    }
    public var valid: Bool {
        (1...2_147_483_647).contains(revision) && NativeGPUModelPreparation.id(assetId) && NativeGPUModelPreparation.digest(sha256)
            && (28...33_554_432).contains(bytes) && NativeGPUModelPreparation.token(idempotencyKey)
            && NativeGPUModelPreparation.token(generation) && profile == NativeGPUModelPreparation.profile
    }
    public func matchesApprovedSource(_ artwork: NativeArtwork, artworkId: String) -> Bool {
        valid && NativeGPUModelPreparation.sameID(artwork.id, artworkId) && !artwork.archived && artwork.revision == revision
            && artwork.approvedRevision == revision && NativeGPUModelPreparation.sameID(artwork.approvedAssetId, assetId)
            && artwork.assets.contains { NativeGPUModelPreparation.sameID($0.id, assetId) && $0.state == "approved" && $0.mime == "model/gltf-binary" && $0.sha256 == sha256 && $0.bytes == Int64(bytes) }
    }
}

/// Exact GPU-family job only; mixed legacy payloads are rejected. The receipt
/// digest is recomputed and compared to an independently server-verified receipt;
/// this does not prove geometry preservation on the client.
public struct NativeGPUModelPreparationJob: Sendable, Equatable {
    public let value: DocumentValue
    public let artworkId: String
    public let request: NativeGPUModelPreparationRequest
    public init(_ v: DocumentValue, artworkId: String, request: NativeGPUModelPreparationRequest, verifiedReceiptSHA256: String? = nil) throws {
        guard request.valid, NativeGPUModelPreparation.id(artworkId), case .object(let f) = v, Set(f.keys).subtracting(["assetId"]) == ["id", "state", "progress", "attempts", "errorCode", "retryAt", "gpuPreparation"],
              NativeGPUModelPreparation.id(v["id"].string), let state = v["state"].string,
              ["queued", "processing", "approved", "failed", "cancelled"].contains(state),
              NativeGPUModelPreparation.integer(v["progress"], 0...100), NativeGPUModelPreparation.integer(v["attempts"], 0...5),
              v["errorCode"] == .null || (v["errorCode"].string?.isEmpty == false),
              let retry = v["retryAt"].string, ISO8601DateFormatter().date(from: retry) != nil || NativeGPUModelPreparation.fractionalDate(retry) else { throw NativeGPUModelPreparation.invalid() }
        let n = v["gpuPreparation"]
        guard case .object(let nf) = n,
              Set(nf.keys).subtracting(["candidate", "serverReceipt"]) == ["version", "purpose", "profile", "recipeSHA256", "operationID", "generation", "artworkRevision", "status", "workerDrained", "source"],
              n["version"] == .integer(1), n["purpose"] == .string(NativeGPUModelPreparation.purpose), n["profile"] == .string(NativeGPUModelPreparation.profile),
              n["recipeSHA256"] == .string(NativeGPUModelPreparation.recipe), n["operationID"] == .string(request.idempotencyKey), n["generation"] == .string(request.generation),
              n["artworkRevision"] == .integer(Int64(request.revision)), NativeGPUModelPreparation.source(n["source"]),
              NativeGPUModelPreparation.sameID(n["source"]["assetId"].string, request.assetId), n["source"]["sha256"] == .string(request.sha256), n["source"]["bytes"] == .integer(Int64(request.bytes)),
              case .bool(let drained) = n["workerDrained"], let phase = n["status"].string else { throw NativeGPUModelPreparation.invalid() }
        let allowed = (state == "queued" && phase == "pending") || (state == "processing" && phase == "processing") || (state == "approved" && !drained && phase == "processing") || (state == "approved" && drained && phase == "ready") || (state == "failed" && phase == "failed") || (state == "cancelled" && phase == "cancelled")
        guard allowed else { throw NativeGPUModelPreparation.invalid() }
        if phase == "ready" {
            let c = n["candidate"], r = n["serverReceipt"]
            guard v["progress"] == .integer(100), NativeGPUModelPreparation.candidate(c),
                  !NativeGPUModelPreparation.sameID(c["assetId"].string, request.assetId), v["assetId"] == c["assetId"],
                  NativeGPUModelPreparation.receipt(r, jobId: v["id"].string!, artworkId: artworkId, request: request, source: n["source"], candidate: c, verifiedReceiptSHA256: verifiedReceiptSHA256) else { throw NativeGPUModelPreparation.invalid() }
        } else {
            guard nf["candidate"] == nil, nf["serverReceipt"] == nil, f["assetId"] == nil else { throw NativeGPUModelPreparation.invalid() }
        }
        value = v; self.artworkId = artworkId; self.request = request
    }

    /// Compare against freshly obtained authoritative approval and rights digests.
    /// Parsing a job alone does not establish that either is still current.
    public func matchesCurrentSourceAuthority(approvalSHA256: String, rightsSHA256: String) -> Bool {
        NativeGPUModelPreparation.digest(approvalSHA256) && NativeGPUModelPreparation.digest(rightsSHA256)
            && value["gpuPreparation"]["source"]["approvalSHA256"] == .string(approvalSHA256)
            && value["gpuPreparation"]["source"]["rightsSHA256"] == .string(rightsSHA256)
    }
}

extension NativeGPUModelPreparation {
    static func fractionalDate(_ s: String) -> Bool { let f = ISO8601DateFormatter(); f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]; return f.date(from: s) != nil }
    static func receipt(_ r: DocumentValue, jobId: String, artworkId: String, request: NativeGPUModelPreparationRequest, source: DocumentValue, candidate: DocumentValue, verifiedReceiptSHA256: String?) -> Bool {
        guard case .object(var unsigned) = r else { return false }
        unsigned.removeValue(forKey: "sha256")
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        guard let canonical = try? encoder.encode(DocumentValue.object(unsigned)) else { return false }
        let computed = SHA256.hash(data: canonical).map { String(format: "%02x", $0) }.joined()
        return exact(r, ["version", "purpose", "profile", "recipeSHA256", "jobId", "artworkId", "artworkRevision", "operationID", "generation", "source", "candidate", "producer", "sha256"])
            && r["version"] == .integer(1) && r["purpose"] == .string(purpose) && r["profile"] == .string(profile) && r["recipeSHA256"] == .string(recipe)
            && sameID(r["jobId"].string, jobId) && sameID(r["artworkId"].string, artworkId) && r["artworkRevision"] == .integer(Int64(request.revision))
            && r["operationID"] == .string(request.idempotencyKey) && r["generation"] == .string(request.generation) && r["source"] == source && r["candidate"] == candidate
            && producer(r["producer"]) && digest(r["sha256"].string) && r["sha256"].string == computed && r["sha256"].string == verifiedReceiptSHA256
    }
}

struct NativeGPUModelCodingKey: CodingKey {
    let stringValue: String
    init?(stringValue: String) { self.stringValue = stringValue }
    var intValue: Int? { nil }
    init?(intValue: Int) { return nil }
}
