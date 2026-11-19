// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerApp
@testable import ManagerCore

@MainActor
final class FixtureFirstViewportWorkTests: XCTestCase {
    func testPreservingFrameKeepsOrdinaryAuthorityAndBusyExclusion() async throws {
        let model = try await connectedModel()
        let draft = try NativeExhibitionDraft.new()
        let id = UUID().uuidString.lowercased()
        let document = NativeDocumentState(format: 1, revision: 1, workspaceId: id,
            tenantId: id, principalId: id, draft: draft, etag: "\"studio-r1-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        func frame() -> ViewportFrame {
            ViewportFrame(epoch: id, sequence: 1, tenantId: id, principalId: id, draft: draft,
                selection: nil, mode: "translate", view: "isometric", disabled: model.workBlocksViewportFrame)
        }
        let acknowledged = frame()
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        let original = try encoder.encode(acknowledged)
        var reached = false
        await model.work(viewport: .preserveAcknowledgedFrame) {
            XCTAssertTrue(model.busy)
            XCTAssertFalse(model.canSelectCatalog)
            await model.work { XCTFail("Another mutation entered while the frame was held") }
            let sameFrame = frame()
            XCTAssertEqual(try encoder.encode(sameFrame), original)
            let bound = try LightObjectRequest.nativeFrame(acknowledged: sameFrame, document: document,
                ownerEpoch: id, acceptedSequence: 1, readOnly: false, boundedEdit: nil)
            XCTAssertEqual(bound.baseETag, document.etag)
            XCTAssertThrowsError(try LightObjectRequest.nativeFrame(acknowledged: sameFrame, document: document,
                ownerEpoch: id, acceptedSequence: 2, readOnly: false, boundedEdit: nil)) {
                XCTAssertEqual(($0 as? AuthoringFailure)?.code, "VIEWPORT_FRAME_STALE")
            }
            reached = true
        }
        XCTAssertTrue(reached)
        XCTAssertNil(model.failure)
        XCTAssertFalse(model.busy)
        await model.work {
            XCTAssertTrue(model.busy)
            XCTAssertTrue(model.workBlocksViewportFrame)
        }
        XCTAssertFalse(model.workBlocksViewportFrame)
    }

    func testFailureAndCancellationReleaseFramePreservingWork() async throws {
        let model = try await connectedModel()
        for code in ["VIEWPORT_FRAME_STALE", "AUTHORING_PROPOSAL_INVALID"] {
            await model.work(viewport: .preserveAcknowledgedFrame) {
                XCTAssertTrue(model.busy)
                throw AuthoringFailure(code)
            }
            XCTAssertEqual(model.failure?.code, code)
            XCTAssertFalse(model.busy)
            XCTAssertFalse(model.workBlocksViewportFrame)
        }
        let cancelled = Task {
            await model.work(viewport: .preserveAcknowledgedFrame) {
                XCTAssertTrue(model.busy)
                withUnsafeCurrentTask { $0?.cancel() }
                try Task.checkCancellation()
                XCTFail("Cancelled work continued")
            }
        }
        await cancelled.value
        XCTAssertFalse(model.busy)
        XCTAssertFalse(model.workBlocksViewportFrame)
    }

    private func connectedModel() async throws -> ExhibitionModel {
        let model = ExhibitionModel()
        let owner = UUID().uuidString.lowercased()
        let api = try AuthoringAPI(port: 57841, tenantId: owner)
        let session = try decode(AuthoringSession.self, [
            "tenantId": owner, "userId": owner, "role": "admin", "csrfToken": "fixture",
            "expiresAt": "2099-01-01T00:00:00Z"
        ])
        let credentials = try decode(LocalAuthoringSession.self, [
            "tenantId": owner, "principalId": owner, "origin": "http://127.0.0.1:57841",
            "token": String(repeating: "a", count: 43)
        ])
        await model.connect(api: api, session: session, credentials: credentials,
            read: { throw AuthoringFailure("TEST_STORE_UNAVAILABLE") },
            write: { _ in
                XCTFail("Work-state tests must never write the document")
                throw AuthoringFailure("TEST_WRITE_UNEXPECTED")
            })
        XCTAssertEqual(model.failure?.code, "TEST_STORE_UNAVAILABLE")
        return model
    }

    private func decode<T: Decodable>(_ type: T.Type, _ value: [String: String]) throws -> T {
        try JSONDecoder().decode(type, from: JSONEncoder().encode(value))
    }
}
