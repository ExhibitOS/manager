// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewerBudgetPolicyRequestTests: XCTestCase {
    private func frame() throws -> ViewportFrame {
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        candidate["extensions"] = .object(["org.example/opaque": .object(["keep": .string("exact")])])
        try draft.replaceCandidate(candidate)
        return ViewportFrame(epoch: "11111111-1111-4111-8111-111111111111", sequence: 4,
            tenantId: "22222222-2222-4222-8222-222222222222", principalId: "33333333-3333-4333-8333-333333333333",
            draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
    }
    private func reply(_ request: ViewerBudgetPolicyRequest) -> DocumentValue {
        guard case .object(var fields) = request.value else { return .null }
        fields.removeValue(forKey: "type"); fields.removeValue(forKey: "policy"); fields["candidate"] = request.candidate
        return .object(fields)
    }
    func testOnlyPolicyChangesAndAutomaticTargetNormalizesLikeShared() throws {
        let frame = try frame()
        let policy: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1), "targetProfile": .string("auto"),
            "profiles": .object(["compact": .object(["admission": .object(["residentBytes": .integer(12345678)])])])])
        let request = try ViewerBudgetPolicyRequest(frame: frame, command: .object(["type": .string("set-viewer-budget-policy"), "policy": policy]))
        let checked = try request.checkedReply(reply(request)), applied = try checked.apply(to: frame.draft)
        XCTAssertEqual(applied.candidate["extensions"]["org.example/opaque"], frame.draft.candidate["extensions"]["org.example/opaque"])
        XCTAssertEqual(applied.candidate["extensions"][ViewerBudgetSettings.namespace]["targetProfile"], .null)
        XCTAssertEqual(applied.candidate["extensions"][ViewerBudgetSettings.namespace]["profiles"], policy["profiles"])
        XCTAssertEqual(frame.draft.candidate["extensions"][ViewerBudgetSettings.namespace], .null)
    }
    func testResetAbsentIsNoOpAndMixedOrStaleReplyCannotApply() throws {
        let frame = try frame(), request = try ViewerBudgetPolicyRequest(frame: frame, command: ViewerBudgetSettings.resetCommand)
        XCTAssertEqual(try request.apply(to: frame.draft), frame.draft)
        var response = reply(request); response["candidate"]["title"] = .string("Unrequested content")
        XCTAssertThrowsError(try request.checkedReply(response))
        response = reply(request); response["epoch"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try request.checkedReply(response))
        XCTAssertThrowsError(try request.apply(to: NativeExhibitionDraft.new()))
    }
    func testInspectionRepliesAreNeverReplayedAsFreshCurrentAnalysis() throws {
        let bytes = Data("same request".utf8)
        XCTAssertNil(try NativeAutomationSession.replayResponse(for: "viewer-budget-inspect", prior: (bytes, .object(["old": .bool(true)])), matching: bytes))
    }

    func testRetainedPolicyDeltaRejectsUnrelatedEditsAndWrongVersion() throws {
        let frame = try frame()
        let policy: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1),
            "targetProfile": .string("compact"), "profiles": .object([:])])
        let request = try ViewerBudgetPolicyRequest(frame: frame, command: .object(["type": .string("set-viewer-budget-policy"), "policy": policy]))
        let next = try request.apply(to: frame.draft)
        XCTAssertNoThrow(try ViewerBudgetPolicyRequest.requirePolicyOnlyChange(from: frame.draft, to: next))
        var changedBody = next.document; changedBody["candidate"]["title"] = .string("Unrelated edit")
        let changed = try NativeExhibitionDraft(document: changedBody)
        XCTAssertThrowsError(try ViewerBudgetPolicyRequest.requirePolicyOnlyChange(from: frame.draft, to: changed))
        var body = next.document; body["id"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try ViewerBudgetPolicyRequest.requirePolicyOnlyChange(from: frame.draft, to: NativeExhibitionDraft(document: body)))
        XCTAssertThrowsError(try ViewerBudgetPolicyRequest.requirePolicyOnlyChange(from: next, to: next))
    }

    func testOnlyExactPolicyPendingIntentCanUseScopedReplay() throws {
        let frame = try frame()
        var next = frame.draft, candidate = frame.draft.candidate
        candidate["extensions"][ViewerBudgetSettings.namespace] = .object(["version": .integer(1), "policyVersion": .integer(1), "targetProfile": .string("compact"), "profiles": .object([:])])
        try next.replaceCandidate(candidate)
        let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\""
        var state = NativeDocumentState(format: 1, revision: 3, workspaceId: UUID().uuidString.lowercased(),
            tenantId: frame.tenantId, principalId: frame.principalId, draft: next, etag: etag,
            savedEditVersion: frame.draft.document["editVersion"].integer,
            pending: .init(etag: etag, draft: next, origin: ViewerBudgetPolicyRequest.pendingOrigin))
        XCTAssertTrue(ViewerBudgetPolicyRequest.isPending(in: state))
        state.pending = .init(etag: etag, draft: next, origin: "bounded-placement-v1")
        XCTAssertFalse(ViewerBudgetPolicyRequest.isPending(in: state))
        state.pending = .init(etag: etag, draft: frame.draft, origin: ViewerBudgetPolicyRequest.pendingOrigin)
        XCTAssertFalse(ViewerBudgetPolicyRequest.isPending(in: state))
        state.pending = .init(etag: nil, draft: next, origin: ViewerBudgetPolicyRequest.pendingOrigin)
        XCTAssertFalse(ViewerBudgetPolicyRequest.isPending(in: state))
    }
}
