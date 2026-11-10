// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum WorkspaceFurnishingKind: String, Codable, CaseIterable, Sendable {
    case pedestal, bench, stool, partition
    case infoStand = "info-stand"
    case barrier
    case displayCase = "display-case"
    case artworkFrame = "artwork-frame"
    case portalFinish = "portal-finish"

    var minimumExtensionVersion: Int64 {
        switch self {
        case .pedestal, .bench: 1
        case .stool: 2
        case .partition, .infoStand, .barrier, .displayCase: 3
        case .artworkFrame: 4
        case .portalFinish: 6
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

public enum WorkspaceFurnishingPreparationPolicy: String, Codable, Sendable {
    case omitUnusedInvalidTangent = "omit-unused-invalid-tangent-r1"
    case normalizeLegacySignTexture = "normalize-legacy-sign-texture-r1"

    static let profile = "workspace-artwork-frame-preparation-v1"
    static let infoStandProfile = "workspace-info-stand-preparation-v1"
    static let infoStandSourceByteLimit = 4 * 1024 * 1024

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

    func renderable(for source: WorkspaceFurnishingModel) -> WorkspaceFurnishingModel? {
        switch self {
        case .omitUnusedInvalidTangent: Self.models.first { $0.source == source }?.renderable
        case .normalizeLegacySignTexture: nil
        }
    }
}

public typealias WorkspaceArtworkFramePreparationPolicy = WorkspaceFurnishingPreparationPolicy

struct WorkspaceRegisteredFrameLineage: Codable, Sendable {
    static let profile = "registered-frame-original-lineage-v1"
    static let originalByteLimit = 177_500
    enum Phase: String, Codable, Sendable {
        case notStarted = "not-started", awaitingOriginal = "awaiting-original", readyToPrepare = "ready-to-prepare", registered
    }
    struct Receipt: Codable, Sendable {
        let workspaceId: String, intakeId: String
        let recipe: WorkspaceArtworkFramePreparationPolicy
        let sourceSHA256: String, renderableSHA256: String
    }
    struct Begin: Encodable {
        let requestId: String, furnishingId: String, furnishingVersion: Int
        let sourceModel: WorkspaceFurnishingModel
        let recipe: WorkspaceArtworkFramePreparationPolicy
    }

    let version: Int, kind: String
    let workspaceId: String, tenantId: String, principalId: String
    let intakeId: String, requestId: String, furnishingId: String, furnishingVersion: Int
    let recipe: WorkspaceArtworkFramePreparationPolicy, phase: Phase
    let sourceModel: WorkspaceFurnishingModel, renderableModel: WorkspaceFurnishingModel
    let historicalRequestSHA256: String, descriptorSHA256: String
    let receipt: Receipt?, workerDrained: Bool?

    var begin: Begin {
        Begin(requestId: requestId, furnishingId: furnishingId, furnishingVersion: furnishingVersion,
              sourceModel: sourceModel, recipe: recipe)
    }

    func check(history: WorkspaceFurnishingIntake, workspace: String, tenant: String, principal: String,
               previous: Self? = nil) throws {
        guard version == 1, kind == Self.profile, workspaceFurnishingID(workspace), workspaceId == workspace,
              tenantId == tenant, principalId == principal, intakeId == history.intakeId, requestId == history.requestId,
              history.phase == .registered, history.preparationPolicy == nil, history.declaration.kind == .artworkFrame,
              history.furnishing?.id == furnishingId, history.furnishing?.version == furnishingVersion,
              furnishingVersion == 1, history.declaration.model == renderableModel,
              recipe.renderable(for: sourceModel) == renderableModel,
              nativeImportDigest(historicalRequestSHA256), nativeImportDigest(descriptorSHA256),
              previous.map({ $0.historicalRequestSHA256 == historicalRequestSHA256
                  && $0.descriptorSHA256 == descriptorSHA256 && $0.sourceModel == sourceModel }) ?? true else {
            throw AuthoringFailure("FRAME_LINEAGE_RESPONSE_INVALID")
        }
        if phase == .registered {
            guard workerDrained == true, let receipt, receipt.workspaceId == workspaceId,
                  receipt.intakeId == intakeId, receipt.recipe == recipe,
                  receipt.sourceSHA256 == sourceModel.sha256, receipt.renderableSHA256 == renderableModel.sha256 else {
                throw AuthoringFailure("FRAME_LINEAGE_RESPONSE_INVALID")
            }
        } else if receipt != nil || workerDrained != nil {
            throw AuthoringFailure("FRAME_LINEAGE_RESPONSE_INVALID")
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

    var canRequestInfoStandPreparation: Bool {
        kind == .infoStand && rights.holder == "ExhibitOS" && rights.ownership == "owner" && rights.licenseId == "CC0-1.0"
            && (28...WorkspaceFurnishingPreparationPolicy.infoStandSourceByteLimit).contains(model.bytes)
    }
}

public enum WorkspaceFurnishingFitProfile: String, Codable, Sendable {
    case straightRing = "straight-ring-v1"
    case continuousPortal = "continuous-portal-v1"
    case modularPortal = "modular-portal-v1"

    func matches(_ kind: WorkspaceFurnishingKind) -> Bool {
        kind == (self == .straightRing ? .artworkFrame : .portalFinish)
    }
}

public struct WorkspaceFurnishingFit: Codable, Equatable, Sendable {
    public let version: Int
    public let sourceBytes: Int
    public let sourceSha256: String
    public let geometry: DocumentValue

    var profile: WorkspaceFurnishingFitProfile? {
        geometry["profile"].string.flatMap(WorkspaceFurnishingFitProfile.init(rawValue:))
    }

    func matches(_ details: WorkspaceFurnishingDetails) -> Bool {
        version == 1 && sourceBytes == details.model.bytes && sourceSha256 == details.model.sha256
            && profile?.matches(details.kind) == true
    }
}

struct WorkspaceFurnishingRequest: Encodable {
    let requestId: String
    let details: WorkspaceFurnishingDetails
    var preparationPolicy: WorkspaceFurnishingPreparationPolicy? = nil
    var fitProfile: WorkspaceFurnishingFitProfile? = nil

    func encode(to encoder: Encoder) throws {
        try details.encode(to: encoder)
        var fields = encoder.container(keyedBy: CodingKeys.self)
        try fields.encode(requestId, forKey: .requestId)
        try fields.encodeIfPresent(preparationPolicy, forKey: .preparationPolicy)
        try fields.encodeIfPresent(fitProfile, forKey: .fitProfile)
    }
    private enum CodingKeys: CodingKey { case requestId, preparationPolicy, fitProfile }
}

public struct WorkspaceFurnishing: Codable, Equatable, Sendable {
    public let id: String
    public let version: Int
    public let details: WorkspaceFurnishingDetails
    public let fit: WorkspaceFurnishingFit?

    public init(from decoder: Decoder) throws {
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        id = try fields.decode(String.self, forKey: .id)
        version = try fields.decode(Int.self, forKey: .version)
        details = try WorkspaceFurnishingDetails(from: decoder)
        fit = fields.contains(.fit) ? try fields.decode(WorkspaceFurnishingFit.self, forKey: .fit) : nil
        guard workspaceFurnishingID(id), version == 1, details.isValid,
              fit.map({ $0.matches(details) }) ?? true else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
    }

    public func encode(to encoder: Encoder) throws {
        try details.encode(to: encoder)
        var fields = encoder.container(keyedBy: CodingKeys.self)
        try fields.encode(id, forKey: .id)
        try fields.encode(version, forKey: .version)
        try fields.encodeIfPresent(fit, forKey: .fit)
    }
    private enum CodingKeys: CodingKey { case id, version, fit }
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
        public let recipe: WorkspaceFurnishingPreparationPolicy
        public let renderableModel: WorkspaceFurnishingModel
        public let runtimeManifestSHA256: String?

        private enum CodingKeys: String, CodingKey { case sourceModel, recipe, renderableModel, runtimeManifestSHA256 }

        public init(from decoder: Decoder) throws {
            let fields = try decoder.container(keyedBy: CodingKeys.self)
            recipe = try fields.decode(WorkspaceFurnishingPreparationPolicy.self, forKey: .recipe)
            switch recipe {
            case .omitUnusedInvalidTangent:
                try Self.requireKeys(decoder, ["sourceModel", "recipe", "renderableModel"])
                runtimeManifestSHA256 = nil
            case .normalizeLegacySignTexture:
                try Self.requireKeys(decoder, ["sourceModel", "recipe", "renderableModel", "runtimeManifestSHA256"])
                runtimeManifestSHA256 = try fields.decode(String.self, forKey: .runtimeManifestSHA256)
            }
            for key in [CodingKeys.sourceModel, .renderableModel] {
                try Self.requireKeys(fields.superDecoder(forKey: key), ["mime", "bytes", "sha256"])
            }
            sourceModel = try fields.decode(WorkspaceFurnishingModel.self, forKey: .sourceModel)
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
    public let preparationPolicy: WorkspaceFurnishingPreparationPolicy?
    public let preparation: Preparation?
    public let fitProfile: WorkspaceFurnishingFitProfile?

    private enum CodingKeys: CodingKey {
        case intakeId, requestId, phase, declaration, received, retryable, error, furnishing, preparationPolicy, preparation, fitProfile
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
            ? try fields.decode(WorkspaceFurnishingPreparationPolicy.self, forKey: .preparationPolicy) : nil
        preparation = fields.contains(.preparation) ? try fields.decode(Preparation.self, forKey: .preparation) : nil
        fitProfile = fields.contains(.fitProfile) ? try fields.decode(WorkspaceFurnishingFitProfile.self, forKey: .fitProfile) : nil
    }

    func check(id: String? = nil, requestId: String? = nil, details: WorkspaceFurnishingDetails? = nil,
               expectedPolicy: WorkspaceFurnishingPreparationPolicy? = nil, expectedFit: WorkspaceFurnishingFitProfile? = nil,
               supportedExtensionVersion: Int64 = 1,
               admitsPreparation: Bool = false, infoStandPreparationManifest: String? = nil) throws {
        guard workspaceFurnishingID(intakeId), workspaceFurnishingID(self.requestId),
              id.map({ $0 == intakeId }) ?? true, requestId.map({ $0 == self.requestId }) ?? true,
              declaration.isValid, declaration.kind.minimumExtensionVersion <= supportedExtensionVersion,
              details.map({ $0 == declaration && expectedPolicy == preparationPolicy && expectedFit == fitProfile }) ?? true,
              fitProfile.map({ supportedExtensionVersion >= 7 && preparationPolicy == nil && $0.matches(declaration.kind) }) ?? true,
              furnishing.map({ $0.fit?.profile == fitProfile }) ?? true,
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
        guard preparation == nil || phase == .registered else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        switch preparationPolicy {
        case .omitUnusedInvalidTangent:
            guard admitsPreparation, declaration.kind == .artworkFrame,
                  let renderable = preparationPolicy.renderable(for: declaration.model),
                  (phase == .registered) == (preparation != nil),
                  preparation.map({ $0.renderableModel == renderable }) ?? true else {
                throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
            }
        case .normalizeLegacySignTexture:
            guard let infoStandPreparationManifest, nativeImportDigest(infoStandPreparationManifest),
                  declaration.canRequestInfoStandPreparation,
                  preparation.map({ $0.runtimeManifestSHA256 == infoStandPreparationManifest
                      && $0.renderableModel.isValid && (28..<declaration.model.bytes).contains($0.renderableModel.bytes)
                      && $0.renderableModel.sha256 != declaration.model.sha256 }) ?? true else {
                throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
            }
        }
        if let preparation {
            let expected = WorkspaceFurnishingDetails(kind: declaration.kind, name: declaration.name,
                rights: declaration.rights, provenance: declaration.provenance, model: preparation.renderableModel)
            guard preparation.recipe == preparationPolicy, preparation.sourceModel == declaration.model,
                  furnishing?.details == expected else {
                throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
            }
        } else if let furnishing, furnishing.details != declaration {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
    }
}

public enum WorkspaceFurnishingIntakeAction: String, Sendable {
    case complete, cancel, retry
}
