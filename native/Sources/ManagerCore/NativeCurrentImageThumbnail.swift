// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit

private func thumbnailInvalid() -> AuthoringFailure { AuthoringFailure("IMAGE_THUMBNAIL_STATE_INVALID") }
private func thumbnailKeys(_ value: DocumentValue, _ required: Set<String>, optional: Set<String> = []) throws {
    guard case .object(let fields) = value, required.isSubset(of: Set(fields.keys)),
          Set(fields.keys).isSubset(of: required.union(optional)) else { throw thumbnailInvalid() }
}
private func thumbnailHash<T: Encodable>(_ value: T) throws -> String {
    let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    return try SHA256.hash(data: encoder.encode(value)).map { String(format: "%02x", $0) }.joined()
}
private func thumbnailID(_ value: String) -> Bool {
    guard let uuid = UUID(uuidString: value) else { return false }
    return uuid.uuidString.lowercased() == value && value.range(
        of: #"^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"#,
        options: .regularExpression) != nil
}
private func thumbnailDigest(_ value: String) -> Bool {
    value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
}
private func thumbnailScaled(width: Int, height: Int, edge: Int) -> (Int, Int) {
    let scale = min(1, Double(edge) / Double(max(width, height)))
    return (max(1, Int((Double(width) * scale).rounded())), max(1, Int((Double(height) * scale).rounded())))
}

/// Closed creation body; no asset data, file name, path or hash is accepted.
public struct NativeCurrentImageThumbnailRequest: Codable, Equatable, Sendable {
    public let requestId: String
    public let expectedRevision: Int
    public init(requestId: String, expectedRevision: Int) throws {
        guard thumbnailID(requestId), (1...2_147_483_647).contains(expectedRevision) else { throw thumbnailInvalid() }
        self.requestId = requestId; self.expectedRevision = expectedRevision
    }
    public init(from decoder: Decoder) throws {
        let wire = try DocumentValue(from: decoder)
        try thumbnailKeys(wire, ["requestId", "expectedRevision"])
        guard let id = wire["requestId"].string, let revision = wire["expectedRevision"].integer,
              let value = Int(exactly: revision) else { throw thumbnailInvalid() }
        try self.init(requestId: id, expectedRevision: value)
    }
}

