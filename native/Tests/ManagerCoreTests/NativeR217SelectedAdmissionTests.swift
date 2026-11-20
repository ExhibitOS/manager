// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeR217SelectedAdmissionTests: XCTestCase {
    private func fixture(_ key: String) throws -> Data {
        guard let path = ProcessInfo.processInfo.environment[key] else { throw XCTSkip("Requires independently sealed detached evidence") }
        return try Data(contentsOf: URL(fileURLWithPath: path))
    }

    func testIndependentR217ScopeCommandsThroughExistingDispatch() throws {
        try checkSelectedCommand("EXHIBITOS_TEST_R217_PUBLIC_COMMAND", expected: AuthoringAPI.r217PublicFinishesManifest,
                                 other: AuthoringAPI.r217CannonManifest)
        try checkSelectedCommand("EXHIBITOS_TEST_R217_CANNON_COMMAND", expected: AuthoringAPI.r217CannonManifest,
                                 other: AuthoringAPI.r217PublicFinishesManifest)
    }

    func testIndependentR217WheatCommandThroughExistingDispatch() throws {
        try checkSelectedCommand("EXHIBITOS_TEST_R217_WHEAT_COMMAND", expected: AuthoringAPI.r217WheatSelectedManifest,
                                 other: AuthoringAPI.r217PublicFinishesManifest)
    }

    func testIndependentR218PaintingCommandThroughExistingDispatch() throws {
        try checkSelectedCommand("EXHIBITOS_TEST_R218_PAINTING_COMMAND", expected: AuthoringAPI.r218PaintingSelectedManifest,
                                 other: AuthoringAPI.r217WheatSelectedManifest)
    }

    func testIndependentR219WheatFitCommandThroughExistingDispatch() throws {
        try checkSelectedCommand("EXHIBITOS_TEST_R219_WHEAT_FIT_COMMAND", expected: AuthoringAPI.r219WheatFitSelectedManifest,
                                 other: AuthoringAPI.r218PaintingSelectedManifest)
    }

    func testIndependentR219ClosingCommandThroughExistingDispatch() throws {
        try checkSelectedCommand("EXHIBITOS_TEST_R219_CLOSING_COMMAND", expected: AuthoringAPI.r219ClosingSelectedManifest,
                                 other: AuthoringAPI.r219WheatFitSelectedManifest)
    }

    private func checkSelectedCommand(_ file: String, expected: String, other: String) throws {
        let sealed = try JSONDecoder().decode(DocumentValue.self, from: fixture(file))
        let manifest = try XCTUnwrap(sealed["runtimeManifestSha256"].string)
        XCTAssertEqual(manifest, expected)
        let command = sealed["command"]
        var request: DocumentValue = .object(["operation": .string("export-static-native-original"), "command": command])
        let selected = try XCTUnwrap(NativeAutomationSession.classifySelectedOriginal(in: request, manifest: manifest))
        let image = try NativePublicationOriginalImage(command["originalImage"])
        let etag = try XCTUnwrap(command["etag"].string)
        XCTAssertEqual(try selected.sourceCommitment(etag: etag, originalImage: image, runtimeManifest: manifest),
                       sealed["sdkOriginalCommandCommitmentSha256"].string)
        try selected.checkSource(etag: etag, originalImage: image, manifest: manifest)
        XCTAssertNotEqual(try selected.sourceCommitment(etag: etag, originalImage: image, runtimeManifest: other),
                          sealed["sdkOriginalCommandCommitmentSha256"].string)
        XCTAssertThrowsError(try selected.checkSource(etag: etag, originalImage: image,
                                                      manifest: AuthoringAPI.r216SelectedFinishesManifest))
        XCTAssertThrowsError(try selected.checkSource(etag: etag + " ", originalImage: image, manifest: manifest))
        XCTAssertThrowsError(try selected.checkSource(etag: etag, originalImage: nil, manifest: manifest))
        request["operation"] = .string("workspace-frame-attach")
        XCTAssertThrowsError(try NativeAutomationSession.classifySelectedOriginal(in: request, manifest: manifest))
        XCTAssertNil(try NativeAutomationSession.classifySelectedOriginal(in: .object(["operation": .string("read")]), manifest: manifest))
    }

    func testFreshRequestPreservesExactDetachedPublicationHistory() throws {
        try checkFreshRequestPreservesHistory(manifest: AuthoringAPI.r217PublicFinishesManifest,
                                              publicationFixture: "EXHIBITOS_TEST_R216_PUBLICATION")
    }

    func testFreshWheatRequestPreservesAcceptedPublicFinishesHistory() throws {
        try checkFreshRequestPreservesHistory(manifest: AuthoringAPI.r217WheatSelectedManifest,
                                              publicationFixture: "EXHIBITOS_TEST_R217_PUBLIC_FINISHES_PUBLICATION")
    }

    func testFreshR218PaintingRequestPreservesAcceptedWheatHistory() throws {
        try checkFreshRequestPreservesHistory(manifest: AuthoringAPI.r218PaintingSelectedManifest,
                                              publicationFixture: "EXHIBITOS_TEST_R217_WHEAT_PUBLICATION",
                                              documentFixture: "EXHIBITOS_TEST_R218_SAVED_DOCUMENT")
    }

    func testFreshR219WheatFitRequestPreservesAcceptedPublicationHistory() throws {
        try checkFreshRequestPreservesHistory(manifest: AuthoringAPI.r219WheatFitSelectedManifest,
                                              publicationFixture: "EXHIBITOS_TEST_R218_PAINTING_PUBLICATION",
                                              documentFixture: "EXHIBITOS_TEST_R219_WHEAT_FIT_JOURNAL")
        let decoder = JSONDecoder()
        let source = try decoder.decode(NativeDocumentState.self, from: fixture("EXHIBITOS_TEST_R219_WHEAT_FIT_JOURNAL"))
        let publication = try decoder.decode(NativePublicationState.self, from: fixture("EXHIBITOS_TEST_R218_PAINTING_PUBLICATION"))
        let wrongOwner = NativeDocumentState(format: source.format, revision: source.revision, workspaceId: source.workspaceId,
            tenantId: source.tenantId, principalId: UUID().uuidString.lowercased(), draft: source.draft,
            etag: source.etag, savedEditVersion: source.savedEditVersion, pending: nil)
        let requestID = UUID().uuidString.lowercased()
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: AuthoringAPI.r219WheatFitSelectedManifest,
            document: wrongOwner, publication: publication, requestID: requestID))
        var stale = source
        stale.etag = nil
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: AuthoringAPI.r219WheatFitSelectedManifest,
            document: stale, publication: publication, requestID: requestID))
        var pending = source
        pending.pending = .init(etag: source.etag, draft: try XCTUnwrap(source.draft))
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: AuthoringAPI.r219WheatFitSelectedManifest,
            document: pending, publication: publication, requestID: requestID))
    }

    func testFreshR219ClosingRequiresClaimedWheatPredecessor() throws {
        let manifest = AuthoringAPI.r219ClosingSelectedManifest
        try checkFreshRequestPreservesHistory(manifest: manifest, publicationFixture: "EXHIBITOS_TEST_R219_WHEAT_PUBLICATION",
                                               documentFixture: "EXHIBITOS_TEST_R219_WHEAT_FIT_JOURNAL")
        let decoder = JSONDecoder()
        let source = try decoder.decode(NativeDocumentState.self, from: fixture("EXHIBITOS_TEST_R219_WHEAT_FIT_JOURNAL"))
        let closed = try decoder.decode(NativePublicationState.self, from: fixture("EXHIBITOS_TEST_R219_WHEAT_PUBLICATION"))
        let fresh = UUID().uuidString.lowercased()
        var invalid = closed
        invalid.completed = nil
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: manifest,
            document: source, publication: invalid, requestID: fresh))
        invalid = closed
        invalid.nativeOriginalAttempts = nil
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: manifest,
            document: source, publication: invalid, requestID: fresh))
    }

    private func checkFreshRequestPreservesHistory(manifest: String, publicationFixture: String,
                                                   documentFixture: String? = nil) throws {
        let decoder = JSONDecoder()
        let sourceBytes: Data
        if let documentFixture {
            sourceBytes = try fixture(documentFixture)
        } else {
            let read = try decoder.decode(DocumentValue.self, from: fixture("EXHIBITOS_TEST_R217_AUTHENTICATED_READ"))
            sourceBytes = try JSONEncoder().encode(read["document"])
        }
        let source = try decoder.decode(NativeDocumentState.self, from: sourceBytes)
        let prior = try decoder.decode(NativePublicationState.self, from: fixture(publicationFixture))
        let fresh = UUID().uuidString.lowercased()
        try NativeAutomationSession.checkFreshOfficialOriginal(manifest: manifest, document: source, publication: prior, requestID: fresh)
        let completed = try XCTUnwrap(prior.completed?.publishRequestId)
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: manifest, document: source,
                                                                                    publication: prior, requestID: completed))
        for id in try XCTUnwrap(prior.nativeOriginalAttempts).keys {
            XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: manifest, document: source,
                                                                                        publication: prior, requestID: id))
        }
        var wrong = source
        wrong.revision = 610
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: manifest, document: wrong,
                                                                                    publication: prior, requestID: fresh))
        var stage = prior
        let intent = try stage.nativeOriginalIntent(in: source, runtimeManifest: manifest, requestId: fresh,
                                                    requestSHA256: String(repeating: "a", count: 64))
        try stage.stageNativeOriginal(intent)
        XCTAssertThrowsError(try NativeAutomationSession.checkFreshOfficialOriginal(manifest: manifest, document: source,
                                                                                    publication: stage, requestID: fresh))
        XCTAssertEqual(prior.completed?.publishRequestId, completed)
    }
}
