// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewerBudgetAnalysisRequestTests: XCTestCase {
    private func document() throws -> NativeDocumentState {
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate; candidate["title"] = .string("Unsaved budget analysis")
        try draft.replaceCandidate(candidate)
        return NativeDocumentState(format: 1, revision: 648,
            workspaceId: "11111111-1111-4111-8111-111111111111", tenantId: "22222222-2222-4222-8222-222222222222",
            principalId: "33333333-3333-4333-8333-333333333333", draft: draft,
            etag: "\"studio-r213-" + String(repeating: "a", count: 64) + "\"", savedEditVersion: 1, pending: nil)
    }
    private func reply(_ request: ViewerBudgetAnalysisRequest) -> DocumentValue {
        .object(["version": .integer(1), "policyVersion": .integer(1), "accountingVersion": .integer(1),
            "binding": .object(["exhibitionId": .string(request.draft.id), "savedRevision": .integer(Int64(request.savedRevision)),
                "etag": .string(request.etag), "draftId": request.draft.document["id"], "draftEditVersion": request.draft.document["editVersion"],
                "source": .string("draft"), "inputSHA256": .string(String(repeating: "b", count: 64))]),
            "selection": .object(["effectiveProfile": .string("compact")]), "policySHA256": .string(String(repeating: "c", count: 64)),
            "resolvedPolicy": .object(["profile": .string("compact"), "policyVersion": .integer(1), "accountingVersion": .integer(1)]),
            "estimates": .object(["wholeExhibition": .object(["evaluation": .object(["profile": .string("compact"), "scope": .string("wholeExhibition"), "status": .string("unknown")])])]),
            "analysisComplete": .bool(false)])
    }
    func testDirtyAnalysisUsesServerCASRevisionRatherThanNativeJournalRevision() throws {
        let document = try document(), request = try ViewerBudgetAnalysisRequest(document: document, requestedProfile: "compact")
        XCTAssertTrue(document.dirty)
        XCTAssertEqual(request.savedRevision, 213)
        XCTAssertEqual(request.body["expectedRevision"], .integer(213))
        XCTAssertEqual(request.body["draft"], document.draft?.document)
        XCTAssertEqual(request.body["options"]["context"], .string("viewer"))
        XCTAssertEqual(try request.checkedReply(reply(request))["analysisComplete"], .bool(false))
    }
    func testStaleDraftRevisionETagAndRequestedProfileAreRejected() throws {
        let request = try ViewerBudgetAnalysisRequest(document: document(), requestedProfile: "compact")
        for key in ["exhibitionId", "savedRevision", "etag", "draftId", "draftEditVersion", "source", "inputSHA256"] {
            var response = reply(request); response["binding"][key] = .string("stale")
            XCTAssertThrowsError(try request.checkedReply(response), key)
        }
        var response = reply(request); response["selection"]["effectiveProfile"] = .string("native")
        XCTAssertThrowsError(try request.checkedReply(response))
    }
    func testPendingSaveAndInvalidCASBaseCannotBeGuessedOrRetried() throws {
        var document = try document()
        document.pending = .init(etag: document.etag, draft: document.draft!)
        XCTAssertThrowsError(try ViewerBudgetAnalysisRequest(document: document))
        document.pending = nil
        for etag in ["studio-r213-invalid", "\"studio-r0-" + String(repeating: "a", count: 64) + "\"", "\"studio-r2147483648-" + String(repeating: "a", count: 64) + "\""] {
            document.etag = etag
            XCTAssertThrowsError(try ViewerBudgetAnalysisRequest(document: document))
        }
    }
}