public struct NativeCurrentImageThumbnailJob: Codable, Sendable {
    public static let profile = "current-approved-image-thumbnail-256-v1"
    public struct Source: Codable, Equatable, Sendable {
        public let assetId: String, sha256: String, mime: String
        public let bytes: Int
        var valid: Bool { thumbnailID(assetId) && thumbnailDigest(sha256) && mime == "image/png" && (33...32 * 1024 * 1024).contains(bytes) }
    }
    public struct Image: Codable, Equatable, Sendable {
        public let assetId: String, sha256: String, mime: String
        public let bytes: Int, width: Int, height: Int
        var valid: Bool { thumbnailID(assetId) && thumbnailDigest(sha256) && mime == "image/png"
            && (33...512 * 1024).contains(bytes) && (1...256).contains(width) && (1...256).contains(height) }
    }
    public struct Metric: Codable, Equatable, Sendable {
        public let sha256: String
        public let bytes: Int, width: Int, height: Int
        func valid(edge: Int) -> Bool { thumbnailDigest(sha256) && (1...8 * 1024 * 1024).contains(bytes)
            && (1...edge).contains(width) && (1...edge).contains(height) }
    }
    public struct ProducerSource: Codable, Equatable, Sendable {
        public let mime: String, sha256: String
        public let bytes: Int, width: Int, height: Int
        public let iccSHA256: String?
        private enum CodingKeys: String, CodingKey { case mime,sha256,bytes,width,height,iccSHA256 }
        public func encode(to encoder: Encoder) throws {
            var c = encoder.container(keyedBy: CodingKeys.self)
            try c.encode(mime, forKey:.mime); try c.encode(sha256, forKey:.sha256); try c.encode(bytes, forKey:.bytes)
            try c.encode(width, forKey:.width); try c.encode(height, forKey:.height); try c.encode(iccSHA256, forKey:.iccSHA256)
        }
    }
    public struct Producer: Codable, Equatable, Sendable {
        public let version: Int
        public let recipeSHA256: String, decoder: String
        public let source: ProducerSource
        public let full: Metric
        public let coarse: Metric?
    }
    public struct Receipt: Codable, Sendable {
        public let version: Int, artworkRevision: Int
        public let profile: String, jobId: String, artworkId: String, workspaceId: String, principalId: String, rightsSHA256: String
        public let source: Source, thumbnail: Image, producer: Producer
        public let sha256: String
        public init(from decoder: Decoder) throws {
            let wire = try DocumentValue(from: decoder)
            try thumbnailKeys(wire, ["version","profile","jobId","artworkId","artworkRevision","workspaceId","principalId","rightsSHA256","source","thumbnail","producer","sha256"])
            try thumbnailKeys(wire["source"], ["assetId","sha256","bytes","mime"])
            try thumbnailKeys(wire["thumbnail"], ["assetId","sha256","bytes","mime","width","height"])
            try thumbnailKeys(wire["producer"], ["version","recipeSHA256","decoder","source","full"], optional: ["coarse"])
            try thumbnailKeys(wire["producer"]["source"], ["mime","sha256","bytes","width","height","iccSHA256"])
            try thumbnailKeys(wire["producer"]["full"], ["sha256","bytes","width","height"])
            if case .object(let producer) = wire["producer"], let coarse = producer["coarse"] {
                try thumbnailKeys(coarse, ["sha256","bytes","width","height"])
            }
            enum Keys: String, CodingKey { case version,profile,jobId,artworkId,artworkRevision,workspaceId,principalId,rightsSHA256,source,thumbnail,producer,sha256 }
            let c = try decoder.container(keyedBy: Keys.self)
            version = try c.decode(Int.self, forKey: .version); profile = try c.decode(String.self, forKey: .profile)
            jobId = try c.decode(String.self, forKey: .jobId); artworkId = try c.decode(String.self, forKey: .artworkId)
            artworkRevision = try c.decode(Int.self, forKey: .artworkRevision); workspaceId = try c.decode(String.self, forKey: .workspaceId)
            principalId = try c.decode(String.self, forKey: .principalId); rightsSHA256 = try c.decode(String.self, forKey: .rightsSHA256)
            source = try c.decode(Source.self, forKey: .source); thumbnail = try c.decode(Image.self, forKey: .thumbnail)
            producer = try c.decode(Producer.self, forKey: .producer); sha256 = try c.decode(String.self, forKey: .sha256)
            guard case .object(var body) = wire else { throw thumbnailInvalid() }
            body.removeValue(forKey: "sha256")
            guard thumbnailDigest(sha256), try thumbnailHash(DocumentValue.object(body)) == sha256 else { throw thumbnailInvalid() }
        }
    }
    /// This decoded value is not an operator review or selection authority.
    public struct SelectionCandidate: Codable, Equatable, Sendable {
        public struct View: Codable, Equatable, Sendable { public let kind: String }
        public let expectedRevision: Int
        public let sourceAssetId: String, sourceSha256: String, thumbnailAssetId: String, thumbnailSha256: String
        public let view: View
    }
    public struct Preparation: Codable, Sendable {
        public enum Phase: String, Codable, Sendable { case pending, failed, ready }
        public let profile: String, artworkRevision: Int
        public let source: Source
        public let status: Phase
        public let workerDrained: Bool
        public let serverReceipt: Receipt?
        public let selectionCandidate: SelectionCandidate?
        public init(from decoder: Decoder) throws {
            let wire = try DocumentValue(from: decoder)
            let ready = wire["status"].string == "ready"
            let keys: Set<String> = ["profile","artworkRevision","source","status","workerDrained"]
            try thumbnailKeys(wire, ready ? keys.union(["serverReceipt","selectionCandidate"]) : keys)
            try thumbnailKeys(wire["source"], ["assetId","sha256","bytes","mime"])
            if ready {
                try thumbnailKeys(wire["selectionCandidate"], ["expectedRevision","sourceAssetId","sourceSha256","thumbnailAssetId","thumbnailSha256","view"])
                try thumbnailKeys(wire["selectionCandidate"]["view"], ["kind"])
            }
            enum Keys: String, CodingKey { case profile,artworkRevision,source,status,workerDrained,serverReceipt,selectionCandidate }
            let c = try decoder.container(keyedBy: Keys.self)
            profile = try c.decode(String.self, forKey: .profile); artworkRevision = try c.decode(Int.self, forKey: .artworkRevision)
            source = try c.decode(Source.self, forKey: .source); status = try c.decode(Phase.self, forKey: .status)
            workerDrained = try c.decode(Bool.self, forKey: .workerDrained)
            serverReceipt = try c.decodeIfPresent(Receipt.self, forKey: .serverReceipt)
            selectionCandidate = try c.decodeIfPresent(SelectionCandidate.self, forKey: .selectionCandidate)
            guard profile == NativeCurrentImageThumbnailJob.profile, (1...2_147_483_647).contains(artworkRevision), source.valid,
                  ready == (serverReceipt != nil && selectionCandidate != nil), !ready || workerDrained else { throw thumbnailInvalid() }
        }
    }

