// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit

/// Exact runtime qualification for the existing original/derived JPEG custody
/// contract. This does not enable watermark or model-derivation features.
public enum NativeJPEGImportRuntime {
    public static let currentManifestSHA256 = "115be25154dc989c9bfe4c0b5a94ed23b7ed9930afc9695cc8198480bc0b5dfe"
    private static let qualifiedICCManifests: Set<String> = [
        "253de8963ec9f0e0a2884a27d8e756b453cf61899e38c31571d9993a7d6ecff4",
    ]
    // This exact development pair is used to qualify the existing ICC workflow.
    static func supportsPortalICCValidation(manifestSHA256: String?, ready: Bool) -> Bool {
        #if DEBUG && os(macOS)
        ready && manifestSHA256 == "0bea16fb7d61aa3b0e60921a7f4d8d702025e4857f779fd763b9633f4884b330"
        #else
        false
        #endif
    }
    public static func supportsICC(manifestSHA256: String?, ready: Bool) -> Bool {
        guard ready, let manifestSHA256 else { return false }
        return qualifiedICCManifests.contains(manifestSHA256)
            || supportsPortalICCValidation(manifestSHA256: manifestSHA256, ready: ready)
            || supportsSameOwnerArtworkReplacement(manifestSHA256: manifestSHA256, ready: ready)
    }
    static func supportsSameOwnerArtworkReplacement(manifestSHA256: String?, ready: Bool) -> Bool {
        #if DEBUG && os(macOS)
        ready && manifestSHA256 == "73a491c071ae5c6f10c786793733968549ab400333569d8427530b6e81606c9b"
        #else
        false
        #endif
    }
    public static func supports(manifestSHA256: String?, ready: Bool) -> Bool {
        guard ready else { return false }
        return supportsICC(manifestSHA256: manifestSHA256, ready: ready)
            || manifestSHA256 == currentManifestSHA256
            || manifestSHA256 == "74b08c7899d74ad989bbb61e47fc86fe9fa25c8853ab27ce57e9135c4932e96a"
            || PublicationWatermarkRuntime.supports(manifestSHA256: manifestSHA256, ready: ready)
    }
}

/// An intent to prepare the server-owned coarse member of a reviewed image pair.
public struct NativeCoarsePreparationIntent: Codable, Equatable, Sendable {
    public let version: Int
    public let profile: String
    public let approvalId: String
    public let artworkRevision: Int

    public init(approvalId: String, artworkRevision: Int) {
        version = 1
        profile = "viewer-png-box-512-v1"
        self.approvalId = approvalId
        self.artworkRevision = artworkRevision
    }
    private enum CodingKeys: String, CodingKey, CaseIterable { case version, profile, approvalId, artworkRevision }

    public init(from decoder: Decoder) throws {
        try checkCoarseKeys(decoder, required: Set(CodingKeys.allCases.map(\.stringValue)))
        let values = try decoder.container(keyedBy: CodingKeys.self)
        version = try values.decode(Int.self, forKey: .version)
        profile = try values.decode(String.self, forKey: .profile)
        approvalId = try values.decode(String.self, forKey: .approvalId)
        artworkRevision = try values.decode(Int.self, forKey: .artworkRevision)
        guard valid else { throw AuthoringFailure("IMPORT_INPUT_INVALID") }
    }

    var valid: Bool {
        version == 1 && profile == "viewer-png-box-512-v1"
            && (1...2_147_483_647).contains(artworkRevision)
            && approvalId.range(
                of: #"^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"#,
                options: [.regularExpression, .caseInsensitive]
            ) != nil
    }
}

private struct CoarseWireKey: CodingKey {
    let stringValue: String
    let intValue: Int? = nil
    init?(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { return nil }
}

private func checkCoarseKeys(_ decoder: Decoder, required: Set<String>, complete: Set<String>? = nil) throws {
    let container = try decoder.container(keyedBy: CoarseWireKey.self)
    let keys = Set(container.allKeys.map(\.stringValue))
    guard keys == required || keys == complete else {
        throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unexpected coarse preparation shape"))
    }
}

public struct NativeCoarsePreparationStatus: Decodable, Sendable {
    public let profile: String
    public let approvalId: String
    public let artworkRevision: Int
    public let status: String
    public let workerDrained: Bool
    public let assetId: String?
    public let sha256: String?
    public let bytes: Int?
    private enum CodingKeys: String, CodingKey, CaseIterable {
        case profile, approvalId, artworkRevision, status, workerDrained, assetId, sha256, bytes
    }

