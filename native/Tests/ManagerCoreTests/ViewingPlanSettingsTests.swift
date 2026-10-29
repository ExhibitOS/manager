// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class ViewingPlanSettingsTests: XCTestCase {
    private let placement = "10000000-0000-4000-8000-00000000000a"
    private let viewpoint = "20000000-0000-4000-8000-00000000000b"
    private let revision = "30000000-0000-4000-8000-00000000000c"
    private let ns = ViewingPlanSettings.namespace
    private func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let view = try BenchViewpoint(id: viewpoint, name: "Authored", roomId: room, position: [1, 1.6, 2], target: [0, 1.4, 0], fov: 60)
        candidate["artworks"] = .array([.object(["revisionId": .string(revision), "artworkType": .string("sculpture")])])
        candidate["placements"] = .array([.object(["id": .string(placement), "artworkRevisionId": .string(revision)])])
        candidate["extensions"] = .object([BenchSettings.presentationNamespace: .object(["version": .integer(1), "viewpoints": .array([view.value]), "credits": .string("keep")]),
                                           "org.exhibitos.studio/template": .object(["modified": .bool(false)]),
                                           "example.org/opaque": .object(["keep": .integer(9)])])
        try draft.replaceCandidate(candidate); return draft
    }
    private func plan(orbit: DocumentValue? = nil) -> DocumentValue {
        var stop: [String: DocumentValue] = ["placementId": .string(placement), "viewpointId": .string(viewpoint)]
        if let orbit { stop["orbit"] = orbit }
        return .object(["version": .integer(1), "stops": .array([.object(stop)])])
    }
    func testOpeningAbsenceAndEmptyPlanDoNotGenerateViewsOrReorderPlacements() throws {
        let base = try fixture(), fields = try ViewingPlanFormBuffer(draft: base)
        XCTAssertFalse(fields.usesPlan); XCTAssertTrue(fields.stops.isEmpty)
        XCTAssertEqual(try fields.command(in: base).proposedCandidate(in: base), base.candidate)
        let empty = try ViewingPlanSettings(value: .object(["version": .integer(1), "stops": .array([])]), in: base)
        let candidate = try ViewingPlanCommand(plan: empty).proposedCandidate(in: base)
        for key in ["rooms", "placements", "artworks"] { XCTAssertEqual(candidate[key], base.candidate[key]) }
        for key in [BenchSettings.presentationNamespace, "org.exhibitos.studio/template", "example.org/opaque"] {
            XCTAssertEqual(candidate["extensions"][key], base.candidate["extensions"][key])
        }
    }
    func testReferencesCaseCanonicalizationAndInvalidRawInputRemainNonmutating() throws {
        let base = try fixture(); var fields = try ViewingPlanFormBuffer(draft: base)
        fields.append(placementId: placement.uppercased()); fields.append(placementId: placement)
        XCTAssertEqual(fields.stops.count, 1)
        XCTAssertThrowsError(try fields.command(in: base), "A viewpoint is never invented")
        fields.stops[0].viewpointId = viewpoint.uppercased()
        let command = try fields.command(in: base)
        XCTAssertEqual(command.plan?.stops.first?["placementId"].string, placement)
        XCTAssertEqual(command.plan?.stops.first?["viewpointId"].string, viewpoint)
        fields.stops[0].usesOrbit = true; fields.stops[0].minimumDegrees = "-"
        XCTAssertThrowsError(try fields.command(in: base)); XCTAssertEqual(fields.stops[0].minimumDegrees, "-")
        XCTAssertEqual(base.candidate["extensions"][ns], .null)
    }
    func testStrictProfileDuplicateReferencesOrbitBoundsAndSculptureOnly() throws {
        let base = try fixture()
        let good = plan(orbit: .object(["minYaw": .number(-.pi), "maxYaw": .number(.pi)]))
        _ = try ViewingPlanSettings(value: good, in: base)
        var bad = good; bad["visitorProgress"] = .integer(1)
        XCTAssertThrowsError(try ViewingPlanSettings(value: bad, in: base))
        bad = good; bad["stops"] = .array([good["stops"].items![0], good["stops"].items![0]])
        XCTAssertThrowsError(try ViewingPlanSettings(value: bad, in: base))
        for bounds in [[0.0, 0.0], [-Double.pi - 0.01, 0], [0, Double.pi + 0.01], [1, 2], [-2, -1]] {
            XCTAssertThrowsError(try ViewingPlanSettings(value: plan(orbit: .object(["minYaw": .number(bounds[0]), "maxYaw": .number(bounds[1])])), in: base))
        }
        var image = base, candidate = base.candidate
        candidate["artworks"] = .array([.object(["revisionId": .string(revision), "artworkType": .string("image")])])
        try image.replaceCandidate(candidate)
        XCTAssertThrowsError(try ViewingPlanSettings(value: good, in: image))
        _ = try ViewingPlanSettings(value: plan(), in: image)
    }
    func testApplyUndoReopenPreserveExactOrbitAndOpaqueData() throws {
        let base = try fixture(), raw = plan(orbit: .object(["minYaw": .number(-0.123456789012345), "maxYaw": .number(0.876543210987654)]))
        let command = ViewingPlanCommand(plan: try ViewingPlanSettings(value: raw, in: base))
        let candidate = try command.proposedCandidate(in: base)
        let preview = try ViewingPlanPreview(candidate: candidate, base: base, command: command)
        var next = try preview.apply(to: base, command: command)
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(next))
        let unchanged = try ViewingPlanFormBuffer(draft: reopened).command(in: reopened)
        XCTAssertEqual(unchanged.plan?.value, raw, "Degree display must not round-trip untouched radians")
        let removed = try ViewingPlanCommand(plan: nil).proposedCandidate(in: reopened)
        guard case .object(let extensions) = removed["extensions"] else { return XCTFail("container") }
        XCTAssertNil(extensions[ns]); XCTAssertEqual(removed, base.candidate)
        try next.replaceCandidate(base.candidate)
        XCTAssertEqual(next.candidate, base.candidate)
    }
    func testBoundReplyAndPinnedApplyRefuseStaleScopeAndUnrelatedChanges() throws {
        let base = try fixture(), command = ViewingPlanCommand(plan: try ViewingPlanSettings(value: plan(), in: base))
        let frame = ViewportFrame(epoch: UUID().uuidString, sequence: 7, tenantId: UUID().uuidString, principalId: UUID().uuidString, draft: base, selection: nil, mode: "translate", view: "isometric", disabled: false)
        let request = try ViewingPlanViewportRequest(frame: frame, command: command)
        guard case .object(var fields) = request.value else { return XCTFail("request") }
        XCTAssertEqual(Set(fields.keys), Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "type", "plan"]))
        fields.removeValue(forKey: "type"); fields.removeValue(forKey: "plan")
        fields["candidate"] = try command.proposedCandidate(in: base)
        let reply = DocumentValue.object(fields), preview = try request.checkedReply(reply)
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] {
            var changed = reply; changed[key] = .string("stale")
            XCTAssertThrowsError(try request.checkedReply(changed))
        }
        var changed = reply; changed["accepted"] = .bool(true)
        XCTAssertThrowsError(try request.checkedReply(changed))
        changed = reply; changed["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try request.checkedReply(changed))
        var later = base; try later.rename("later")
        XCTAssertThrowsError(try preview.apply(to: later, command: command))
        XCTAssertThrowsError(try preview.apply(to: base, command: ViewingPlanCommand(plan: nil)))
    }
    func testSaveIntentAndPublicationCopiesRetainPlanReferencesAndExactRequestIdentity() throws {
        var source = try fixture()
        let raw = plan(orbit: .object(["minYaw": .number(-0.123456789012345), "maxYaw": .number(0.876543210987654)]))
        let command = ViewingPlanCommand(plan: try ViewingPlanSettings(value: raw, in: source))
        try source.replaceCandidate(command.proposedCandidate(in: source))
        let original = source, etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
        let intent = NativeDocumentState.Pending(etag: etag, draft: source)
        let reopened = try JSONDecoder().decode(NativeDocumentState.Pending.self, from: JSONEncoder().encode(intent))
        XCTAssertEqual(reopened.requestId, intent.requestId); XCTAssertEqual(reopened.etag, etag)
        XCTAssertEqual(reopened.draft, source)
        XCTAssertEqual(reopened.draft.candidate["extensions"][ns], raw)
        let pending = try NativePublicationPending(source: source, etag: etag)
        let restored = try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(pending))
        XCTAssertEqual(restored, pending, "Uncertain publication retries must reuse the exact snapshot and both request IDs")
        for copy in [restored.snapshot, try source.copyForEditing()] {
            XCTAssertNotEqual(copy.id, source.id)
            XCTAssertNotEqual(copy.candidate["revisionId"], source.candidate["revisionId"])
            XCTAssertEqual(copy.candidate["extensions"][ns], raw)
            XCTAssertEqual(try ViewingPlanSettings.read(in: copy)?.value, raw)
            for key in ["placements", "artworks", "rooms", "extensions"] { XCTAssertEqual(copy.candidate[key], source.candidate[key]) }
        }
        XCTAssertEqual(source, original, "Preparing a saved/publication copy never edits the authoring original")
    }
}