    public let id: String, state: String
    public let progress: Int, attempts: Int
    public let errorCode: String?, assetId: String?, retryAt: String?
    public let thumbnailPreparation: Preparation
    public init(from decoder: Decoder) throws {
        let wire = try DocumentValue(from: decoder)
        try thumbnailKeys(wire, ["id","state","progress","attempts","errorCode","retryAt","thumbnailPreparation"],
                          optional: ["assetId","originalAssetId","workerDrainVersion","workerDrained"])
        enum Keys: String, CodingKey { case id,state,progress,attempts,errorCode,assetId,retryAt,thumbnailPreparation,originalAssetId,workerDrainVersion,workerDrained }
        let c = try decoder.container(keyedBy: Keys.self)
        id = try c.decode(String.self, forKey: .id); state = try c.decode(String.self, forKey: .state)
        progress = try c.decode(Int.self, forKey: .progress); attempts = try c.decode(Int.self, forKey: .attempts)
        errorCode = try c.decodeIfPresent(String.self, forKey: .errorCode); assetId = try c.decodeIfPresent(String.self, forKey: .assetId)
        retryAt = try c.decodeIfPresent(String.self, forKey: .retryAt)
        thumbnailPreparation = try c.decode(Preparation.self, forKey: .thumbnailPreparation)
        let drainVersion = try c.decodeIfPresent(Int.self, forKey: .workerDrainVersion)
        let outerDrained = try c.decodeIfPresent(Bool.self, forKey: .workerDrained)
        guard thumbnailID(id), (0...100).contains(progress), (0...5).contains(attempts),
              ["queued","processing","approved","failed","cancelled"].contains(state),
              (errorCode == nil || AuthoringFailure.safeCode(errorCode!)),
              wire["originalAssetId"] == .null,
              (!c.contains(.workerDrainVersion) || drainVersion == 1),
              (!c.contains(.workerDrained) || outerDrained == thumbnailPreparation.workerDrained) else { throw thumbnailInvalid() }
        switch thumbnailPreparation.status {
        case .ready:
            guard state == "approved", progress == 100, errorCode == nil,
                  assetId == thumbnailPreparation.serverReceipt?.thumbnail.assetId else { throw thumbnailInvalid() }
        case .failed:
            guard ["failed","cancelled"].contains(state), assetId == nil else { throw thumbnailInvalid() }
        case .pending:
            guard ["queued","processing","approved"].contains(state), assetId == nil,
                  !thumbnailPreparation.workerDrained else { throw thumbnailInvalid() }
        }
    }
    public func encode(to encoder: Encoder) throws {
        enum Keys: String, CodingKey { case id,state,progress,attempts,errorCode,assetId,retryAt,thumbnailPreparation }
        var c = encoder.container(keyedBy: Keys.self)
        try c.encode(id, forKey:.id); try c.encode(state, forKey:.state)
        try c.encode(progress, forKey:.progress); try c.encode(attempts, forKey:.attempts)
        try c.encode(errorCode, forKey:.errorCode); try c.encodeIfPresent(assetId, forKey:.assetId)
        try c.encode(retryAt, forKey:.retryAt); try c.encode(thumbnailPreparation, forKey:.thumbnailPreparation)
    }

