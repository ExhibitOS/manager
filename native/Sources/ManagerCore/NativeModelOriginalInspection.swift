// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeModelOriginalInspection: Decodable, Equatable, Sendable {
    public static let profile = "bounded-compact-source-inspection-v1"

    private struct Original: Decodable, Equatable, Sendable {
        let mime: String, sha256: String, bytes: Int
    }

    private let version: Int, profileId: String, original: Original
    private let rawBounds: ArtworkMetadata.DisplayReference.Bounds
    public let triangles: Int, vertices: Int
    private let sourceUnits: String, qualification: String

    func check(file: ImportFileSnapshot, large: Bool = false) throws {
        let expectedProfile = large ? NativeModelPreparationIntent.largeProfile : NativeModelPreparationIntent.profile
        guard version == 1, profileId == expectedProfile,
              sourceUnits == "unknown", qualification == "structure-only",
              original.mime == file.mime, original.sha256 == file.sha256, original.bytes == file.data.count,
              (1...(large ? 400_000 : 200_000)).contains(triangles),
              (1...(large ? 262_144 : 131_072)).contains(vertices),
              rawBounds.min.count == 3, rawBounds.max.count == 3,
              (rawBounds.min + rawBounds.max).allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }),
              zip(rawBounds.min, rawBounds.max).allSatisfy({ (0...10_000).contains($0.1 - $0.0) })
        else { throw AuthoringFailure("MODEL_PREPARATION_PROFILE_UNSUPPORTED") }
    }

    public func applying(to metadata: ArtworkMetadata, height: Double) throws -> ArtworkMetadata {
        guard rawBounds.min.count == 3, rawBounds.max.count == 3,
              zip(rawBounds.min, rawBounds.max).allSatisfy({ $0.1 > $0.0 }),
              height.isFinite, height > 0, height <= 10_000 else {
            throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
        }
        let scale = height / (rawBounds.max[1] - rawBounds.min[1])
        guard (0.000_001...100).contains(scale) else { throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID") }
        var next = metadata
        next.provenance.sourceUnits = "unknown"
        next.provenance.scaleApplied = false
        next.displayReference = .init(version: 1, status: "provisional-uncalibrated",
            assetSha256: original.sha256, rawBounds: rawBounds, referenceHeightMeters: height,
            pose: .init(position: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [scale, scale, scale]))
        return next
    }
}
