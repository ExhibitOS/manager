// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeModelPreparationIntent: Codable, Equatable, Sendable {
    public static let profile = "embedded-model-compact-20k-512-v1"
    public static let largeProfile = "embedded-model-compact-large-20k-512-v2"
    public static let largeSourceLimit = 64 * 1024 * 1024
    static let largeRecipeSHA256 = "0f28c31aca95ed70441e2229bbb7db72ab0b367c54708f39a7bfe54b6c50a2b6"
    static let recipeSHA256 = "a11cd154369496ee9af9cac73538d8f4cfc49df0043aeaae3af186c96162344e"
    public let version: Int
    public let profileId: String
    public let operationID: String
    public let generation: String
    public let artworkRevision: Int

    public init(operationID: String, generation: String, artworkRevision: Int, large: Bool = false) {
        version = 1
        profileId = large ? Self.largeProfile : Self.profile
        self.operationID = operationID
        self.generation = generation
        self.artworkRevision = artworkRevision
    }

    public static func requireFreshIntake(for artwork: NativeArtwork) throws {
        guard artwork.approvedRevision != artwork.revision else {
            throw AuthoringFailure("IMPORT_MODEL_NEW_REVISION_REQUIRED")
        }
    }

    var isLarge: Bool { profileId == Self.largeProfile }

    var valid: Bool {
        let token = "^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"
        return version == 1 && (profileId == Self.profile || isLarge)
            && operationID.range(of: token, options: .regularExpression) != nil
            && generation.range(of: token, options: .regularExpression) != nil
            && (1...2_147_483_647).contains(artworkRevision)
    }

    func matches(_ intent: NativeImportIntent) -> Bool {
        valid && operationID == intent.idempotencyKey && intent.mime == "model/gltf-binary"
            && intent.derived == nil && intent.modelDerivation == nil
            && intent.coarsePreparation == nil && intent.hayesOriginal == nil
            && (!isLarge || (28...Self.largeSourceLimit).contains(intent.bytes))
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
        public let displayReference: ArtworkMetadata.DisplayReference?
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
              receipt.recipeSHA256 == (intent.isLarge ? NativeModelPreparationIntent.largeRecipeSHA256 : NativeModelPreparationIntent.recipeSHA256),
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
            && (receipt.displayReference?.matches(display: display, producer: producer) ?? true)
    }

    var approvalRevision: Int {
        guard let reference = serverReceipt?.displayReference, reference.assetSha256 != original.sha256 else {
            return intent.artworkRevision
        }
        return intent.artworkRevision + 1
    }

    func approvedMetadata(from metadata: ArtworkMetadata) -> ArtworkMetadata {
        var approved = metadata
        if let reference = serverReceipt?.displayReference { approved.displayReference = reference }
        return approved
    }

    private func matchesArtworkRevision(_ artwork: NativeArtwork) -> Bool {
        guard let metadata = artwork.metadata else { return false }
        guard let reference = serverReceipt?.displayReference else {
            return metadata.displayReference == nil && metadata.provenance.sourceUnits != "unknown"
                && artwork.revision == intent.artworkRevision
        }
        guard metadata.provenance.sourceUnits == "unknown", !metadata.provenance.scaleApplied else { return false }
        if artwork.revision == intent.artworkRevision {
            return metadata.displayReference == reference.binding(to: original.sha256)
        }
        return artwork.revision == approvalRevision && artwork.approvedRevision == approvalRevision
            && artwork.approvedAssetId == display?.assetId && metadata.displayReference == reference
    }

    public func permitsReview(of artwork: NativeArtwork, request: NativeImportIntent, job: NativeImportJob) -> Bool {
        guard matches(request, job: job), status == .ready, let display, let originalId = original.assetId,
              artwork.id == request.artworkId, matchesArtworkRevision(artwork), !artwork.archived,
              artwork.metadata?.rights == request.rights,
              artwork.assets.contains(where: { $0.id == display.assetId && $0.state == "approved"
                  && $0.mime == display.mime && $0.sha256 == display.sha256 && $0.bytes == display.bytes }) else { return false }
        return display.reusedOriginal || artwork.assets.contains(where: { $0.id == originalId && $0.state == "quarantine"
            && $0.mime == original.mime && $0.sha256 == original.sha256 && $0.bytes == original.bytes })
    }

    public func permitsClearingTransfer(artwork: NativeArtwork?, request: NativeImportIntent, job: NativeImportJob) -> Bool {
        if status == .cancelled { return workerDrained && matches(request, job: job) }
        guard let artwork, permitsReview(of: artwork, request: request, job: job) else { return false }
        return artwork.approvedRevision == artwork.revision && artwork.approvedAssetId == job.assetId
    }
}

extension ArtworkMetadata.DisplayReference {
    func binding(to sha256: String) -> Self {
        .init(version: version, status: status, assetSha256: sha256, rawBounds: rawBounds,
              referenceHeightMeters: referenceHeightMeters, pose: pose)
    }

    fileprivate func matches(display: NativeModelPreparationStatus.Display,
                             producer: NativeModelPreparationStatus.Receipt.Producer) -> Bool {
        guard version == 1, status == "provisional-uncalibrated", assetSha256 == display.sha256,
              producer.source.bounds == producer.output.bounds,
              rawBounds.min.count == 3, rawBounds.max.count == 3,
              (rawBounds.min + rawBounds.max).allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }),
              rawBounds.min.map({ Double(Float($0)) }) == producer.source.bounds.min,
              rawBounds.max.map({ Double(Float($0)) }) == producer.source.bounds.max,
              zip(rawBounds.min, rawBounds.max).allSatisfy({ $0.1 > $0.0 && $0.1 - $0.0 <= 10_000 }),
              referenceHeightMeters.isFinite, referenceHeightMeters > 0, referenceHeightMeters <= 10_000, pose.position == [0, 0, 0], pose.rotation == [0, 0, 0, 1],
              pose.scale.count == 3 else { return false }
        let scale = referenceHeightMeters / (rawBounds.max[1] - rawBounds.min[1])
        let tolerance = max(1e-12, abs(scale) * 1e-9)
        return pose.scale.allSatisfy { (0.000_001...100).contains($0) && abs($0 - scale) <= tolerance }
    }
}
