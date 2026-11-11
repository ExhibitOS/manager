// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Dedicated first-preview repair. A rendered selection is a separate contract;
/// completion is the embedded preview and exact persisted response, even at selectionVersion zero.
enum NativeProductImageThumbnailRepair {
    static let profile = "product-image-thumbnail-repair-v1"
    static let thumbnail = "org.exhibitos.viewer/thumbnail"
    static let cms = "org.exhibitos.studio/cms"

    static func input(command: NativeAutomationReplacementCommand, requestId: String) throws -> DocumentValue {
        guard case .productImageThumbnailRepair = command.target,
              UUID(uuidString: requestId)?.uuidString.lowercased() == requestId else { throw invalid() }
        return .object(["kind": .string(profile), "requestId": .string(requestId),
            "artworkId": .string(command.artworkId), "artworkRevision": .integer(Int64(command.artworkRevision)),
            "importJobId": .string(command.importJobId)])
    }

    static func missingPreview(_ artwork: DocumentValue) -> Bool {
        guard case .object(let extensions) = artwork["extensions"], extensions[thumbnail] == nil,
              let assets = artwork["assets"].items else { return false }
        return !assets.contains { $0["role"] == .string("preview") }
    }

    // Server productImageReplacementSnapshot explicitly rejects prior.revision <=
    // receipt.artworkRevision: repair is for a previously adopted product result.
    static func acceptsProducerRevision(_ producer: Int, current: Int) -> Bool {
        current > 1 && current < 2_147_483_647 && (1..<current).contains(producer)
    }

    static func review(draft: NativeExhibitionDraft, artwork: NativeArtwork, approved: DocumentValue,
                       job: NativeImportJob, command: NativeAutomationReplacementCommand,
                       workspaceId: String, principalId: String) throws {
        guard artwork.id == command.artworkId, artwork.revision == command.artworkRevision,
              artwork.approvedRevision == artwork.revision, !artwork.archived,
              let rights = artwork.metadata?.rights, rights.permissions.display, rights.permissions.export,
              job.id == command.importJobId, job.state == "approved", job.progress == 100,
              let status = job.productImagePreparation, let receipt = status.serverReceipt,
              receipt.artworkId == command.artworkId,
              acceptsProducerRevision(receipt.artworkRevision, current: command.artworkRevision),
              artwork.approvedAssetId == receipt.full.assetId,
              draft.artworks.filter({ $0["id"].string == command.artworkId }) == [approved],
              approved["revision"].integer == Int64(command.artworkRevision), approved["artworkType"] == .string("image"),
              missingPreview(approved) else { throw invalid() }
        let rightsValue = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(rights))
        guard approved["rights"] == rightsValue else { throw invalid() }
        let preparation = NativeProductImagePreparationIntent(artworkRevision: receipt.artworkRevision,
            fresh: status.profile == NativeProductImagePreparationIntent.freshProfileID)
        let intent = NativeImportIntent(artworkId: command.artworkId, mime: receipt.original.mime,
            sha256: receipt.original.sha256, bytes: receipt.original.bytes, scaleMeters: 1, rights: rights,
            productImagePreparation: preparation)
        guard status.validatesReady(intent: intent, preparation: preparation, job: job,
                                    workspaceID: workspaceId, principalID: principalId),
              approved["primaryAssetId"].string == receipt.full.assetId,
              NativeProductImagePreparationStatus.matchesCanonicalTiers(approved, full: receipt.full, coarse: receipt.coarse),
              [receipt.full, receipt.coarse].compactMap({ $0 }).allSatisfy({ image in
                  approved["assets"].items?.contains { asset in
                      asset["id"].string == image.assetId && asset["role"] == .string("image")
                          && asset["mime"].string == image.mime && asset["sha256"].string == image.sha256
                          && asset["bytes"].integer == Int64(image.bytes)
                  } == true
              }) else { throw invalid() }
    }

    /// Reconstruct the only permitted delta and compare the entire opaque draft.
    /// Rights, Full/Coarse, LOD, policies, other artworks and all poses survive exactly.
    static func validateChange(before: NativeExhibitionDraft, after: NativeExhibitionDraft,
                               command: NativeAutomationReplacementCommand) throws {
        let matches = before.artworks.indices.filter { before.artworks[$0]["id"].string == command.artworkId }
        guard matches.count == 1, let index = matches.first, after.artworks.count == before.artworks.count else { throw invalid() }
        let prior = before.artworks[index], next = after.artworks[index]
        guard missingPreview(prior), prior["artworkType"] == .string("image"),
              prior["revision"].integer == Int64(command.artworkRevision),
              next["revision"].integer == Int64(command.artworkRevision + 1),
              let revisionId = next["revisionId"].string, UUID(uuidString: revisionId) != nil,
              next["revisionId"] != prior["revisionId"], let at = next["createdAt"].string,
              ISO8601DateFormatter().date(from: at) != nil || ISO8601DateFormatter.fractionalDate(at),
              let assets = prior["assets"].items, let newAssets = next["assets"].items,
              newAssets.count == assets.count + 1, Array(newAssets.dropLast()) == assets,
              let preview = newAssets.last, let assetId = preview["id"].string, UUID(uuidString: assetId) != nil,
              !before.artworks.flatMap({ $0["assets"].items ?? [] }).contains(where: { $0["id"].string == assetId }),
              let bytes = preview["bytes"].integer, (1...512 * 1024).contains(bytes),
              let digest = preview["sha256"].string, nativeImportDigest(digest),
              preview == .object(["id": .string(assetId), "path": .string("assets/\(assetId)/thumbnail.png"),
                  "role": .string("preview"), "mime": .string("image/png"), "bytes": .integer(bytes), "sha256": .string(digest)]),
              case .object(var extensions) = prior["extensions"],
              let events = prior["provenance"]["events"].items else { throw invalid() }
        var expectedArtwork = prior
        for key in ["revision", "revisionId", "createdAt", "assets"] { expectedArtwork[key] = next[key] }
        var binding = prior["extensions"][cms]
        guard binding["artworkId"].string == command.artworkId,
              binding["revision"].integer == Int64(command.artworkRevision) else { throw invalid() }
        binding["revision"] = next["revision"]
        extensions[cms] = binding
        extensions[thumbnail] = .object(["version": .integer(1), "assetId": .string(assetId)])
        expectedArtwork["extensions"] = .object(extensions)
        expectedArtwork["provenance"]["events"] = .array(events + [.object([
            "id": .string(revisionId), "type": .string("edited"), "at": .string(at),
            "description": .string("Generated first preview from the currently approved Full display image.")])])
        guard expectedArtwork == next else { throw invalid() }
        var document = before.document, artworks = before.artworks, placements = before.placements
        artworks[index] = next
        var changed = 0
        for i in placements.indices where placements[i]["artworkRevisionId"] == prior["revisionId"] {
            guard placements[i]["assetId"] == prior["primaryAssetId"] else { throw invalid() }
            placements[i]["artworkRevisionId"] = .string(revisionId)
            changed += 1
        }
        guard changed > 0 else { throw invalid() }
        document["candidate"]["artworks"] = .array(artworks)
        document["candidate"]["placements"] = .array(placements)
        guard document == after.document else { throw invalid() }
    }

    private static func invalid() -> AuthoringFailure { AuthoringFailure("ARTWORK_REPLACEMENT_INVALID") }
}

private extension ISO8601DateFormatter {
    static func fractionalDate(_ value: String) -> Bool {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: value) != nil
    }
}
