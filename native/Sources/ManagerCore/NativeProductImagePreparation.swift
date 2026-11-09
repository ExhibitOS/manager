// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit

public struct NativeProductImagePreparationIntent: Codable, Equatable, Sendable {
    public static let profileID = "product-image-pair-2048-512-v1"
    public static let freshProfileID = "ordinary-image-preparation-v1"
    public static let maximumSourceBytes = 32 * 1024 * 1024
    public let version: Int
    public let profile: String
    public let artworkRevision: Int

    public init(artworkRevision: Int, fresh: Bool = false) {
        version = 1
        profile = fresh ? Self.freshProfileID : Self.profileID
        self.artworkRevision = artworkRevision
    }

    private enum CodingKeys: String, CodingKey { case version, profile, artworkRevision }

    public init(from decoder: Decoder) throws {
        try requireProductImageKeys(DocumentValue(from: decoder), ["version", "profile", "artworkRevision"])
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        version = try fields.decode(Int.self, forKey: .version)
        profile = try fields.decode(String.self, forKey: .profile)
        artworkRevision = try fields.decode(Int.self, forKey: .artworkRevision)
        guard valid else { throw AuthoringFailure("IMAGE_PREPARATION_INTENT_INVALID") }
    }

    public var isFresh: Bool { profile == Self.freshProfileID }

    var valid: Bool {
        version == 1 && (isFresh || profile == Self.profileID) && (1..<2_147_483_647).contains(artworkRevision)
    }

    func accepts(mime: String) -> Bool {
        mime == "image/jpeg" || (isFresh && mime == "image/png")
    }

    func matches(_ intent: NativeImportIntent) -> Bool {
        valid && accepts(mime: intent.mime) && intent.scaleMeters == 1
            && (4...Self.maximumSourceBytes).contains(intent.bytes) && nativeImportDigest(intent.sha256)
            && intent.derived == nil && intent.coarsePreparation == nil && intent.modelDerivation == nil
            && intent.modelPreparation == nil && intent.hayesOriginal == nil
    }

    public func checkFreshArtwork(_ artwork: NativeArtwork, rights: ArtworkMetadata.Rights, approvedAssetId: String? = nil) throws {
        guard isFresh, valid, !artwork.archived, artwork.revision == artworkRevision,
              artwork.metadata?.displayReference == nil, artwork.metadata?.rights == rights,
              rights.permissions.display, rights.permissions.export,
              artwork.approvedAssetId == nil && artwork.approvedRevision == nil
                || (approvedAssetId != nil && artwork.approvedAssetId == approvedAssetId && artwork.approvedRevision == artworkRevision)
        else { throw AuthoringFailure("IMAGE_PREPARATION_CONTEXT_CHANGED") }
    }

    public func checkSource(_ file: ImportFileSnapshot, artwork: NativeArtwork) throws {
        guard valid, !isFresh, file.mime == "image/jpeg", (4...Self.maximumSourceBytes).contains(file.data.count),
              !artwork.archived, artwork.revision == artworkRevision, artwork.approvedRevision == artworkRevision,
              let metadata = artwork.metadata, metadata.displayReference == nil,
              metadata.rights.permissions.display, metadata.rights.permissions.export,
              artwork.assets.contains(where: { $0.id == artwork.approvedAssetId && $0.mime == "image/png" && $0.state == "approved" }) else {
            throw AuthoringFailure("IMAGE_PREPARATION_CONTEXT_CHANGED")
        }
    }

    public static func definitivelyRejectedCreate(_ error: AuthoringFailure) -> Bool {
        (error.status == 400 && error.code == "IMAGE_PREPARATION_INPUT_INVALID")
            || (error.status == 409 && error.code == "IMAGE_PREPARATION_CONTEXT_CHANGED")
    }
}

public struct NativeProductImagePreparationStatus: Codable, Equatable, Sendable {
    public enum Phase: String, Codable, Sendable { case pending, failed, ready }

    public struct Original: Codable, Equatable, Sendable {
        public let assetId: String
        public let mime: String
        public let sha256: String
        public let bytes: Int
    }

