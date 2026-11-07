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

    fileprivate init(bytes: Int, sha256: String) {
        mime = "model/gltf-binary"
        self.bytes = bytes
        self.sha256 = sha256
    }

    var isValid: Bool {
        mime == "model/gltf-binary" && (1...Self.byteLimit).contains(bytes) && nativeImportDigest(sha256)
    }
}

public enum WorkspaceArtworkFramePreparationPolicy: String, Codable, Sendable {
    case omitUnusedInvalidTangent = "omit-unused-invalid-tangent-r1"

    static let profile = "workspace-artwork-frame-preparation-v1"

    // The three reviewed source → v2 model pairs in ExhibitOS Spec draft.24.
    private static let models: [(source: WorkspaceFurnishingModel, renderable: WorkspaceFurnishingModel)] = [
        (.init(bytes: 101_768, sha256: "17c571accc4ed2554731d4ce89f3cad4faa66cf2bca83321ecd92ad2537093d9"),
         .init(bytes: 101_720, sha256: "2fb941a9f6ed030638512e917e348a0a3000e29c1133fe3f9670268ff1c4169c")),
        (.init(bytes: 147_640, sha256: "b21ee369bf9e2d958d9ff2c03cf2457bbd22669c7d1621c379bebcb6330a8c89"),
         .init(bytes: 147_592, sha256: "a0d0c0d3f02fea91b6a09273eec8d200248a16c8caf5139bd029b681af8eddcd")),
        (.init(bytes: 177_500, sha256: "06ac605194e7aa870c4c9c68c17e00b6d1256c421aba6ed9b169e7f873b7b8d9"),
         .init(bytes: 177_476, sha256: "5af789b1aa8e6aa410b40cf16e27c9e2000f87438f1fa52cb08bc46c19c75778")),
    ]

    public static func matching(_ model: WorkspaceFurnishingModel) -> Self? {
        let policy = Self.omitUnusedInvalidTangent
        return policy.renderable(for: model) == nil ? nil : policy
    }

    fileprivate func renderable(for source: WorkspaceFurnishingModel) -> WorkspaceFurnishingModel? {
        switch self {
        case .omitUnusedInvalidTangent: Self.models.first { $0.source == source }?.renderable
        }
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
    var preparationPolicy: WorkspaceArtworkFramePreparationPolicy? = nil

    func encode(to encoder: Encoder) throws {
        try details.encode(to: encoder)
        var fields = encoder.container(keyedBy: CodingKeys.self)
        try fields.encode(requestId, forKey: .requestId)
        try fields.encodeIfPresent(preparationPolicy, forKey: .preparationPolicy)
    }
    private enum CodingKeys: CodingKey { case requestId, preparationPolicy }
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
    public struct Preparation: Codable, Equatable, Sendable {
        public let sourceModel: WorkspaceFurnishingModel
        public let recipe: WorkspaceArtworkFramePreparationPolicy
        public let renderableModel: WorkspaceFurnishingModel

        private enum CodingKeys: String, CodingKey { case sourceModel, recipe, renderableModel }

        public init(from decoder: Decoder) throws {
            try Self.requireKeys(decoder, ["sourceModel", "recipe", "renderableModel"])
            let fields = try decoder.container(keyedBy: CodingKeys.self)
            for key in [CodingKeys.sourceModel, .renderableModel] {
                try Self.requireKeys(fields.superDecoder(forKey: key), ["mime", "bytes", "sha256"])
            }
            sourceModel = try fields.decode(WorkspaceFurnishingModel.self, forKey: .sourceModel)
            recipe = try fields.decode(WorkspaceArtworkFramePreparationPolicy.self, forKey: .recipe)
            renderableModel = try fields.decode(WorkspaceFurnishingModel.self, forKey: .renderableModel)
        }

        private struct WireKey: CodingKey {
            let stringValue: String
            let intValue: Int? = nil
            init?(stringValue: String) { self.stringValue = stringValue }
            init?(intValue: Int) { return nil }
        }

        private static func requireKeys(_ decoder: Decoder, _ expected: Set<String>) throws {
            let fields = try decoder.container(keyedBy: WireKey.self)
            guard Set(fields.allKeys.map(\.stringValue)) == expected else {
                throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
            }
        }
    }

    public var id: String { intakeId }
    public let intakeId: String
    public let requestId: String
    public let phase: Phase
    public let declaration: WorkspaceFurnishingDetails
    public let received: Receipt?
    public let retryable: Bool
    public let error: Failure?
    public let furnishing: WorkspaceFurnishing?
    public let preparationPolicy: WorkspaceArtworkFramePreparationPolicy?
    public let preparation: Preparation?

    private enum CodingKeys: CodingKey {
        case intakeId, requestId, phase, declaration, received, retryable, error, furnishing, preparationPolicy, preparation
    }

    public init(from decoder: Decoder) throws {
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        intakeId = try fields.decode(String.self, forKey: .intakeId)
        requestId = try fields.decode(String.self, forKey: .requestId)
        phase = try fields.decode(Phase.self, forKey: .phase)
        declaration = try fields.decode(WorkspaceFurnishingDetails.self, forKey: .declaration)
        received = try fields.decodeIfPresent(Receipt.self, forKey: .received)
        retryable = try fields.decode(Bool.self, forKey: .retryable)
        error = try fields.decodeIfPresent(Failure.self, forKey: .error)
        furnishing = try fields.decodeIfPresent(WorkspaceFurnishing.self, forKey: .furnishing)
        preparationPolicy = fields.contains(.preparationPolicy)
            ? try fields.decode(WorkspaceArtworkFramePreparationPolicy.self, forKey: .preparationPolicy) : nil
        preparation = fields.contains(.preparation) ? try fields.decode(Preparation.self, forKey: .preparation) : nil
    }

    func check(id: String? = nil, requestId: String? = nil, details: WorkspaceFurnishingDetails? = nil,
               expectedPolicy: WorkspaceArtworkFramePreparationPolicy? = nil, supportedExtensionVersion: Int64 = 1,
               admitsPreparation: Bool = false) throws {
        guard workspaceFurnishingID(intakeId), workspaceFurnishingID(self.requestId),
              id.map({ $0 == intakeId }) ?? true, requestId.map({ $0 == self.requestId }) ?? true,
              declaration.isValid, declaration.kind.minimumExtensionVersion <= supportedExtensionVersion,
              details.map({ $0 == declaration && expectedPolicy == preparationPolicy }) ?? true,
              received.map({ $0.bytes == declaration.model.bytes && $0.sha256 == declaration.model.sha256 }) ?? true,
              !retryable || (phase == .failed && received != nil),
              phase != .validating || received != nil,
              (phase == .registered) == (furnishing != nil),
              furnishing == nil || received != nil,
              error.map({ AuthoringFailure.safeCode($0.code) }) ?? true else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        guard let preparationPolicy else {
            guard preparation == nil, furnishing.map({ $0.details == declaration }) ?? true else {
                throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
            }
            return
        }
        guard admitsPreparation, declaration.kind == .artworkFrame,
              let renderable = preparationPolicy.renderable(for: declaration.model),
              (phase == .registered) == (preparation != nil) else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        if let preparation {
            let expected = WorkspaceFurnishingDetails(kind: declaration.kind, name: declaration.name,
                rights: declaration.rights, provenance: declaration.provenance, model: renderable)
            guard preparation.recipe == preparationPolicy, preparation.sourceModel == declaration.model,
                  preparation.renderableModel == renderable, furnishing?.details == expected else {
                throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
            }
        }
    }
}

public enum WorkspaceFurnishingIntakeAction: String, Sendable {
    case complete, cancel, retry
}
