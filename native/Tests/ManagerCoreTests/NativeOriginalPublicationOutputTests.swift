// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeOriginalPublicationOutputTests: XCTestCase {
    private func fixture() throws -> (NativeDocumentState, NativePublicationState, DocumentValue, NativeStaticExportStatus) {
        let owner = UUID().uuidString.lowercased(), original = UUID().uuidString.lowercased()
        let pubID = UUID().uuidString.lowercased(), job = UUID().uuidString.lowercased()
        let draft = try NativeExhibitionDraft.new(title: "Saved source")
        let etag = "\"studio-r7-" + String(repeating: "a", count: 64) + "\""
        let plan = String(repeating: "b", count: 64), revision = String(repeating: "c", count: 64)
        let document = NativeDocumentState(format: 1, revision: 31, workspaceId: owner, tenantId: owner,
            principalId: owner, draft: draft, etag: etag, savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let intent = try NativePublicationPending(nativeOriginalSource: draft, etag: etag, requestId: original, planBindingSha256: plan)
        let publication = NativePublication(publicationId: pubID, publicUrl: "/p/" + pubID,
            revisionSha256: revision, publishedAt: "2026-11-08T00:00:00Z", status: "published")
        var state = NativePublicationState(format: 1, revision: 3, workspaceId: owner, tenantId: owner,
            principalId: owner, pending: nil, completed: nil)
        try state.stageNativeOriginal(intent); state.pending = nil
        state.completed = NativePublicationCompleted(pending: intent, publication: publication)
        try state.claimNativeOriginalExport(intent)
        let command: DocumentValue = .object(["etag": .string(etag), "publicationId": .string(pubID),
            "revisionSha256": .string(revision), "originalPublishRequestId": .string(original),
            "cancelledDetailJobId": .string(job), "planBindingSha256": .string(plan)])
        let status = NativeStaticExportStatus(format: 1, jobId: job, workspaceId: owner, tenantId: owner,
            principalId: owner, publicationId: pubID, revisionSha256: revision, output: "/private/output",
            state: "cancelled", receipt: nil, errorCode: "NATIVE_STATIC_CANCELLED", privateEditor: true)
        return (document, state, command, status)
    }
    private func value<T: Encodable>(_ item: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(item))
    }
    private func decode<T: Decodable>(_ item: DocumentValue, as: T.Type) throws -> T {
        try JSONDecoder().decode(T.self, from: JSONEncoder().encode(item))
    }
    func testColdClaimedPublicationAllowsOutputWithoutChangingTheClaimOrSource() throws {
        let (document, state, command, status) = try fixture()
        let cold = try decode(value(state), as: NativePublicationState.self), before = try value(cold)
        let c = try NativeOriginalPublicationOutputCommand(command)
        let accepted = try cold.nativeOriginalPublicationForOutput(c, in: document)
        XCTAssertEqual(accepted, state.completed?.publication)
        XCTAssertNoThrow(try c.requireCancelledOutput(status, owner: document, path: "/private/output"))
        XCTAssertEqual(try value(cold), before)
        let intent = try NativePublicationPending(nativeOriginalSource: document.draft!, etag: document.etag!,
            requestId: c.originalPublishRequestId, planBindingSha256: c.planBindingSha256)
        var retained = cold
        XCTAssertThrowsError(try retained.stageNativeOriginal(intent))
        XCTAssertThrowsError(try cold.completed!.refuseNativeOriginalExportReplay(intent))
    }
    func testChangedSourcePublicationPurposeAndPlanAreRejectedBeforeOutput() throws {
        let (document, state, command, _) = try fixture()
        for key in ["etag", "publicationId", "revisionSha256", "originalPublishRequestId", "planBindingSha256"] {
            var wrong = command
            wrong[key] = .string(["etag": "changed", "revisionSha256": String(repeating: "d", count: 64),
                "planBindingSha256": String(repeating: "e", count: 64)][key] ?? UUID().uuidString.lowercased())
            let c = try NativeOriginalPublicationOutputCommand(wrong)
            XCTAssertThrowsError(try state.nativeOriginalPublicationForOutput(c, in: document), key)
        }
        let c = try NativeOriginalPublicationOutputCommand(command)
        var unpublished = try value(state); unpublished["completed"]["publication"]["status"] = .string("unpublished")
        XCTAssertThrowsError(try decode(unpublished, as: NativePublicationState.self).nativeOriginalPublicationForOutput(c, in: document))
        for key in ["sourceExhibitionId", "snapshotExhibitionId", "mode"] {
            var wrong = try value(state); wrong["completed"][key] = .string(key == "mode" ? "ordinary" : UUID().uuidString.lowercased())
            XCTAssertThrowsError(try decode(wrong, as: NativePublicationState.self).nativeOriginalPublicationForOutput(c, in: document), key)
        }
        var foreign = try value(document); foreign["workspaceId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try state.nativeOriginalPublicationForOutput(c, in: decode(foreign, as: NativeDocumentState.self)))
        var dirty = try value(document); dirty["savedEditVersion"] = .integer(999)
        XCTAssertThrowsError(try state.nativeOriginalPublicationForOutput(c, in: decode(dirty, as: NativeDocumentState.self)))
    }
    func testMissingUnclaimedOrDifferentAttemptCannotBecomeAnExportContinuation() throws {
        let (document, state, command, _) = try fixture(); let c = try NativeOriginalPublicationOutputCommand(command)
        var wrong = try value(state); wrong["nativeOriginalAttempts"] = .object([:])
        XCTAssertThrowsError(try decode(wrong, as: NativePublicationState.self).nativeOriginalPublicationForOutput(c, in: document))
        wrong = try value(state); wrong["nativeOriginalAttempts"][c.originalPublishRequestId]["exportClaimed"] = .bool(false)
        XCTAssertThrowsError(try decode(wrong, as: NativePublicationState.self).nativeOriginalPublicationForOutput(c, in: document))
        wrong = try value(state); wrong["nativeOriginalAttempts"][c.originalPublishRequestId]["planBindingSha256"] = .string(String(repeating: "d", count: 64))
        XCTAssertThrowsError(try decode(wrong, as: NativePublicationState.self).nativeOriginalPublicationForOutput(c, in: document))
        wrong = try value(state); wrong["pending"] = try value(NativePublicationPending(nativeOriginalSource: document.draft!, etag: document.etag!, requestId: c.originalPublishRequestId, planBindingSha256: c.planBindingSha256))
        XCTAssertThrowsError(try decode(wrong, as: NativePublicationState.self).nativeOriginalPublicationForOutput(c, in: document))
    }
    func testOnlyTheCorrelatedCancelledOwnerOutputCanStartANewAttempt() throws {
        let (document, _, command, status) = try fixture(); let c = try NativeOriginalPublicationOutputCommand(command)
        for (key, replacement) in [("jobId", .string(UUID().uuidString.lowercased())),
            ("publicationId", .string(UUID().uuidString.lowercased())), ("revisionSha256", .string(String(repeating: "d", count: 64))),
            ("workspaceId", .string(UUID().uuidString.lowercased())), ("tenantId", .string(UUID().uuidString.lowercased())),
            ("principalId", .string(UUID().uuidString.lowercased())), ("output", .string("/different")),
            ("state", .string("running")), ("state", .string("interrupted")), ("state", .string("completed")),
            ("errorCode", .null), ("privateEditor", .bool(false))] as [(String, DocumentValue)] {
            var wrong = try value(status); wrong[key] = replacement
            XCTAssertThrowsError(try c.requireCancelledOutput(decode(wrong, as: NativeStaticExportStatus.self), owner: document, path: "/private/output"), key)
        }
        XCTAssertThrowsError(try c.requireCancelledOutput(nil, owner: document, path: "/private/output"))
    }
    func testContinuationCommandIsClosedAndRequiresCanonicalBindings() throws {
        let (_, _, command, _) = try fixture()
        var wrong = command; wrong["preparePublication"] = .bool(true)
        XCTAssertThrowsError(try NativeOriginalPublicationOutputCommand(wrong))
        for key in ["etag", "publicationId", "revisionSha256", "originalPublishRequestId", "cancelledDetailJobId", "planBindingSha256"] {
            var missing = command; missing[key] = .null
            XCTAssertThrowsError(try NativeOriginalPublicationOutputCommand(missing), key)
        }
        wrong = command; wrong["cancelledDetailJobId"] = .string("not-a-job")
        XCTAssertThrowsError(try NativeOriginalPublicationOutputCommand(wrong))
    }
}