    public struct Image: Codable, Equatable, Sendable {
        public let assetId: String
        public let mime: String
        public let sha256: String
        public let bytes: Int
        public let width: Int
        public let height: Int

        fileprivate func valid(maximumEdge: Int) -> Bool {
            mime == "image/png" && productImageID(assetId) && nativeImportDigest(sha256)
                && (1...8 * 1024 * 1024).contains(bytes)
                && (1...maximumEdge).contains(width) && (1...maximumEdge).contains(height)
        }
    }

    public struct Receipt: Codable, Equatable, Sendable {
        public struct Producer: Codable, Equatable, Sendable {
            public struct Metric: Codable, Equatable, Sendable {
                public let sha256: String
                public let bytes: Int
                public let width: Int
                public let height: Int

                fileprivate func matches(_ image: Image) -> Bool {
                    sha256 == image.sha256 && bytes == image.bytes && width == image.width && height == image.height
                }
            }

            public struct Source: Codable, Equatable, Sendable {
                public let mime: String
                public let sha256: String
                public let bytes: Int
                public let width: Int
                public let height: Int
                public let iccSHA256: String?

                private enum CodingKeys: String, CodingKey { case mime, sha256, bytes, width, height, iccSHA256 }

                public func encode(to encoder: Encoder) throws {
                    var fields = encoder.container(keyedBy: CodingKeys.self)
                    try fields.encode(mime, forKey: .mime)
                    try fields.encode(sha256, forKey: .sha256)
                    try fields.encode(bytes, forKey: .bytes)
                    try fields.encode(width, forKey: .width)
                    try fields.encode(height, forKey: .height)
                    try fields.encode(iccSHA256, forKey: .iccSHA256)
                }
            }

            public let version: Int
            public let recipeSHA256: String
            public let decoder: String
            public let source: Source
            public let full: Metric
            public let coarse: Metric?
        }

        public let version: Int
        public let jobId: String
        public let artworkId: String
        public let artworkRevision: Int
        public let workspaceId: String
        public let principalId: String
        public let rightsSHA256: String
        public let original: Original
        public let full: Image
        public let coarse: Image?
        public let producer: Producer
        public let sha256: String

        private enum CodingKeys: String, CodingKey {
            case version, jobId, artworkId, artworkRevision, workspaceId, principalId, rightsSHA256
            case original, full, coarse, producer, sha256
        }

        public init(from decoder: Decoder) throws {
            let wire = try DocumentValue(from: decoder)
            let required: Set<String> = ["version", "jobId", "artworkId", "artworkRevision", "workspaceId",
                                         "principalId", "rightsSHA256", "original", "full", "producer", "sha256"]
            try requireProductImageKeys(wire, required, optional: ["coarse"])
            try requireProductImageKeys(wire["original"], ["assetId", "mime", "sha256", "bytes"])
            try checkProductImageMetrics(wire, asset: true)
            let producerWire = wire["producer"]
            try requireProductImageKeys(producerWire, ["version", "recipeSHA256", "decoder", "source", "full"], optional: ["coarse"])
            try requireProductImageKeys(producerWire["source"], ["mime", "sha256", "bytes", "width", "height", "iccSHA256"])
            try checkProductImageMetrics(producerWire, asset: false)

            let fields = try decoder.container(keyedBy: CodingKeys.self)
            version = try fields.decode(Int.self, forKey: .version)
            jobId = try fields.decode(String.self, forKey: .jobId)
            artworkId = try fields.decode(String.self, forKey: .artworkId)
            artworkRevision = try fields.decode(Int.self, forKey: .artworkRevision)
            workspaceId = try fields.decode(String.self, forKey: .workspaceId)
            principalId = try fields.decode(String.self, forKey: .principalId)
            rightsSHA256 = try fields.decode(String.self, forKey: .rightsSHA256)
            original = try fields.decode(Original.self, forKey: .original)
            full = try fields.decode(Image.self, forKey: .full)
            coarse = try fields.decodeIfPresent(Image.self, forKey: .coarse)
            producer = try fields.decode(Producer.self, forKey: .producer)
            sha256 = try fields.decode(String.self, forKey: .sha256)
            guard case .object(var body) = wire else { throw invalidProductImageReceipt() }
            body.removeValue(forKey: "sha256")
            guard try productImageHash(DocumentValue.object(body)) == sha256 else { throw invalidProductImageReceipt() }
        }
    }

