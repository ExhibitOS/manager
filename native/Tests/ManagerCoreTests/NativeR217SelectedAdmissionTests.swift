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
        let entries = [
            ("EXHIBITOS_TEST_R217_PUBLIC_COMMAND", AuthoringAPI.r217PublicFinishesManifest, AuthoringAPI.r217CannonManifest),
            ("EXHIBITOS_TEST_R217_CANNON_COMMAND", AuthoringAPI.r217CannonManifest, AuthoringAPI.r217PublicFinishesManifest)
        ]
        for (file, expected, other) in entries {
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
    }

    func testFreshRequestPreservesExactDetachedPublicationHistory() throws {
        let decoder = JSONDecoder()
        let read = try decoder.decode(DocumentValue.self, from: fixture("EXHIBITOS_TEST_R217_AUTHENTICATED_READ"))
        let source = try decoder.decode(NativeDocumentState.self, from: JSONEncoder().encode(read["document"]))
        let prior = try decoder.decode(NativePublicationState.self, from: fixture("EXHIBITOS_TEST_R216_PUBLICATION"))
        let fresh = UUID().uuidString.lowercased()
        let manifest = AuthoringAPI.r217PublicFinishesManifest
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
