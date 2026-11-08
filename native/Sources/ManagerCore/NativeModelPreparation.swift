// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeModelPreparationIntent: Codable, Equatable, Sendable {
    public static let profile = "embedded-model-compact-20k-512-v1"
    static let recipeSHA256 = "a11cd154369496ee9af9cac73538d8f4cfc49df0043aeaae3af186c96162344e"
    public let version: Int
    public let profileId: String
    public let operationID: String
    public let generation: String
    public let artworkRevision: Int

    public init(operationID: String, generation: String, artworkRevision: Int) {
        version = 1
        profileId = Self.profile
        self.operationID = operationID
        self.generation = generation
        self.artworkRevision = artworkRevision
    }

    var valid: Bool {
        let token = "^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"
        return version == 1 && profileId == Self.profile
            && operationID.range(of: token, options: .regularExpression) != nil
            && generation.range(of: token, options: .regularExpression) != nil
            && (1...2_147_483_647).contains(artworkRevision)
    }

    func matches(_ intent: NativeImportIntent) -> Bool {
        valid && operationID == intent.idempotencyKey && intent.mime == "model/gltf-binary"
            && intent.derived == nil && intent.modelDerivation == nil
            && intent.coarsePreparation == nil && intent.hayesOriginal == nil
    }
}

public struct NativeModelPreparationStatus: Decodable, Equatable, Sendable {
    public enum Phase: String, Decodable, Sendable { case pending, processing, ready, failed, cancelled }

    public struct Asset: Decodable, Equatable, Sendable {
        public let assetId: String?
        public let mime: String
        public let sha256: String
        public let bytes: Int
    }

    public struct Display: Decodable, Equatable, Sendable {
        public let assetId: String
        public let mime: String
        public let sha256: String
        public let bytes: Int
        public let reusedOriginal: Bool
    }

    public struct Receipt: Decodable, Equatable, Sendable {
        public struct Producer: Decodable, Equatable, Sendable {
            public struct Bounds: Decodable, Equatable, Sendable {
                public let min: [Double], max: [Double]
            }
            public struct Metrics: Decodable, Equatable, Sendable {
                public let sha256: String
                public let bytes: Int, triangles: Int, vertices: Int
                public let bounds: Bounds
                public let textureEdge: Int?
                public let reusedOriginal: Bool?
            }
            public struct Image: Decodable, Equatable, Sendable {
                public let role: String, sourceSHA256: String, decodedSHA256: String, outputSHA256: String
                public let sourceBytes: Int, sourceChannels: Int, width: Int, height: Int
                public let outputWidth: Int, outputHeight: Int
            }
            public struct Simplifier: Decodable, Equatable, Sendable {
                public let version: String
                public let relativeAppearanceError: Double
                public let lockedVertices: Int
            }
            public let version: Int
            public let recipeId: String, recipeSHA256: String, decoder: String
            public let source: Metrics, output: Metrics
            public let images: [Image]
            public let simplifier: Simplifier
        }
        public let version: Int
        public let profileId: String, recipeSHA256: String, jobId: String, artworkId: String
        public let artworkRevision: Int
        public let operationID: String, generation: String, rightsSHA256: String
        public let original: Asset, display: Display, producer: Producer
        public let sha256: String
    }

    public let intent: NativeModelPreparationIntent
    public let required: Bool
    public let status: Phase
    public let workerDrained: Bool
    public let original: Asset
    public let display: Display?
    public let serverReceipt: Receipt?

    private enum CodingKeys: String, CodingKey { case required, status, workerDrained, original, display, serverReceipt }
    public init(from decoder: Decoder) throws {
        intent = try NativeModelPreparationIntent(from: decoder)
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        required = try fields.decode(Bool.self, forKey: .required)
        status = try fields.decode(Phase.self, forKey: .status)
        workerDrained = try fields.decode(Bool.self, forKey: .workerDrained)
        original = try fields.decode(Asset.self, forKey: .original)
        display = try fields.decodeIfPresent(Display.self, forKey: .display)
        serverReceipt = try fields.decodeIfPresent(Receipt.self, forKey: .serverReceipt)
    }

    func matches(_ request: NativeImportIntent, job: NativeImportJob) -> Bool {
        guard required, request.modelPreparation == intent, intent.matches(request),
              original.mime == request.mime, original.sha256 == request.sha256, original.bytes == request.bytes else { return false }
        if status != .ready {
            guard original.assetId == nil, display == nil, serverReceipt == nil, job.assetId == nil else { return false }
            switch status {
            case .pending: return !workerDrained && ["uploading", "queued"].contains(job.state)
            case .processing: return !workerDrained && ["processing", "approved"].contains(job.state)
            case .failed: return job.state == "failed"
            case .cancelled: return job.state == "cancelled"
            case .ready: return false
            }
        }
        guard job.state == "approved", workerDrained, let originalId = original.assetId,
              UUID(uuidString: originalId) != nil, let display, UUID(uuidString: display.assetId) != nil,
              job.assetId == display.assetId, display.mime == request.mime, nativeImportDigest(display.sha256),
              (28...8 * 1024 * 1024).contains(display.bytes), let receipt = serverReceipt,
              receipt.version == 1, receipt.profileId == intent.profileId,
              receipt.recipeSHA256 == NativeModelPreparationIntent.recipeSHA256,
              receipt.jobId == job.id, receipt.artworkId == request.artworkId,
              receipt.artworkRevision == intent.artworkRevision, receipt.operationID == intent.operationID,
              receipt.generation == intent.generation, nativeImportDigest(receipt.rightsSHA256),
              nativeImportDigest(receipt.sha256), receipt.original == original, receipt.display == display else { return false }
        if display.reusedOriginal {
            guard originalId == display.assetId, original.sha256 == display.sha256, original.bytes == display.bytes else { return false }
        } else if originalId == display.assetId { return false }
        let producer = receipt.producer
        return producer.version == 1 && producer.recipeId == intent.profileId
            && producer.recipeSHA256 == receipt.recipeSHA256 && producer.source.sha256 == original.sha256
            && producer.source.bytes == original.bytes && producer.output.sha256 == display.sha256
            && producer.output.bytes == display.bytes && producer.output.reusedOriginal == display.reusedOriginal
    }

    public func permitsReview(of artwork: NativeArtwork, request: NativeImportIntent, job: NativeImportJob) -> Bool {
        guard matches(request, job: job), status == .ready, let display, let originalId = original.assetId,
              artwork.id == request.artworkId, artwork.revision == intent.artworkRevision, !artwork.archived,
              artwork.metadata?.rights == request.rights,
              artwork.assets.contains(where: { $0.id == display.assetId && $0.state == "approved"
                  && $0.mime == display.mime && $0.sha256 == display.sha256 && $0.bytes == display.bytes }) else { return false }
        return display.reusedOriginal || artwork.assets.contains(where: { $0.id == originalId && $0.state == "quarantine"
            && $0.mime == original.mime && $0.sha256 == original.sha256 && $0.bytes == original.bytes })
    }
}