    public let provenance: String
    public let profile: String
    public let artworkRevision: Int
    public let status: Phase
    public let workerDrained: Bool
    public let original: Original?
    public let full: Image?
    public let coarse: Image?
    public let serverReceipt: Receipt?

    private enum CodingKeys: String, CodingKey {
        case provenance, profile, artworkRevision, status, workerDrained, original, full, coarse, serverReceipt
    }

    public init(from decoder: Decoder) throws {
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        status = try fields.decode(Phase.self, forKey: .status)
        var required: Set<String> = ["provenance", "profile", "artworkRevision", "status", "workerDrained"]
        let wire = try DocumentValue(from: decoder)
        if status == .ready {
            required.formUnion(["original", "full", "serverReceipt"])
            try requireProductImageKeys(wire["original"], ["assetId", "mime", "sha256", "bytes"])
            try checkProductImageMetrics(wire, asset: true)
        }
        try requireProductImageKeys(wire, required, optional: status == .ready ? ["coarse"] : [])
        provenance = try fields.decode(String.self, forKey: .provenance)
        profile = try fields.decode(String.self, forKey: .profile)
        artworkRevision = try fields.decode(Int.self, forKey: .artworkRevision)
        workerDrained = try fields.decode(Bool.self, forKey: .workerDrained)
        original = fields.contains(.original) ? try fields.decode(Original.self, forKey: .original) : nil
        full = fields.contains(.full) ? try fields.decode(Image.self, forKey: .full) : nil
        coarse = fields.contains(.coarse) ? try fields.decode(Image.self, forKey: .coarse) : nil
        serverReceipt = fields.contains(.serverReceipt) ? try fields.decode(Receipt.self, forKey: .serverReceipt) : nil
    }

    static func matchesCanonicalTiers(_ artwork: DocumentValue, full: Image, coarse: Image?) -> Bool {
        let lod = artwork["extensions"]["org.exhibitos.viewer/lod"]
        guard lod["version"].integer == 1, let variants = lod["variants"].items,
              variants.count == (coarse == nil ? 1 : 2) else { return false }
        func hasTier(_ image: Image, _ detail: String) -> Bool {
            variants.contains {
                $0["assetId"].string == image.assetId && $0["detail"].string == detail
                    && $0["textureSize"].integer == Int64(max(image.width, image.height))
            }
        }
        return hasTier(full, "full") && (coarse.map { hasTier($0, "coarse") } ?? true)
    }

    public func matches(_ preparation: NativeProductImagePreparationIntent, job: NativeImportJob) -> Bool {
        guard preparation.valid, provenance == "server-generated", profile == preparation.profile,
              artworkRevision == preparation.artworkRevision, productImageID(job.id) else { return false }
        switch status {
        case .pending:
            return !workerDrained && ["uploading", "queued", "processing", "approved"].contains(job.state)
                && job.assetId == nil && job.originalAssetId == nil
        case .failed:
            return ["failed", "cancelled"].contains(job.state) && job.assetId == nil && job.originalAssetId == nil
        case .ready:
            guard workerDrained, job.state == "approved", let original, let full, let receipt = serverReceipt,
                  original.assetId == job.originalAssetId, full.assetId == job.assetId, receipt.jobId == job.id,
                  receipt.artworkRevision == artworkRevision, receipt.original == original,
                  receipt.full == full, receipt.coarse == coarse else { return false }
            return true
        }
    }

