// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore
final class ViewportProtocolTests: XCTestCase {
    func testBoundedFramePreparationDoesNotAuthorizeInteractionWithoutAck() {
        XCTAssertFalse(NativeViewportAccess.frameMayBeEditable(immutable: true, boundedAdmission: false))
        XCTAssertTrue(NativeViewportAccess.frameMayBeEditable(immutable: true, boundedAdmission: true))
        XCTAssertTrue(NativeViewportAccess.boundedInteractionBlocked(boundedAdmission: true, acknowledged: false))
        XCTAssertFalse(NativeViewportAccess.boundedInteractionBlocked(boundedAdmission: true, acknowledged: true))
        XCTAssertTrue(NativeViewportAccess.frameMayBeEditable(immutable: false, boundedAdmission: false))
        XCTAssertFalse(NativeViewportAccess.boundedInteractionBlocked(boundedAdmission: false, acknowledged: false))
    }
    func testBoundedEditCapabilityRequiresEverySavedScopeField() throws {
        let tenant = UUID().uuidString.lowercased(), principal = UUID().uuidString.lowercased()
        let workspace = UUID().uuidString.lowercased(), exhibition = UUID().uuidString.lowercased()
        let etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
        let payload: [String: Any] = ["version":1,"kind":"native-bounded-edit-v1","tenantId":tenant,
            "principalId":principal,"workspaceId":workspace,"exhibitionId":exhibition,"revision":2,"etag":etag]
        let capability = try JSONDecoder().decode(NativeBoundedEditCapability.self, from: JSONSerialization.data(withJSONObject: payload))
        XCTAssertTrue(capability.matches(workspaceId: workspace, tenantId: tenant, principalId: principal, exhibitionId: exhibition, revision: 2, etag: etag))
        XCTAssertFalse(capability.matches(workspaceId: UUID().uuidString, tenantId: tenant, principalId: principal, exhibitionId: exhibition, revision: 2, etag: etag))
        XCTAssertFalse(capability.matches(workspaceId: workspace, tenantId: tenant, principalId: principal, exhibitionId: exhibition, revision: 3, etag: etag))
        XCTAssertFalse(capability.matches(workspaceId: workspace, tenantId: tenant, principalId: principal, exhibitionId: exhibition, revision: 2, etag: etag + "x"))
        let old = try JSONDecoder().decode(NativeViewportPolicy.self, from: Data("{\"exhibitionId\":\"\(exhibition)\",\"revision\":2,\"etag\":\"old\",\"boundedReadOnly\":true}".utf8))
        XCTAssertNil(old.boundedProfile); XCTAssertNil(old.editingCapability)
    }
    func testBoundedFrameScopeIsPairedAndLegacyFramesOmitIt() throws {
        let draft = try NativeExhibitionDraft.new(), workspace = UUID().uuidString.lowercased()
        func frame(_ workspace: String?, _ etag: String?) -> ViewportFrame {
            ViewportFrame(epoch: UUID().uuidString, sequence: 1, tenantId: UUID().uuidString,
                principalId: UUID().uuidString, draft: draft, selection: nil, mode: "translate", view: "top", disabled: false,
                workspaceId: workspace, baseETag: etag)
        }
        let legacy = try JSONSerialization.jsonObject(with: JSONEncoder().encode(frame(nil, nil))) as! [String: Any]
        XCTAssertNil(legacy["workspaceId"]); XCTAssertNil(legacy["baseETag"])
        XCTAssertThrowsError(try JSONEncoder().encode(frame(workspace, nil)))
        XCTAssertThrowsError(try JSONEncoder().encode(frame(nil, "etag")))
        XCTAssertThrowsError(try JSONEncoder().encode(frame("not-a-uuid", "etag")))
        let scoped = try JSONSerialization.jsonObject(with: JSONEncoder().encode(frame(workspace, "etag"))) as! [String: Any]
        XCTAssertEqual(scoped["workspaceId"] as? String, workspace); XCTAssertEqual(scoped["baseETag"] as? String, "etag")
    }
    func testBoundedAdmissionRejectsPendingImportAndForeignScope() throws {
        let workspace = UUID().uuidString.lowercased(), tenant = UUID().uuidString.lowercased(), principal = UUID().uuidString.lowercased()
        let draft = try NativeExhibitionDraft.new(), etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
        let document = NativeDocumentState(format: 1, revision: 1, workspaceId: workspace, tenantId: tenant, principalId: principal,
            draft: draft, etag: etag, savedEditVersion: 1, pending: nil)
        let imported = NativeOEXImportState(format: 1, revision: 1, workspaceId: workspace, tenantId: tenant, principalId: principal,
            intent: nil, jobId: nil, fileName: nil)
        let session = AuthoringSession(userId: principal, tenantId: tenant, role: "admin", csrfToken: "test", expiresAt: "test")
        let raw: [String: Any] = ["exhibitionId":draft.id,"revision":2,"etag":etag,"boundedReadOnly":false,"boundedProfile":true,"boundedEditRequired":true,
            "editingCapability":["version":1,"kind":"native-bounded-edit-v1","workspaceId":workspace,"tenantId":tenant,
                                 "principalId":principal,"exhibitionId":draft.id,"revision":2,"etag":etag]]
        let policy = try JSONDecoder().decode(NativeViewportPolicy.self, from: JSONSerialization.data(withJSONObject: raw))
        XCTAssertNotNil(NativeBoundedEditAdmission.admit(policy: policy, document: document, importState: imported, session: session, selectedImport: false))
        XCTAssertNil(NativeBoundedEditAdmission.admit(policy: policy, document: document, importState: imported, session: session, selectedImport: true))
        var pending = document; pending.pending = .init(etag: etag, draft: draft)
        XCTAssertNil(NativeBoundedEditAdmission.admit(policy: policy, document: pending, importState: imported, session: session, selectedImport: false))
        var active = imported; active.jobId = UUID().uuidString
        XCTAssertNil(NativeBoundedEditAdmission.admit(policy: policy, document: document, importState: active, session: session, selectedImport: false))
        let other = AuthoringSession(userId: UUID().uuidString, tenantId: tenant, role: "admin", csrfToken: "test", expiresAt: "test")
        XCTAssertNil(NativeBoundedEditAdmission.admit(policy: policy, document: document, importState: imported, session: other, selectedImport: false))
        var dirty = document, changed = draft; try changed.rename("changed locally"); dirty.draft = changed
        XCTAssertNil(NativeBoundedEditAdmission.admit(policy: policy, document: dirty, importState: imported, session: session, selectedImport: false))
        var old = raw; old.removeValue(forKey: "editingCapability")
        let oldPolicy = try JSONDecoder().decode(NativeViewportPolicy.self, from: JSONSerialization.data(withJSONObject: old))
        XCTAssertNil(NativeBoundedEditAdmission.admit(policy: oldPolicy, document: document, importState: imported, session: session, selectedImport: false))
    }
    func testBoundedAcknowledgementRejectsStaleFrameAndMismatchedScope() throws {
        let draft = try NativeExhibitionDraft.new(), workspace = UUID().uuidString.lowercased()
        let tenant = UUID().uuidString.lowercased(), principal = UUID().uuidString.lowercased(), epoch = UUID().uuidString.lowercased()
        let etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
        let scope: [String: Any] = ["version":1,"kind":"native-bounded-edit-v1","workspaceId":workspace,"tenantId":tenant,
                                    "principalId":principal,"exhibitionId":draft.id,"revision":2,"etag":etag]
        let capability = try JSONDecoder().decode(NativeBoundedEditCapability.self, from: JSONSerialization.data(withJSONObject: scope))
        let response = try JSONDecoder().decode(ViewportAcceptance.self, from: JSONSerialization.data(withJSONObject: ["accepted":true,"boundedReadOnly":false,"editingCapability":scope]))
        let policy = try JSONDecoder().decode(NativeViewportPolicy.self, from: JSONSerialization.data(withJSONObject: ["exhibitionId":draft.id,"revision":2,"etag":etag,"boundedProfile":true,"boundedEditRequired":true,"boundedReadOnly":false,"editingCapability":scope]))
        let frame = ViewportFrame(epoch: epoch, sequence: 2, tenantId: tenant, principalId: principal, draft: draft, selection: nil,
                                  mode: "translate", view: "top", disabled: false, workspaceId: workspace, baseETag: etag)
        let document = NativeDocumentState(format: 1, revision: 1, workspaceId: workspace, tenantId: tenant, principalId: principal,
            draft: draft, etag: etag, savedEditVersion: 1, pending: nil)
        let imported = NativeOEXImportState(format: 1, revision: 1, workspaceId: workspace, tenantId: tenant, principalId: principal,
            intent: nil, jobId: nil, fileName: nil)
        let session = AuthoringSession(userId: principal, tenantId: tenant, role: "admin", csrfToken: "test", expiresAt: "test")
        func ack(_ response: ViewportAcceptance, _ owner: String, _ sequence: Int, _ document: NativeDocumentState) -> Bool {
            NativeBoundedEditAdmission.acknowledges(response, frame: frame, expected: capability, policy: policy, ownerEpoch: owner,
                                                     currentSequence: sequence, document: document, importState: imported,
                                                     session: session, capturedSession: session, firstAdmission: true, selectedImport: false)
        }
        XCTAssertTrue(ack(response, epoch, 2, document))
        XCTAssertFalse(ack(response, UUID().uuidString, 2, document))
        XCTAssertFalse(ack(response, epoch, 3, document))
        var pendingDocument = document
        pendingDocument.pending = .init(etag: etag, draft: draft)
        XCTAssertFalse(ack(response, epoch, 2, pendingDocument))
        var pendingImport = imported
        pendingImport.jobId = UUID().uuidString
        XCTAssertFalse(NativeBoundedEditAdmission.acknowledges(response, frame: frame, expected: capability, policy: policy, ownerEpoch: epoch,
            currentSequence: 2, document: document, importState: pendingImport, session: session, capturedSession: session, firstAdmission: true, selectedImport: false))
        XCTAssertFalse(NativeBoundedEditAdmission.acknowledges(response, frame: frame, expected: capability, policy: policy, ownerEpoch: epoch,
            currentSequence: 2, document: document, importState: imported, session: session, capturedSession: session, firstAdmission: true, selectedImport: true))
        var changed = document, altered = draft; try altered.rename("later edit"); changed.draft = altered
        XCTAssertFalse(ack(response, epoch, 2, changed))
        let dirtyFrame = ViewportFrame(epoch: epoch, sequence: 2, tenantId: tenant, principalId: principal, draft: altered, selection: nil,
            mode: "translate", view: "top", disabled: false, workspaceId: workspace, baseETag: etag)
        XCTAssertFalse(NativeBoundedEditAdmission.acknowledges(response, frame: dirtyFrame, expected: capability, policy: policy, ownerEpoch: epoch,
            currentSequence: 2, document: changed, importState: imported, session: session, capturedSession: session, firstAdmission: true, selectedImport: false))
        let switched = AuthoringSession(userId: principal, tenantId: tenant, role: "admin", csrfToken: "new", expiresAt: "test")
        XCTAssertFalse(NativeBoundedEditAdmission.acknowledges(response, frame: frame, expected: capability, policy: policy, ownerEpoch: epoch,
            currentSequence: 2, document: document, importState: imported, session: switched, capturedSession: session, firstAdmission: true, selectedImport: false))
        var foreignScope = scope; let foreignTenant = UUID().uuidString.lowercased(); foreignScope["tenantId"] = foreignTenant
        let foreignCapability = try JSONDecoder().decode(NativeBoundedEditCapability.self, from: JSONSerialization.data(withJSONObject: foreignScope))
        let foreignResponse = try JSONDecoder().decode(ViewportAcceptance.self, from: JSONSerialization.data(withJSONObject: ["accepted":true,"boundedReadOnly":false,"editingCapability":foreignScope]))
        let foreignPolicy = try JSONDecoder().decode(NativeViewportPolicy.self, from: JSONSerialization.data(withJSONObject: ["exhibitionId":draft.id,"revision":2,"etag":etag,"boundedProfile":true,"boundedEditRequired":true,"boundedReadOnly":false,"editingCapability":foreignScope]))
        let foreignFrame = ViewportFrame(epoch: epoch, sequence: 2, tenantId: foreignTenant, principalId: principal, draft: draft, selection: nil,
            mode: "translate", view: "top", disabled: false, workspaceId: workspace, baseETag: etag)
        XCTAssertFalse(NativeBoundedEditAdmission.acknowledges(foreignResponse, frame: foreignFrame, expected: foreignCapability, policy: foreignPolicy, ownerEpoch: epoch,
            currentSequence: 2, document: document, importState: imported, session: session, capturedSession: session, firstAdmission: true, selectedImport: false))
        let old = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"boundedReadOnly\":false}".utf8))
        XCTAssertFalse(ack(old, epoch, 2, document))
    }
    func testBoundedCandidateChangesOnlyTheSelectedPlacementTransform() throws {
        var draft = try NativeExhibitionDraft.new()
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(UUID().uuidString),
            "primaryAssetId": .string(UUID().uuidString), "metadata": .object(["title": .string("합성")])])
        let room = draft.rooms[0]["id"].string!, first = UUID().uuidString, second = UUID().uuidString
        try draft.addApprovedArtwork(artwork, roomId: room, placementId: first, transform: NativeExhibitionDraft.identity)
        try draft.addApprovedArtwork(artwork, roomId: room, placementId: second, transform: NativeExhibitionDraft.identity)
        let selection = ViewportSelection(kind: "placement", id: first)
        var candidate = draft.candidate, rows = candidate["placements"].items!
        rows[0]["transform"]["position"] = .vector([1, 2, 3]); candidate["placements"] = .array(rows)
        var good = draft; try good.acceptBoundedPlacement(candidate, selection: selection, savedBaseline: draft)
        XCTAssertEqual(good.placements[0]["transform"]["position"], .vector([1, 2, 3]))
        var next = good.candidate, nextRows = next["placements"].items!
        nextRows[0]["transform"]["position"] = .vector([2, 3, 4]); next["placements"] = .array(nextRows)
        var repeated = good
        try repeated.acceptBoundedPlacement(next, selection: selection, savedBaseline: draft)
        XCTAssertEqual(repeated.placements[0]["transform"]["position"], .vector([2, 3, 4]))
        var later = good.candidate, laterRows = later["placements"].items!
        laterRows[1]["transform"]["position"] = .vector([4, 5, 6]); later["placements"] = .array(laterRows)
        var intermediate = good
        XCTAssertThrowsError(try intermediate.acceptBoundedPlacement(later, selection: ViewportSelection(kind: "placement", id: second), savedBaseline: draft))
        try intermediate.acceptBoundedPlacement(draft.candidate, selection: selection, savedBaseline: draft)
        XCTAssertEqual(intermediate.candidate, draft.candidate)
        var secondOnly = draft.candidate, secondRows = secondOnly["placements"].items!
        secondRows[1]["transform"]["position"] = .vector([4, 5, 6]); secondOnly["placements"] = .array(secondRows)
        try intermediate.acceptBoundedPlacement(secondOnly, selection: ViewportSelection(kind: "placement", id: second), savedBaseline: draft)
        var wrong = candidate; wrong["title"] = .string("unexpected")
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(wrong, selection: selection, savedBaseline: draft))
        var other = candidate, both = other["placements"].items!; both[1]["transform"]["position"] = .vector([4, 5, 6]); other["placements"] = .array(both)
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(other, selection: selection, savedBaseline: draft))
        var changedBinding = candidate, changedRows = changedBinding["placements"].items!; changedRows[0]["roomId"] = .string(UUID().uuidString); changedBinding["placements"] = .array(changedRows)
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(changedBinding, selection: selection, savedBaseline: draft))
        var reordered = candidate; reordered["placements"] = .array(Array(rows.reversed()))
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(reordered, selection: selection, savedBaseline: draft))
        var malformed = candidate, badRows = malformed["placements"].items!
        badRows[0]["transform"]["scale"] = .vector([1, 1]); malformed["placements"] = .array(badRows)
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(malformed, selection: selection, savedBaseline: draft))
        badRows = candidate["placements"].items!; badRows[0]["transform"]["extra"] = .bool(true); malformed["placements"] = .array(badRows)
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(malformed, selection: selection, savedBaseline: draft))
        badRows = candidate["placements"].items!; badRows[0]["transform"]["position"] = .array([.number(.nan), .number(0), .number(0)]); malformed["placements"] = .array(badRows)
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(malformed, selection: selection, savedBaseline: draft))
        XCTAssertThrowsError(try draft.acceptBoundedPlacement(candidate, selection: ViewportSelection(kind: "room", id: room), savedBaseline: draft))
    }
    func testGeometryProposalPreservesIdentityRightsAndOpaqueFields() throws {
        var draft = try NativeExhibitionDraft.new()
        let original = draft
        var geometry = draft.candidate
        var rooms = geometry["rooms"].items!; rooms[0]["name"] = .string("바꾼 방")
        geometry["rooms"] = .array(rooms)
        try draft.acceptGeometry(geometry)
        XCTAssertEqual(draft.rooms[0]["name"].string, "바꾼 방")
        XCTAssertEqual(draft.id, original.id)
        XCTAssertEqual(draft.document["editVersion"].integer, 2)
        let accepted = draft
        for key in ["id","artworks","extensions","accessibility","title","mediaAssets","scripts"] {
            var wrong = geometry; wrong[key] = .string("untrusted replacement")
            XCTAssertThrowsError(try draft.acceptGeometry(wrong)); XCTAssertEqual(draft, accepted)
        }
        try draft.replaceCandidate(original.candidate)
        XCTAssertEqual(draft.candidate, original.candidate)
        XCTAssertEqual(draft.document["editVersion"].integer, 3)
    }
    func testFrameRetainsNullSelectionAndRequiresRealSelectedItem() throws {
        let draft = try NativeExhibitionDraft.new(), epoch = UUID().uuidString
        let frame = ViewportFrame(epoch: epoch, sequence: 1, tenantId: UUID().uuidString, principalId: UUID().uuidString, draft: draft, selection: nil, mode: "translate", view: "top", disabled: false)
        let object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(frame)) as! [String: Any]
        XCTAssertTrue(object["selection"] is NSNull)
        XCTAssertEqual(object.count, 12)
        XCTAssertEqual(object["cameraSequence"] as? Int, 0)
        XCTAssertEqual(object["transparent"] as? Bool, true)
        XCTAssertTrue(ViewportSelection(kind: "room", id: draft.rooms[0]["id"].string!).belongs(to: draft))
        XCTAssertFalse(ViewportSelection(kind: "placement", id: draft.rooms[0]["id"].string!).belongs(to: draft))
        XCTAssertFalse(ViewportSelection(kind: "arbitrary", id: UUID().uuidString).belongs(to: draft))
    }
    func testPlanPresentationAddsOnlyItsBoundedOptionalField() throws {
        let draft = try NativeExhibitionDraft.new()
        let selection = ViewportSelection(kind: "room", id: draft.rooms[0]["id"].string!)
        let epoch = UUID().uuidString, tenant = UUID().uuidString, principal = UUID().uuidString
        func encoded(_ presentation: ViewportPresentation) throws -> [String: Any] {
            let frame = ViewportFrame(epoch: epoch, sequence: 7, tenantId: tenant, principalId: principal,
                draft: draft, selection: selection, mode: "dimensions", view: "front", disabled: true,
                cameraSequence: 9, transparent: false, presentation: presentation)
            return try JSONSerialization.jsonObject(with: JSONEncoder().encode(frame)) as! [String: Any]
        }
        let scene = try encoded(.threeD)
        var plan = try encoded(.plan)
        XCTAssertEqual(scene.count, 12)
        XCTAssertNil(scene["presentation"])
        XCTAssertEqual(plan.removeValue(forKey: "presentation") as? String, "plan")
        XCTAssertEqual(try JSONSerialization.data(withJSONObject: scene, options: .sortedKeys),
                       try JSONSerialization.data(withJSONObject: plan, options: .sortedKeys))
    }
    func testPlanCapabilityRequiresExplicitAcceptedSupportAndKeepsLegacyCompatible() throws {
        func response(_ text: String) throws -> ViewportAcceptance {
            try JSONDecoder().decode(ViewportAcceptance.self, from: Data(text.utf8))
        }
        XCTAssertFalse(try response(#"{"accepted":true}"#).supportsPlan)
        XCTAssertFalse(try response(#"{"accepted":false,"presentations":["3d","plan"]}"#).supportsPlan)
        XCTAssertFalse(try response(#"{"accepted":true,"presentations":["3d"]}"#).supportsPlan)
        XCTAssertTrue(try response(#"{"accepted":true,"presentations":["3d","plan"]}"#).supportsPlan)
        XCTAssertThrowsError(try response(#"{"accepted":true,"presentations":["arbitrary"]}"#))
    }

    func testBoundedReadOnlyAcknowledgementDistinguishesMissingFromFalse() throws {
        func decode(_ text: String) throws -> ViewportAcceptance {
            try JSONDecoder().decode(ViewportAcceptance.self, from: Data(text.utf8))
        }
        XCTAssertNil(try decode(#"{"accepted":true}"#).boundedReadOnly)
        XCTAssertEqual(try decode(#"{"accepted":true,"boundedReadOnly":false}"#).boundedReadOnly, false)
        XCTAssertEqual(try decode(#"{"accepted":true,"boundedReadOnly":true}"#).boundedReadOnly, true)
        XCTAssertThrowsError(try decode(#"{"accepted":true,"boundedReadOnly":"false"}"#))
    }

    func testSelectedSavedFrameAndPendingRetryFailClosed() {
        XCTAssertTrue(NativeViewportAccess.locked(selected: true, saved: true, storedLocked: false, frameReadOnly: false, exactOrdinaryFrame: false))
        XCTAssertFalse(NativeViewportAccess.locked(selected: true, saved: true, storedLocked: false, frameReadOnly: false, exactOrdinaryFrame: true))
        XCTAssertTrue(NativeViewportAccess.locked(selected: true, saved: true, storedLocked: false, frameReadOnly: true, exactOrdinaryFrame: true))
        XCTAssertTrue(NativeViewportAccess.locked(selected: true, saved: true, storedLocked: true, frameReadOnly: false, exactOrdinaryFrame: true))
        XCTAssertFalse(NativeViewportAccess.locked(selected: false, saved: true, storedLocked: false, frameReadOnly: false, exactOrdinaryFrame: false))
        XCTAssertFalse(NativeViewportAccess.locked(selected: true, saved: false, storedLocked: false, frameReadOnly: false, exactOrdinaryFrame: false))
        XCTAssertTrue(NativeViewportAccess.blocksPendingReplay(selected: true, pending: true))
        XCTAssertFalse(NativeViewportAccess.blocksPendingReplay(selected: false, pending: true))
        XCTAssertTrue(NativeViewportAccess.blocksPendingReplay(selected: false, pending: true, origin: "bounded-placement-v1"))
        XCTAssertTrue(NativeViewportAccess.blocksPendingReplay(selected: false, pending: true, origin: "future-unrecognized-origin"))
        XCTAssertFalse(NativeViewportAccess.blocksPendingReplay(selected: false, pending: false, origin: "bounded-placement-v1"))
        XCTAssertFalse(NativeViewportAccess.blocksPendingReplay(selected: true, pending: false))
    }

}