    /// Must use a freshly read approved artwork, not a previously cached observation.
    public func check(artwork: NativeArtwork, workspaceId: String, principalId: String, jobId: String) throws {
        let p = thumbnailPreparation
        guard id == jobId, [id, artwork.id, workspaceId, principalId].allSatisfy(thumbnailID),
              !artwork.archived, artwork.revision == p.artworkRevision, artwork.approvedRevision == artwork.revision,
              artwork.approvedAssetId == p.source.assetId, let rights = artwork.metadata?.rights,
              rights.permissions.display, rights.permissions.export,
              artwork.assets.filter({ $0.id == p.source.assetId }).count == 1,
              artwork.assets.filter({ $0.id == p.source.assetId && $0.state == "approved" && $0.mime == "image/png"
                  && $0.sha256 == p.source.sha256 && $0.bytes == Int64(p.source.bytes) }).count == 1 else { throw thumbnailInvalid() }
        guard p.status == .ready else { return }
        guard let r = p.serverReceipt, let selection = p.selectionCandidate,
              r.version == 1, r.profile == Self.profile, r.jobId == id, r.artworkId == artwork.id,
              r.artworkRevision == artwork.revision, r.workspaceId == workspaceId, r.principalId == principalId,
              thumbnailDigest(r.rightsSHA256), try thumbnailHash(rights) == r.rightsSHA256,
              r.source == p.source, r.thumbnail.valid, r.thumbnail.assetId != p.source.assetId,
              selection.expectedRevision == artwork.revision, selection.sourceAssetId == p.source.assetId,
              selection.sourceSha256 == p.source.sha256, selection.thumbnailAssetId == r.thumbnail.assetId,
              selection.thumbnailSha256 == r.thumbnail.sha256, selection.view.kind == "default" else { throw thumbnailInvalid() }
        let producer = r.producer, source = producer.source
        guard producer.version == 1, ["d13ad48b1ae8c29818002225fb5ebb4840907646810d90922a4ea3a5448f06c0",
               "912cac7efe052040dc9aef7c3e21574e48db79965640e8e40b65cd51b839fa12"].contains(producer.recipeSHA256),
              producer.decoder == "sharp-0.35.4-vips-8.18.6-mozjpeg-0826579", source.mime == p.source.mime,
              source.sha256 == p.source.sha256, source.bytes == p.source.bytes, source.iccSHA256 == nil,
              (1...8192).contains(source.width), (1...8192).contains(source.height), source.width * source.height <= 24 * 1024 * 1024,
              producer.full.valid(edge: 2048),
              [2048,1600].contains(where: { thumbnailScaled(width: source.width, height: source.height, edge: $0) == (producer.full.width, producer.full.height) }),
              thumbnailScaled(width: producer.full.width, height: producer.full.height, edge: 256) == (r.thumbnail.width, r.thumbnail.height)
        else { throw thumbnailInvalid() }
        if let coarse = producer.coarse {
            guard coarse.valid(edge: 512), max(producer.full.width, producer.full.height) > 512,
                  coarse.bytes < producer.full.bytes,
                  thumbnailScaled(width: producer.full.width, height: producer.full.height, edge: 512) == (coarse.width, coarse.height)
            else { throw thumbnailInvalid() }
        }
    }
}
