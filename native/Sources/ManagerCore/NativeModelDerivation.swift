// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Public worker recipe identity, never a private MacOutput dependency or an
/// authority/custody grant. Enable only after an immutable runtime is qualified.
public enum NativeModelDerivationProfile {
    public static let id = "exhibitos-near-texture-pair-v1"
    public static let settingsBytes = Data(#"{"allowSpatialSampling":true,"profileId":"exhibitos-near-texture-pair-v1"}"#.utf8)
    public static let settingsSHA256 = "da9039b67435883255f723fd3b37b0fc395ba2acce3ab26706aeaa17d90f73bf"
    public static let tool = "ExhibitOS validation-worker"
    public static let toolVersion = "public-near-texture-v1"
    public static let qualifiedManifestSHA256: String? = "b2ce49e9703cadf7d22b73ec5dfd93622489a07c34721f9bbe2b9f6df17ee28e"
    public static func supports(manifestSHA256: String?, ready: Bool) -> Bool {
        guard ready, let qualifiedManifestSHA256 else { return false }
        // The floor-interior/orbit packet preserves the exact schema-022
        // public worker recipe and API contract; retain the previous packet.
        return manifestSHA256 == qualifiedManifestSHA256
            || manifestSHA256 == "7e823d9372aba3613fb8794f981c0f01cc97a420e3e82c388a783439bc933556"
    }
}

public struct NativeModelDerivationIntent: Codable, Equatable, Sendable {
    public let version: Int
    public let profileId: String
    public let settingsSha256: String
    public let toolVersion: String
    public let operationID: String
    public let generation: String
    public let artworkRevision: Int
    public init(operationID: String, generation: String, artworkRevision: Int) {
        version = 1; profileId = NativeModelDerivationProfile.id
        settingsSha256 = NativeModelDerivationProfile.settingsSHA256
        toolVersion = NativeModelDerivationProfile.toolVersion
        self.operationID = operationID; self.generation = generation; self.artworkRevision = artworkRevision
    }
    public var valid: Bool {
        version == 1 && profileId == NativeModelDerivationProfile.id
            && settingsSha256 == NativeModelDerivationProfile.settingsSHA256 && toolVersion == NativeModelDerivationProfile.toolVersion
            && UUID(uuidString: operationID) != nil && UUID(uuidString: generation) != nil && artworkRevision > 0
    }
    /// Cache identity alone grants neither reuse nor admission. Current server
    /// binding and the complete registered output inventory must also match.
    public func cacheIdentity(sourceSHA256: String) throws -> String {
        guard valid, nativeImportDigest(sourceSHA256) else { throw AuthoringFailure("IMPORT_MODEL_BINDING_INVALID") }
        let value: DocumentValue = .object(["sourceSHA256": .string(sourceSHA256), "profileId": .string(profileId),
            "settingsSHA256": .string(settingsSha256), "tool": .string(NativeModelDerivationProfile.tool), "toolVersion": .string(toolVersion)])
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return nativeImportSHA256(try encoder.encode(value))
    }
}

public struct NativeModelDerivationOutput: Decodable, Equatable, Sendable {
    public let assetId: String
    public let sha256: String
    public let bytes: Int
    public let reusedExactBytes: Bool?
    public let triangles: Int?
    public let textureSize: Int?
    var valid: Bool { UUID(uuidString: assetId) != nil && nativeImportDigest(sha256) && (1...33_554_432).contains(bytes) }
}

public struct NativeModelDerivationStatus: Decodable, Equatable, Sendable {
    public let operationID: String
    public let generation: String
    public let profileId: String
    public let status: String
    public let workerDrained: Bool
    public let fullAssetId: String?
    public let reducedAssetId: String?
    public let errorCode: String?
    public let full: NativeModelDerivationOutput?
    public let reduced: NativeModelDerivationOutput?

    public func matches(_ intent: NativeImportIntent, job: NativeImportJob) -> Bool {
        guard let binding = intent.modelDerivation, binding.valid,
              operationID == binding.operationID, generation == binding.generation, profileId == binding.profileId,
              ["pending", "ready", "failed"].contains(status),
              errorCode.map({ $0.range(of: "^[A-Z0-9_]{1,80}$", options: .regularExpression) != nil }) ?? true,
              fullAssetId.map({ UUID(uuidString: $0) != nil }) ?? true,
              reducedAssetId.map({ UUID(uuidString: $0) != nil }) ?? true else { return false }
        if let full {
            guard full.valid, full.assetId == fullAssetId, full.sha256 == intent.sha256, full.bytes == intent.bytes else { return false }
        }
        if job.state == "approved" {
            guard let full, full.reusedExactBytes == true, job.assetId == full.assetId else { return false }
        }
        if status == "ready" {
            guard job.state == "approved", workerDrained, errorCode == nil,
                  let full, let reduced, reduced.valid, full.reusedExactBytes == true,
                  job.assetId == full.assetId, reducedAssetId == reduced.assetId, full.assetId != reduced.assetId,
                  reduced.bytes < full.bytes, let triangles = full.triangles, triangles > 0, reduced.triangles == triangles,
                  let fullSize = full.textureSize, (1024...2048).contains(fullSize),
                  let reducedSize = reduced.textureSize, [256, 512].contains(reducedSize), reducedSize < fullSize else { return false }
        } else if status == "failed" {
            guard reduced == nil, reducedAssetId == nil else { return false }
        }
        return true
    }

    public func currentOutputs(in artwork: NativeArtwork, intent: NativeImportIntent, job: NativeImportJob) -> Bool {
        guard matches(intent, job: job), status == "ready", workerDrained, artwork.id == intent.artworkId,
              !artwork.archived, artwork.revision == intent.modelDerivation?.artworkRevision,
              artwork.metadata?.rights == intent.rights, let full, let reduced else { return false }
        return [full, reduced].allSatisfy { output in
            artwork.assets.contains { $0.id == output.assetId && $0.state == "approved" && $0.mime == "model/gltf-binary"
                && $0.sha256 == output.sha256 && $0.bytes == Int64(output.bytes) }
        }
    }
}