    public init(from decoder: Decoder) throws {
        try checkCoarseKeys(decoder, required: ["profile", "approvalId", "artworkRevision", "status", "workerDrained"],
                            complete: Set(CodingKeys.allCases.map(\.stringValue)))
        let values = try decoder.container(keyedBy: CodingKeys.self)
        profile = try values.decode(String.self, forKey: .profile)
        approvalId = try values.decode(String.self, forKey: .approvalId)
        artworkRevision = try values.decode(Int.self, forKey: .artworkRevision)
        status = try values.decode(String.self, forKey: .status)
        workerDrained = try values.decode(Bool.self, forKey: .workerDrained)
        assetId = values.contains(.assetId) ? try values.decode(String.self, forKey: .assetId) : nil
        sha256 = values.contains(.sha256) ? try values.decode(String.self, forKey: .sha256) : nil
        bytes = values.contains(.bytes) ? try values.decode(Int.self, forKey: .bytes) : nil
    }

    func matches(_ intent: NativeCoarsePreparationIntent, job: NativeImportJob) -> Bool {
        guard intent.valid, profile == intent.profile, approvalId.lowercased() == intent.approvalId.lowercased(),
              artworkRevision == intent.artworkRevision else { return false }
        switch job.state {
        case "approved":
            guard status == "ready", let assetId, let primary = job.assetId, let original = job.originalAssetId,
                  let coarse = UUID(uuidString: assetId), let full = UUID(uuidString: primary),
                  let source = UUID(uuidString: original), Set([coarse, full, source]).count == 3,
                  let sha256, nativeImportDigest(sha256), let bytes, (1...8_388_608).contains(bytes)
            else { return false }
            return true
        case "uploading", "queued", "processing":
            return status == "pending" && !workerDrained && assetId == nil && sha256 == nil && bytes == nil
        case "failed", "cancelled":
            return status == "failed" && assetId == nil && sha256 == nil && bytes == nil
        default:
            return false
        }
    }
}

/// Client-declared transform provenance. It is neither an authority marker nor
/// evidence that the original is stored or that server registration succeeded.
public struct NativeImportDerivedRecipe: Codable, Equatable, Sendable {
    public struct ColourConversion: Codable, Equatable, Sendable {
        public let version: Int
        public let algorithm: String
        public let sourceIccSha256: String
    }
    public let version: Int
    public let role: String
    public let sampling: String
    public let colourMeaning: String
    public let recipeSha256: String
    public let settingsSha256: String
    public let sourcePixelsSha256: String
    public let outputPixelsSha256: String
    public let toolVersion: String
    public var colourConversion: ColourConversion? = nil