    public func validatesReady(intent: NativeImportIntent, preparation: NativeProductImagePreparationIntent,
                               job: NativeImportJob, workspaceID: String, principalID: String) -> Bool {
        guard intent.productImagePreparation == preparation, matches(preparation, job: job),
              status == .ready, let receipt = serverReceipt,
              let original, let full, receipt.version == 1,
              receipt.artworkId == intent.artworkId.lowercased(), productImageID(receipt.artworkId),
              receipt.workspaceId == workspaceID.lowercased(), productImageID(receipt.workspaceId),
              receipt.principalId == principalID.lowercased(), productImageID(receipt.principalId),
              (try? productImageHash(intent.rights)) == receipt.rightsSHA256,
              preparation.matches(intent), original.mime == intent.mime, nativeImportDigest(original.sha256),
              original.sha256 == intent.sha256, original.bytes == intent.bytes,
              (4...NativeProductImagePreparationIntent.maximumSourceBytes).contains(original.bytes),
              productImageID(original.assetId), original.assetId.lowercased() != full.assetId.lowercased(),
              full.valid(maximumEdge: 2048)
        else { return false }

        let producer = receipt.producer
        let source = producer.source
        guard producer.version == 1,
              producer.recipeSHA256 == (preparation.isFresh
                ? "d13ad48b1ae8c29818002225fb5ebb4840907646810d90922a4ea3a5448f06c0"
                : "f60ff6877a53bf48aabd066f60eab3ea39c6c4e6d97b97856af8ea1b8294430d"),
              producer.decoder == "sharp-0.35.4-vips-8.18.6-mozjpeg-0826579",
              source.mime == original.mime, source.sha256 == original.sha256, source.bytes == original.bytes,
              (1...8192).contains(source.width), (1...8192).contains(source.height),
              source.width * source.height <= 24 * 1024 * 1024,
              source.mime != "image/png" || source.iccSHA256 == nil,
              source.iccSHA256.map(nativeImportDigest) ?? true, producer.full.matches(full),
              [2048, 1600].contains(where: { edge in
                  scaledProductImage(width: source.width, height: source.height, edge: edge) == (full.width, full.height)
              }) else { return false }

        guard let coarse else { return producer.coarse == nil }
        return coarse.valid(maximumEdge: 512) && max(full.width, full.height) > 512
            && coarse.bytes < full.bytes && coarse.assetId.lowercased() != original.assetId.lowercased()
            && coarse.assetId.lowercased() != full.assetId.lowercased()
            && producer.coarse?.matches(coarse) == true
            && scaledProductImage(width: full.width, height: full.height, edge: 512) == (coarse.width, coarse.height)
    }
}

private func invalidProductImageReceipt() -> AuthoringFailure { AuthoringFailure("IMAGE_PREPARATION_RECEIPT_INVALID") }

private func productImageID(_ value: String) -> Bool {
    value.range(of: #"^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"#,
                options: [.regularExpression, .caseInsensitive]) != nil
}

private func requireProductImageKeys(_ value: DocumentValue, _ required: Set<String>, optional: Set<String> = []) throws {
    guard case .object(let fields) = value else { throw invalidProductImageReceipt() }
    let keys = Set(fields.keys)
    guard required.isSubset(of: keys), keys.isSubset(of: required.union(optional)) else { throw invalidProductImageReceipt() }
}

private func checkProductImageMetrics(_ value: DocumentValue, asset: Bool) throws {
    var keys: Set<String> = ["sha256", "bytes", "width", "height"]
    if asset { keys.formUnion(["assetId", "mime"]) }
    try requireProductImageKeys(value["full"], keys)
    if case .object(let fields) = value, let coarse = fields["coarse"] {
        try requireProductImageKeys(coarse, keys)
    }
}

private func productImageHash<T: Encodable>(_ value: T) throws -> String {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    return try SHA256.hash(data: encoder.encode(value)).map { String(format: "%02x", $0) }.joined()
}

private func scaledProductImage(width: Int, height: Int, edge: Int) -> (Int, Int) {
    let ratio = min(1, Double(edge) / Double(max(width, height)))
    return (max(1, Int((Double(width) * ratio).rounded())), max(1, Int((Double(height) * ratio).rounded())))
}
