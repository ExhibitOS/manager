// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeAutomationArtworkTests: XCTestCase {
    private let artworkId = "11111111-1111-4111-8111-111111111111"
    private let assetId = "22222222-2222-4222-8222-222222222222"
    private let jobId = "33333333-3333-4333-8333-333333333333"
    private let tenantId = "44444444-4444-4444-8444-444444444444"
    func testReviewedReplacementKeepsImageRepairAndModelCommandsDisjoint() throws {
        let placement = "55555555-5555-4555-8555-555555555555"
        let hash = String(repeating: "a", count: 64)
        let common: [String: DocumentValue] = [
            "artworkId": .string(artworkId), "artworkRevision": .integer(1),
            "importJobId": .string(jobId), "studioRevision": .integer(4),
            "etag": .string("\"studio-r4-\(hash)\"")
        ]
        func parse(_ extra: [String: DocumentValue]) throws -> NativeAutomationReplacementCommand {
            try .init(.object(["command": .object(common.merging(extra) { _, value in value })]))
        }
        let image = try parse(["coarseSha256": .string(hash)])
        guard case .image(let digest) = image.target else { return XCTFail("Expected legacy image command") }
        XCTAssertEqual(digest, hash)

        let repair = ["kind": DocumentValue.string("product-image-thumbnail-repair-v1"),
                      "receiptSha256": .string(hash), "artworkRevision": .integer(2)]
        let repaired = try parse(repair)
        guard case .productImageThumbnailRepair(let receiptSHA256) = repaired.target else {
            return XCTFail("Expected retained product image thumbnail repair")
        }
        XCTAssertEqual(receiptSHA256, hash)
        XCTAssertEqual(repaired.artworkRevision, 2)
        XCTAssertThrowsError(try parse(["kind": .string("product-image-thumbnail-repair-v1")]))
        XCTAssertThrowsError(try parse(repair.merging(["placementId": .string(placement)]) { _, value in value }))

        let model = ["kind": DocumentValue.string("approved-model-candidate-v1"),
                     "placementId": .string(placement)]
        let command = try parse(model)
        guard case .approvedModel(let placementID) = command.target else { return XCTFail("Expected approved model command") }
        XCTAssertEqual(placementID, placement)
        XCTAssertEqual(command.artworkId, artworkId)
        XCTAssertEqual(command.importJobId, jobId)
        XCTAssertThrowsError(try parse(["kind": .string("approved-model-candidate-v1")]))
        XCTAssertThrowsError(try parse(["placementId": .string(placement), "coarseSha256": .string(hash)]))
        XCTAssertThrowsError(try parse(model.merging(["coarseSha256": .string(hash)]) { _, value in value }))
        XCTAssertThrowsError(try parse(model.merging(["etag": .string("\"studio-r3-\(hash)\"")]) { _, value in value }))
    }

    private func approvalFixture() throws -> (NativeArtwork, NativeImportJob, NativeImportIntent, DocumentValue) {
        let metadata = ArtworkMetadata()
        let encoded = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(metadata))
        let intent = NativeImportIntent(artworkId: artworkId, mime: "model/gltf-binary", sha256: String(repeating: "a", count: 64), bytes: 100, scaleMeters: 1, rights: metadata.rights)
        let body: DocumentValue = .object(["id": .string(artworkId), "artistId": .string(tenantId), "revision": .integer(3), "archived": .bool(false), "metadata": encoded,
            "assets": .array([.object(["id": .string(assetId), "mime": .string(intent.mime), "state": .string("approved"), "sha256": .string(intent.sha256), "bytes": .integer(100)])])])
        let art = try JSONDecoder().decode(NativeArtwork.self, from: JSONEncoder().encode(body))
        let job = NativeImportJob(id: jobId, state: "approved", progress: 100, attempts: 1, errorCode: nil, assetId: assetId, retryAt: nil)
        return (art, job, intent, body)
    }
    func testApprovalBindsExactOriginalHashRightsAndBothRevisions() throws {
        let (art, job, intent, body) = try approvalFixture()
        try NativeAutomationArtworkRules.checkApproval(artwork: art, job: job, jobId: jobId, intent: intent, revision: 3, assetId: assetId)
        for revision in [Int64(2), 0, 2_147_483_648] {
            XCTAssertThrowsError(try NativeAutomationArtworkRules.checkApproval(artwork: art, job: job, jobId: jobId, intent: intent, revision: revision, assetId: assetId))
        }
        for key in ["sha256", "mime", "state", "bytes"] {
            var changed = body, assets = changed["assets"].items!
            assets[0][key] = key == "bytes" ? .integer(99) : .string("changed")
            changed["assets"] = .array(assets)
            let other = try JSONDecoder().decode(NativeArtwork.self, from: JSONEncoder().encode(changed))
            XCTAssertThrowsError(try NativeAutomationArtworkRules.checkApproval(artwork: other, job: job, jobId: jobId, intent: intent, revision: 3, assetId: assetId))
        }
        var changed = body; changed["metadata"]["rights"]["holder"] = .string("another holder")
        let other = try JSONDecoder().decode(NativeArtwork.self, from: JSONEncoder().encode(changed))
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkApproval(artwork: other, job: job, jobId: jobId, intent: intent, revision: 3, assetId: assetId))
        let pending = NativeImportJob(id: jobId, state: "processing", progress: 50, attempts: 1, errorCode: nil, assetId: assetId, retryAt: nil)
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkApproval(artwork: art, job: pending, jobId: jobId, intent: intent, revision: 3, assetId: assetId))
    }
    func testICCReadbackBindsExactApprovedDerivativeAndPrivateOriginal() throws {
        let (_, _, _, fixture) = try approvalFixture()
        let originalId = "55555555-5555-4555-8555-555555555555"
        let originalSHA256 = String(repeating: "a", count: 64)
        let displaySHA256 = String(repeating: "b", count: 64)
        let sampleDigest = String(repeating: "c", count: 64)
        let recipe = NativeImportDerivedRecipe(version: 1, role: "baseColor", sampling: "nearest-centre", colourMeaning: "srgb",
            recipeSha256: try NativeImportDerivedRecipe.recipeSHA256(colourMeaning: "srgb"),
            settingsSha256: sampleDigest, sourcePixelsSha256: sampleDigest, outputPixelsSha256: sampleDigest, toolVersion: "fixture",
            colourConversion: .init(version: 1, algorithm: NativeImagePreparation.iccAlgorithm, sourceIccSha256: sampleDigest))
        let metadata = ArtworkMetadata()
        let intent = NativeImportIntent(artworkId: artworkId, mime: "image/jpeg", sha256: originalSHA256,
            bytes: 100, scaleMeters: 1, rights: metadata.rights,
            derived: .init(mime: "image/png", bytes: 50, sha256: displaySHA256, recipe: recipe))
        var body = fixture
        body["approvedRevision"] = .integer(3)
        body["approvedAssetId"] = .string(assetId)
        body["assets"] = .array([
            .object(["id": .string(originalId), "mime": .string("image/jpeg"), "state": .string("quarantine"),
                     "sha256": .string(originalSHA256), "bytes": .integer(100)]),
            .object(["id": .string(assetId), "mime": .string("image/png"), "state": .string("approved"),
                     "sha256": .string(displaySHA256), "bytes": .integer(50)])
        ])
        let recipeWire = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(recipe))
        let receipt: DocumentValue = .object([
            "id": .string(jobId), "state": .string("approved"), "progress": .integer(100), "attempts": .integer(1),
            "assetId": .string(assetId), "originalAssetId": .string(originalId),
            "imagePreparation": .object(["provenance": .string("client-declared"), "originalSha256": .string(originalSHA256),
                "derivedSha256": .string(displaySHA256), "recipe": recipeWire])
        ])
        func read(_ assetRecord: DocumentValue, _ jobRecord: DocumentValue, revision: Int64 = 3) throws -> NativeImagePreparationStatus {
            let artwork = try JSONDecoder().decode(NativeArtwork.self, from: JSONEncoder().encode(assetRecord))
            let job = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(jobRecord))
            return try NativeAutomationArtworkRules.checkICCReadback(artwork: artwork, job: job, jobId: jobId, intent: intent, revision: revision)
        }
        XCTAssertEqual(try read(body, receipt).recipe, recipe)
        XCTAssertThrowsError(try read(body, receipt, revision: 2))
        var tampered = receipt
        tampered["imagePreparation"]["recipe"]["colourConversion"]["sourceIccSha256"] = .string(displaySHA256)
        XCTAssertThrowsError(try read(body, tampered))
        tampered = receipt
        tampered["originalAssetId"] = .string(assetId)
        XCTAssertThrowsError(try read(body, tampered))
        var exposed = body
        var assets = try XCTUnwrap(exposed["assets"].items)
        assets[0]["state"] = .string("approved")
        exposed["assets"] = .array(assets)
        XCTAssertThrowsError(try read(exposed, receipt))
    }

    func testRegistrationRejectsOtherTenantRevisionAndNonDisplayRights() throws {
        let snapshot: DocumentValue = .object(["id": .string(artworkId), "artworkType": .string("sculpture"),
            "extensions": .object(["org.exhibitos.studio/cms": .object(["tenantId": .string(tenantId), "artworkId": .string(artworkId), "revision": .integer(3)])]),
            "rights": .object(["permissions": .object(["display": .bool(true)])])])
        try NativeAutomationArtworkRules.checkRegistration(snapshot, artworkId: artworkId, revision: 3, tenantId: tenantId)
        for (key, value) in [("tenantId", DocumentValue.string(artworkId)), ("artworkId", .string(tenantId)), ("revision", .integer(2))] {
            var changed = snapshot; changed["extensions"]["org.exhibitos.studio/cms"][key] = value
            XCTAssertThrowsError(try NativeAutomationArtworkRules.checkRegistration(changed, artworkId: artworkId, revision: 3, tenantId: tenantId))
        }
        var changed = snapshot; changed["rights"]["permissions"]["display"] = .bool(false)
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkRegistration(changed, artworkId: artworkId, revision: 3, tenantId: tenantId))
        var image = snapshot; image["artworkType"] = .string("image")
        image["primaryAssetId"] = .string(assetId)
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkRegistration(image, artworkId: artworkId, revision: 3, tenantId: tenantId, allowsImage: true))
        image["assets"] = .array([.object(["id": .string(assetId), "mime": .string("image/png"), "role": .string("image")])])
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkRegistration(image, artworkId: artworkId, revision: 3, tenantId: tenantId))
        try NativeAutomationArtworkRules.checkRegistration(image, artworkId: artworkId, revision: 3, tenantId: tenantId, allowsImage: true)
        image["rights"]["permissions"]["display"] = .bool(false)
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkRegistration(image, artworkId: artworkId, revision: 3, tenantId: tenantId, allowsImage: true))
        image["rights"]["permissions"]["display"] = .bool(true)
        image["artworkType"] = .string("sound")
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkRegistration(image, artworkId: artworkId, revision: 3, tenantId: tenantId, allowsImage: true))
    }
    func testQualifiedMetadataRoundTripsAndBindsExactUpload() throws {
        let (_, job, intent, body) = try approvalFixture()
        var qualified = body
        qualified["metadata"]["provenance"]["sourceUnits"] = .string("unknown")
        qualified["metadata"]["displayReference"] = .object([
            "version": .integer(1), "status": .string("provisional-uncalibrated"), "assetSha256": .string(intent.sha256),
            "rawBounds": .object(["min": .vector([-2, -3, -1]), "max": .vector([2, 3, 1])]),
            "referenceHeightMeters": .number(0.3),
            "pose": .object(["position": .vector([0, 0, 0]), "rotation": .vector([0, 0, 0, 1]), "scale": .vector([0.05, 0.05, 0.05])])
        ])
        let art = try JSONDecoder().decode(NativeArtwork.self, from: JSONEncoder().encode(qualified))
        let metadata = try XCTUnwrap(art.metadata)
        let encoded = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(metadata))
        XCTAssertEqual(encoded, qualified["metadata"])
        try NativeAutomationArtworkRules.checkApproval(artwork: art, job: job, jobId: jobId, intent: intent, revision: 3, assetId: assetId)
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkDisplayAsset(metadata, sha256: intent.sha256, scaleMeters: 0.05))
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkDisplayAsset(metadata, sha256: String(repeating: "b", count: 64), scaleMeters: 1))
        var unqualified = metadata
        unqualified.displayReference = nil
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkDisplayAsset(unqualified, sha256: intent.sha256, scaleMeters: 1))
    }
    func testUploadJournalCannotCrossWorkspaceAndCommandCannotAddDerivation() throws {
        let document = NativeDocumentState(format: 1, revision: 0, workspaceId: artworkId, tenantId: tenantId, principalId: assetId)
        var transfer = NativeUploadState(format: 1, revision: 0, workspaceId: artworkId, tenantId: tenantId, principalId: assetId)
        try NativeAutomationArtworkRules.checkUploadScope(transfer, document: document)
        transfer = NativeUploadState(format: 1, revision: 0, workspaceId: jobId, tenantId: tenantId, principalId: assetId)
        XCTAssertThrowsError(try NativeAutomationArtworkRules.checkUploadScope(transfer, document: document))
        let request: DocumentValue = .object(["command": .object(["artworkId": .string(artworkId), "scaleMeters": .integer(1), "artworkRevision": .integer(3), "modelDerivation": .null])])
        XCTAssertThrowsError(try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "scaleMeters", "artworkRevision"]))
    }
}
