// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit

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
            // Callers must establish a reviewed compatible runtime capability;
            // the existing UI supplies no such admission and stays fail-closed.
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