    private struct Canonical: Encodable { let version: Int; let role: String; let sampling: String; let colourMeaning: String }
    public static func recipeSHA256(colourMeaning: String) throws -> String {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return nativeImportSHA256(try encoder.encode(Canonical(version: 1, role: "baseColor", sampling: "nearest-centre", colourMeaning: colourMeaning)))
    }
    public var valid: Bool {
        version == 1 && role == "baseColor" && sampling == "nearest-centre" && ["srgb", "srgb-assumed"].contains(colourMeaning)
            && [recipeSha256, settingsSha256, sourcePixelsSha256, outputPixelsSha256].allSatisfy(nativeImportDigest)
            && (try? Self.recipeSHA256(colourMeaning: colourMeaning)) == recipeSha256
            && (colourConversion.map { $0.version == 1 && $0.algorithm == NativeImagePreparation.iccAlgorithm && nativeImportDigest($0.sourceIccSha256) && colourMeaning == "srgb" } ?? true)
            && !toolVersion.isEmpty && toolVersion.utf8.count <= 128 && toolVersion.utf8.allSatisfy { (32...126).contains($0) }
    }
}
public struct NativeImportDerived: Codable, Equatable, Sendable {
    public let mime: String
    public let bytes: Int
    public let sha256: String
    public let recipe: NativeImportDerivedRecipe
    public var valid: Bool { mime == "image/png" && bytes > 0 && bytes <= 8_388_608 && nativeImportDigest(sha256) && recipe.valid }
}
/// A staged result, with the immutable original still held separately by the
/// import owner. Only the server can acknowledge custody of both byte streams.
public struct NativePreparedJPEG: Sendable {
    public let originalSHA256: String
    public let png: Data
    public let derived: NativeImportDerived
    public let preparationReceipt: Data
    public static let settingsBytes = Data(#"{"algorithm":"nearest-source-colourspace-jpeg-rgb8-v1","allowSpatialSampling":true,"maximumEdge":2048}"#.utf8)
    public static let iccSettingsBytes = Data(#"{"algorithm":"icc-to-srgb-jpeg-rgb8-v1","allowSpatialSampling":true,"maximumEdge":2048}"#.utf8)

    public static func prepare(original: ImportFileSnapshot, operationID: String, generation: String,
                               isCurrent: @escaping @Sendable () -> Bool, allowICC: Bool = false) async throws -> Self {
        do {
            guard !Task<Never, Never>.isCancelled, isCurrent() else { throw NativeImagePreparation.Failure.cancelledOrStale }
            guard original.mime == "image/jpeg", !original.data.isEmpty, original.data.count <= ImportFileSnapshot.limit,
                  nativeImportSHA256(original.data) == original.sha256 else { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            let detection = Task.detached(priority: .userInitiated) {
                try NativeImagePreparation.embeddedICC(original.data, isCurrent: isCurrent)
            }
            let profile = try await withTaskCancellationHandler { try await detection.value } onCancel: { detection.cancel() }
            // Callers establish the exact compatible runtime before allowing ICC.
            guard profile == nil || allowICC else { throw NativeImagePreparation.Failure.unsupportedJPEG }
            let settings = profile == nil ? settingsBytes : iccSettingsBytes
            let binding = NativeImagePreparation.Binding(operationID: operationID, generation: generation,
                originalSHA256: original.sha256, settingsSHA256: nativeImportSHA256(settings), recipeID: profile == nil ? "native-jpeg-basecolor-rgb8" : "native-jpeg-icc-to-srgb", recipeVersion: "1")
            let candidate = try await NativeImagePreparation.prepare(sources: [.init(role: .baseColor, imageIndex: 0, jpeg: original.data, sha256: original.sha256)],
                binding: binding, settingsBytes: settings, isCurrent: isCurrent)
            guard candidate.binding == binding, candidate.images.count == 1, let image = candidate.images.first,
                  image.report.role == .baseColor, image.report.sourceImageIndex == 0,
                  image.report.sourceJPEGSHA256 == original.sha256, image.report.sourceBytes == original.data.count,
                  image.report.outputPNGSHA256 == nativeImportSHA256(image.png), image.report.outputBytes == image.png.count else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            let os = ProcessInfo.processInfo.operatingSystemVersion
            let colourMeaning = image.report.colourTransferConverted == true ? "srgb" : "srgb-assumed"
            guard image.report.sourceICCSHA256 == profile.map(nativeImportSHA256), image.report.colourTransferConverted == (profile == nil ? nil : true) else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            var recipe = NativeImportDerivedRecipe(version: 1, role: "baseColor", sampling: "nearest-centre", colourMeaning: colourMeaning,
                recipeSha256: try NativeImportDerivedRecipe.recipeSHA256(colourMeaning: colourMeaning), settingsSha256: binding.settingsSHA256,
                sourcePixelsSha256: image.report.sourceRGB8SHA256, outputPixelsSha256: image.report.outputRGB8SHA256,
                toolVersion: "\(NativeImagePreparation.tool)/\(profile == nil ? NativeImagePreparation.toolVersion : NativeImagePreparation.iccToolVersion)/macOS-\(os.majorVersion).\(os.minorVersion).\(os.patchVersion)")
            if let profile {
                recipe.colourConversion = .init(version: 1, algorithm: NativeImagePreparation.iccAlgorithm, sourceIccSha256: nativeImportSHA256(profile))
            }
            let derived = NativeImportDerived(mime: "image/png", bytes: image.png.count, sha256: nativeImportSHA256(image.png), recipe: recipe)
            guard derived.valid else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            guard !Task<Never, Never>.isCancelled, isCurrent() else { throw NativeImagePreparation.Failure.cancelledOrStale }
            return Self(originalSHA256: original.sha256, png: image.png, derived: derived, preparationReceipt: candidate.receipt)
        } catch {
            guard !Task<Never, Never>.isCancelled, isCurrent() else { throw NativeImagePreparation.Failure.cancelledOrStale }
            throw error
        }
    }
}
func nativeImportSHA256(_ bytes: Data) -> String { SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined() }
func nativeImportDigest(_ value: String) -> Bool { value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) } }
