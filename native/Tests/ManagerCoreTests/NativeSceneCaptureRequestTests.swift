// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeSceneCaptureRequestTests: XCTestCase {
    private let owner = "11111111-1111-4111-8111-111111111111"
    private let placement = "22222222-2222-4222-8222-222222222222"
    private let viewpoint = "33333333-3333-4333-8333-333333333333"
    private let artwork = "44444444-4444-4444-8444-444444444444"
    private let asset = "55555555-5555-4555-8555-555555555555"
    private let anotherRoom = "66666666-6666-4666-8666-666666666666"
    private let etag = "\"studio-r7-" + String(repeating: "a", count: 64) + "\""

    private func fixture() throws -> (NativeDocumentState, [String: Any]) {
        var draft = try NativeExhibitionDraft.new(now: Date(timeIntervalSince1970: 1000)), candidate = draft.candidate
        let room = draft.rooms[0]["id"]
        candidate["placements"] = .array([.object(["id": .string(placement), "roomId": room,
            "artworkRevisionId": .string(artwork), "assetId": .string(asset)])])
        candidate["artworks"] = .array([.object(["revisionId": .string(artwork), "primaryAssetId": .string(asset)])])
        candidate["extensions"] = .object(["org.exhibitos.studio/presentation": .object([
            "version": .integer(1), "credits": .string(""), "viewpoints": .array([.object([
                "id": .string(viewpoint), "name": .string("Synthetic viewpoint"), "roomId": room,
                "position": .vector([0,1,2]), "target": .vector([0,1,0]), "fov": .number(50)])])])])
        try draft.replaceCandidate(candidate)
        let state = NativeDocumentState(format: 1, revision: 3, workspaceId: owner, tenantId: owner, principalId: owner,
            draft: draft, etag: etag, savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let request: [String: Any] = ["version": 1, "requestId": owner, "workspaceId": owner, "tenantId": owner,
            "principalId": owner, "runtimeManifestSHA256": String(repeating: "a", count: 64), "expectedNativeRevision": 3,
            "expectedEditVersion": draft.document["editVersion"].integer!, "exhibitionId": draft.id, "serverRevision": 7,
            "expectedETag": etag, "placementId": placement, "viewpointId": viewpoint, "outputDirectory": "/synthetic/capture"]
        return (state, request)
    }
    private func parse(_ fields: [String: Any]) throws -> NativeSceneCaptureRequest {
        try JSONDecoder().decode(NativeSceneCaptureRequest.self, from: JSONSerialization.data(withJSONObject: fields))
    }
    private func invalidRequest(_ fields: [String: Any], file: StaticString = #filePath, line: UInt = #line) {
        do { _ = try parse(fields); XCTFail("Invalid request accepted", file: file, line: line) }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "NATIVE_SCENE_CAPTURE_REQUEST_INVALID", file: file, line: line) }
        catch { XCTFail("Unexpected error: \(error)", file: file, line: line) }
    }
    private func stateChanged(_ request: NativeSceneCaptureRequest, _ state: NativeDocumentState, saved: NativeStudioResponse? = nil,
                              file: StaticString = #filePath, line: UInt = #line) {
        do { try request.validate(document: state, saved: saved); XCTFail("Changed state accepted", file: file, line: line) }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "NATIVE_SCENE_CAPTURE_STATE_CHANGED", file: file, line: line) }
        catch { XCTFail("Unexpected error: \(error)", file: file, line: line) }
    }

    func testClosedRequestAcceptsMatchingNamedViewAndSavedHead() throws {
        let (state, fields) = try fixture(), request = try parse(fields), draft = try XCTUnwrap(state.draft)
        XCTAssertEqual(request.outputDirectory, "/synthetic/capture")
        try request.validate(document: state)
        try request.validate(document: state, saved: NativeStudioResponse(revision: 7, etag: etag, draft: draft))
    }
    func testInvalidShapeTypesBoundsAndCanonicalPath() throws {
        let (_, valid) = try fixture()
        for key in valid.keys { var bad = valid; bad.removeValue(forKey: key); invalidRequest(bad) }
        var extra = valid; extra["candidate"] = [:]; invalidRequest(extra)
        let invalid: [(String, Any)] = [("version", 2), ("requestId", "bad"), ("runtimeManifestSHA256", String(repeating: "A", count: 64)),
            ("expectedNativeRevision", 0), ("expectedNativeRevision", -1), ("expectedNativeRevision", UInt64(Int64.max) + 1),
            ("expectedNativeRevision", true), ("expectedEditVersion", 0),
            ("serverRevision", 0), ("serverRevision", Int64(Int32.max) + 1), ("expectedETag", "\"studio-r8-" + String(repeating: "a", count: 64) + "\""),
            ("placementId", 123), ("viewpointId", "missing"), ("outputDirectory", "relative"),
            ("outputDirectory", "/a/../b"), ("outputDirectory", "/a/./b"), ("outputDirectory", "/a//b"),
            ("outputDirectory", "/a/"), ("outputDirectory", "/a\u{0000}b")]
        for (key, value) in invalid { var bad = valid; bad[key] = value; invalidRequest(bad) }
    }
    func testOwnerRevisionPendingAndSavedComparisonFailClosed() throws {
        let (state, fields) = try fixture(), request = try parse(fields), draft = try XCTUnwrap(state.draft)
        let foreign = NativeDocumentState(format: 1, revision: state.revision, workspaceId: anotherRoom, tenantId: owner,
            principalId: owner, draft: draft, etag: etag, savedEditVersion: state.savedEditVersion, pending: nil)
        stateChanged(request, foreign)
        var changed = state; changed.revision += 1; stateChanged(request, changed)
        changed = state; changed.savedEditVersion = 1; stateChanged(request, changed)
        changed = state; changed.etag = "\"studio-r8-" + String(repeating: "a", count: 64) + "\""; stateChanged(request, changed)
        changed = state; changed.pending = .init(etag: etag, draft: draft); stateChanged(request, changed)
        stateChanged(request, state, saved: NativeStudioResponse(revision: 8, etag: etag, draft: draft))
        stateChanged(request, state, saved: NativeStudioResponse(revision: 7, etag: "other", draft: draft))
        var changedDraft = draft, candidate = draft.candidate
        candidate["title"] = .string("changed"); try changedDraft.replaceCandidate(candidate)
        stateChanged(request, state, saved: NativeStudioResponse(revision: 7, etag: etag, draft: changedDraft))
    }
    func testViewpointAndArtworkMustExistInSameRoom() throws {
        let (state, fields) = try fixture(), request = try parse(fields), draft = try XCTUnwrap(state.draft)
        for variant in 0..<4 {
            var candidate = draft.candidate
            switch variant {
            case 0: candidate["placements"] = .array([])
            case 1: candidate["artworks"] = .array([])
            case 2: candidate["extensions"]["org.exhibitos.studio/presentation"]["viewpoints"] = .array([])
            default:
                var rooms = draft.rooms, room = rooms[0]; room["id"] = .string(anotherRoom); rooms.append(room); candidate["rooms"] = .array(rooms)
                var views = candidate["extensions"]["org.exhibitos.studio/presentation"]["viewpoints"].items!
                views[0]["roomId"] = .string(anotherRoom)
                candidate["extensions"]["org.exhibitos.studio/presentation"]["viewpoints"] = .array(views)
            }
            var changedDraft = draft; try changedDraft.replaceCandidate(candidate)
            var changed = state; changed.draft = changedDraft; changed.savedEditVersion = changedDraft.document["editVersion"].integer
            var updated = fields; updated["expectedEditVersion"] = changed.savedEditVersion!
            stateChanged(try parse(updated), changed)
        }
        var duplicate = draft.candidate
        duplicate["placements"] = .array([draft.placements[0], draft.placements[0]])
        var changedDraft = draft; try changedDraft.replaceCandidate(duplicate)
        var changed = state; changed.draft = changedDraft; changed.savedEditVersion = changedDraft.document["editVersion"].integer
        var updated = fields; updated["expectedEditVersion"] = changed.savedEditVersion!
        stateChanged(try parse(updated), changed)
    }
}
