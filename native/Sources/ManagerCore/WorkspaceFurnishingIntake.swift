// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum WorkspaceFurnishingKind: String, Codable, CaseIterable, Sendable {
    case pedestal, bench, stool, partition
    case infoStand = "info-stand"
    case barrier
    case displayCase = "display-case"
    case artworkFrame = "artwork-frame"

    var minimumExtensionVersion: Int64 {
        switch self {
        case .pedestal, .bench: 1
        case .stool: 2
        case .partition, .infoStand, .barrier, .displayCase: 3
        case .artworkFrame: 4
        }
    }
}

public struct WorkspaceFurnishingModel: Codable, Equatable, Sendable {
    public static let byteLimit = ImportFileSnapshot.limit
    public let mime: String
    public let bytes: Int
    public let sha256: String

    public init(snapshot: ImportFileSnapshot) throws {
        mime = snapshot.mime
        bytes = snapshot.data.count
        sha256 = snapshot.sha256
        guard isValid else { throw AuthoringFailure("FURNISHING_DECLARATION") }
    }

    var isValid: Bool {
        mime == "model/gltf-binary" && (1...Self.byteLimit).contains(bytes) && nativeImportDigest(sha256)
    }
}

func workspaceFurnishingID(_ value: String) -> Bool {
    value.range(of: "^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$",
                options: .regularExpression) != nil
}

private func furnishingText(_ value: String, limit: Int) -> Bool {
    (1...limit).contains(value.utf16.count) && value == value.trimmingCharacters(in: .whitespacesAndNewlines)
}

public struct WorkspaceFurnishingDetails: Codable, Equatable, Sendable {
    public let kind: WorkspaceFurnishingKind
    public let name: String
    public let rights: ArtworkMetadata.Rights
    public let provenance: String
    public let model: WorkspaceFurnishingModel

    public init(kind: WorkspaceFurnishingKind, name: String, rights: ArtworkMetadata.Rights,
                provenance: String, model: WorkspaceFurnishingModel) {
        self.kind = kind
        self.name = name
        self.rights = rights
        self.provenance = provenance
        self.model = model
    }

    var isValid: Bool { furnishingText(name, limit: 120) && furnishingText(provenance, limit: 1024) && model.isValid }
}

struct WorkspaceFurnishingRequest: Encodable {
    let requestId: String
    let details: WorkspaceFurnishingDetails

    func encode(to encoder: Encoder) throws {
        try details.encode(to: encoder)
        var fields = encoder.container(keyedBy: CodingKeys.self)
        try fields.encode(requestId, forKey: .requestId)
    }
    private enum CodingKeys: CodingKey { case requestId }
}

public struct WorkspaceFurnishing: Codable, Equatable, Sendable {
    public let id: String
    public let version: Int
    public let details: WorkspaceFurnishingDetails

    public init(from decoder: Decoder) throws {
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        id = try fields.decode(String.self, forKey: .id)
        version = try fields.decode(Int.self, forKey: .version)
        details = try WorkspaceFurnishingDetails(from: decoder)
        guard workspaceFurnishingID(id), version == 1, details.isValid else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
    }

    public func encode(to encoder: Encoder) throws {
        try details.encode(to: encoder)
        var fields = encoder.container(keyedBy: CodingKeys.self)
        try fields.encode(id, forKey: .id)
        try fields.encode(version, forKey: .version)
    }
    private enum CodingKeys: CodingKey { case id, version }
}

public struct WorkspaceFurnishingSummary: Codable, Sendable, Identifiable {
    public let id: String
    public let version: Int
    public let kind: WorkspaceFurnishingKind
    public let name: String
    public let rightsAvailability: WorkspaceMaterialSummary.Availability

    var isValid: Bool { workspaceFurnishingID(id) && version == 1 && furnishingText(name, limit: 120) }
}

public struct WorkspaceFurnishingIntake: Codable, Sendable, Identifiable {
    public enum Phase: String, Codable, Sendable { case uploading, validating, cancelling, registered, failed, cancelled }
    public struct Receipt: Codable, Sendable { public let bytes: Int, sha256: String }
    public struct Failure: Codable, Sendable { public let code: String }

    public var id: String { intakeId }
    public let intakeId: String
    public let requestId: String
    public let phase: Phase
    public let declaration: WorkspaceFurnishingDetails
    public let received: Receipt?
    public let retryable: Bool
    public let error: Failure?
    public let furnishing: WorkspaceFurnishing?

    func check(id: String? = nil, requestId: String? = nil, details: WorkspaceFurnishingDetails? = nil,
               supportedExtensionVersion: Int64 = 1) throws {
        guard workspaceFurnishingID(intakeId), workspaceFurnishingID(self.requestId),
              id.map({ $0 == intakeId }) ?? true, requestId.map({ $0 == self.requestId }) ?? true,
              declaration.isValid, declaration.kind.minimumExtensionVersion <= supportedExtensionVersion,
              details.map({ $0 == declaration }) ?? true,
              received.map({ $0.bytes == declaration.model.bytes && $0.sha256 == declaration.model.sha256 }) ?? true,
              !retryable || (phase == .failed && received != nil),
              phase != .validating || received != nil,
              (phase == .registered) == (furnishing != nil),
              furnishing.map({ received != nil && $0.details == declaration }) ?? true,
              error.map({ AuthoringFailure.safeCode($0.code) }) ?? true else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
    }
}

public enum WorkspaceFurnishingIntakeAction: String, Sendable {
    case complete, cancel, retry
}
