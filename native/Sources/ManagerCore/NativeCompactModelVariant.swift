// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A new preparation of an already approved full asset. This request carries
/// no input file and is distinct from required preparation of a new original.
public struct NativeCompactModelVariantRequest: Codable, Equatable, Sendable {
    public static let profile = "exhibitos-budget-geometry-pair-v1"
    public let revision: Int
    public let assetId: String
    public let sha256: String
    public let bytes: Int
    public let idempotencyKey: String
    public let generation: String

    public init(revision: Int, assetId: String, sha256: String, bytes: Int,
                idempotencyKey: String = UUID().uuidString.lowercased(),
                generation: String = UUID().uuidString.lowercased()) {
        self.revision = revision
        self.assetId = assetId
        self.sha256 = sha256
        self.bytes = bytes
        self.idempotencyKey = idempotencyKey
        self.generation = generation
    }

    var valid: Bool {
        let token = "^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"
        return (1..<2_147_483_647).contains(revision) && Self.validID(assetId)
            && nativeImportDigest(sha256) && (28...32 * 1024 * 1024).contains(bytes)
            && [idempotencyKey, generation].allSatisfy { $0.range(of: token, options: .regularExpression) != nil }
    }

    static func validID(_ value: String) -> Bool {
        UUID(uuidString: value)?.uuidString.lowercased() == value
    }

    func matchesFull(_ artwork: NativeArtwork, artworkId: String, revision: Int) -> Bool {
        valid && artwork.id == artworkId && !artwork.archived && artwork.metadata != nil
            && artwork.revision == revision && artwork.approvedRevision == revision
            && artwork.approvedAssetId == assetId
            && artwork.assets.contains { $0.id == assetId && $0.state == "approved"
                && $0.mime == "model/gltf-binary" && $0.sha256 == sha256 && $0.bytes == Int64(bytes) }
    }
}

public struct NativeCompactModelVariantJob: Decodable, Equatable, Sendable {
    public struct Asset: Decodable, Equatable, Sendable {
        public let assetId: String
        public let mime: String
        public let sha256: String
        public let bytes: Int

        func matches(_ asset: NativeArtwork.Asset) -> Bool {
            asset.id == assetId && asset.state == "approved" && asset.mime == mime
                && asset.sha256 == sha256 && asset.bytes == Int64(bytes)
        }
    }
    public struct Receipt: Decodable, Equatable, Sendable {
        public let version: Int
        public let purpose: String
        public let profileId: String
        public let publicationProfile: String
        public let recipeSHA256: String
        public let jobId: String
        public let artworkId: String
        public let artworkRevision: Int
        public let operationID: String
        public let generation: String
        public let rightsSHA256: String
        public let approvalSHA256: String
        public let sourceFull: Asset
        public let coarse: Asset
        public let producer: NativeModelPreparationStatus.Receipt.Producer
        public let sha256: String
    }
    public struct Preparation: Decodable, Equatable, Sendable {
        public let version: Int
        public let profileId: String
        public let purpose: String
        public let operationID: String
        public let generation: String
        public let artworkRevision: Int
        public let required: Bool
        public let status: NativeModelPreparationStatus.Phase
        public let workerDrained: Bool
        public let sourceFull: Asset
        public let coarse: Asset?
        public let serverReceipt: Receipt?
    }

    public let id: String
    public let state: String
    public let progress: Int
    public let attempts: Int
    public let assetId: String?
    public let errorCode: String?
    public let retryAt: String?
    public let modelPreparation: Preparation

    var automationValue: DocumentValue {
        let model = modelPreparation
        var preparation: [String: DocumentValue] = [
            "purpose": .string(model.purpose), "profileId": .string(model.profileId),
            "status": .string(model.status.rawValue), "workerDrained": .bool(model.workerDrained),
            "sourceFull": .object(["assetId": .string(model.sourceFull.assetId),
                "sha256": .string(model.sourceFull.sha256), "bytes": .integer(Int64(model.sourceFull.bytes))])
        ]
        if let coarse = model.coarse, let receipt = model.serverReceipt {
            preparation["coarse"] = .object(["assetId": .string(coarse.assetId), "sha256": .string(coarse.sha256),
                "bytes": .integer(Int64(coarse.bytes)), "triangles": .integer(Int64(receipt.producer.output.triangles)),
                "textureEdge": receipt.producer.output.textureEdge.map { .integer(Int64($0)) } ?? .null])
            preparation["serverReceiptSha256"] = .string(receipt.sha256)
        }
        return .object(["id": .string(id), "state": .string(state), "attempts": .integer(Int64(attempts)),
                        "errorCode": errorCode.map(DocumentValue.string) ?? .null, "modelPreparation": .object(preparation)])
    }

    func check(request: NativeCompactModelVariantRequest, artworkId: String) throws {
        let model = modelPreparation
        let full = model.sourceFull
        let invalid = AuthoringFailure("COMPACT_VARIANT_RECEIPT_INVALID")
        guard request.valid, NativeCompactModelVariantRequest.validID(id), model.version == 1,
              model.purpose == "approved-full-compact", model.required,
              model.profileId == NativeModelPreparationIntent.profile,
              model.artworkRevision == request.revision, model.operationID == request.idempotencyKey,
              model.generation == request.generation, full.assetId == request.assetId,
              full.mime == "model/gltf-binary", full.sha256 == request.sha256, full.bytes == request.bytes,
              (0...100).contains(progress), attempts >= 0 else { throw invalid }
        guard model.status == .ready else {
            guard model.coarse == nil, model.serverReceipt == nil, assetId == nil else { throw invalid }
            switch model.status {
            case .pending: guard state == "queued", !model.workerDrained else { throw invalid }
            case .processing: guard ["processing", "approved"].contains(state), !model.workerDrained else { throw invalid }
            case .failed: guard state == "failed" else { throw invalid }
            case .cancelled: guard state == "cancelled" else { throw invalid }
            case .ready: throw invalid
            }
            return
        }
        guard state == "approved", progress == 100, model.workerDrained,
              let coarse = model.coarse, let receipt = model.serverReceipt,
              NativeCompactModelVariantRequest.validID(coarse.assetId), coarse.assetId != full.assetId,
              assetId == coarse.assetId, coarse.mime == full.mime, nativeImportDigest(coarse.sha256),
              coarse.sha256 != full.sha256, (28...8 * 1024 * 1024).contains(coarse.bytes), coarse.bytes < full.bytes,
              receipt.version == 1, receipt.purpose == model.purpose, receipt.profileId == model.profileId,
              receipt.publicationProfile == NativeCompactModelVariantRequest.profile,
              receipt.recipeSHA256 == NativeModelPreparationIntent.recipeSHA256,
              receipt.jobId == id, receipt.artworkId == artworkId, receipt.artworkRevision == request.revision,
              receipt.operationID == request.idempotencyKey, receipt.generation == request.generation,
              [receipt.approvalSHA256, receipt.rightsSHA256, receipt.sha256].allSatisfy(nativeImportDigest),
              receipt.sourceFull == full, receipt.coarse == coarse else { throw invalid }
        let producer = receipt.producer
        guard producer.version == 1, producer.recipeId == model.profileId, producer.recipeSHA256 == receipt.recipeSHA256,
              producer.source.sha256 == full.sha256, producer.source.bytes == full.bytes,
              producer.output.sha256 == coarse.sha256, producer.output.bytes == coarse.bytes,
              producer.output.reusedOriginal == false, producer.source.bounds == producer.output.bounds,
              (1...20_000).contains(producer.output.triangles), (1...20_000).contains(producer.output.vertices),
              producer.output.triangles < producer.source.triangles,
              let edge = producer.output.textureEdge, (1...512).contains(edge) else { throw invalid }
    }
}

public struct NativeCompactModelVariantApproval: Codable, Equatable, Sendable {
    public let artworkId: String
    public let revision: Int
    public let jobId: String
    public let receiptSHA256: String
}
