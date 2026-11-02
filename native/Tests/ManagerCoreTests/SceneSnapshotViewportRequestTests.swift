// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class SceneSnapshotViewportRequestTests: XCTestCase {
    private func fixture(disabled: Bool = false) throws -> SceneSnapshotViewportRequest {
        var draft = try NativeExhibitionDraft.new(), c = draft.candidate
        let room = draft.rooms[0]["id"], placement = UUID().uuidString.lowercased(), revision = UUID().uuidString.lowercased(), asset = UUID().uuidString.lowercased(), view = UUID().uuidString.lowercased()
        c["artworks"] = .array([.object(["revisionId": .string(revision), "assets": .array([.object(["id": .string(asset)])]), "rights": .object(["permissions": .object(["display": .bool(true), "export": .bool(true)])])])])
        c["placements"] = .array([.object(["id": .string(placement), "roomId": room, "artworkRevisionId": .string(revision)])])
        c["extensions"] = .object(["org.exhibitos.studio/presentation": .object(["version": .integer(1), "viewpoints": .array([.object(["id": .string(view), "roomId": room, "position": .vector([0, 1.2, 2]), "target": .vector([0, 1.2, 0]), "fov": .number(50)])])])])
        try draft.replaceCandidate(c)
        let saved = NativeStudioResponse(revision: 9, etag: "\"studio-r9-" + String(repeating: "a", count: 64) + "\"", draft: draft)
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 12, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "front", disabled: disabled, transparent: false)
        return try SceneSnapshotViewportRequest(frame: frame, saved: saved, viewpointId: view, placementId: placement)
    }
    private func reply(_ q: SceneSnapshotViewportRequest) -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "expectedETag", "width", "height"] { fields[key] = q.value[key] }
        // This unit fixture exercises the bounded header/binding layer; actual
        // ImageIO decoding is separately required at the app/real PNG boundary.
        var png = Data([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,2,128,0,0,1,104]); png.append(Data(repeating: 0, count: 9))
        fields["pngBase64"] = .string(png.base64EncodedString()); fields["candidateSha256"] = .string(String(repeating: "b", count: 64))
        fields["saved"] = .object(["exhibitionId": .string(q.saved.draft.id), "draftId": q.saved.draft.document["id"], "revision": .integer(9), "etag": .string(q.saved.etag)])
        fields["camera"] = .object(["viewpointId": q.value["viewpointId"], "roomId": q.placement["roomId"], "position": .vector([10, 1.2, 2]), "target": .vector([10, 1.2, 0]), "fov": .number(50)])
        fields["target"] = .object(["placementId": q.value["placementId"], "artworkRevisionId": q.placement["artworkRevisionId"], "assetId": q.artwork["assets"].items![0]["id"], "ready": .bool(true), "inFrustum": .bool(true), "centerRay": .string("unknown")])
        return .object(fields)
    }
    func testSavedBindingWorldCameraAndUnknownVisibilityRemainExplicit() throws {
        let q = try fixture(), before = q.frame.draft
        let result = try q.checkedReply(reply(q))
        XCTAssertEqual(result.localEditVersion, before.document["editVersion"].integer)
        XCTAssertEqual(result.savedRevision, 9); XCTAssertEqual(result.target["centerRay"].string, "unknown")
        XCTAssertEqual(result.camera["position"], .vector([10, 1.2, 2]))
        XCTAssertEqual(q.frame.draft, before)
        let legacy = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true}".utf8))
        XCTAssertFalse(legacy.supportsSceneSnapshot)
        let current = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"scene-snapshot-v1\"]}".utf8))
        XCTAssertTrue(current.supportsSceneSnapshot)
        XCTAssertThrowsError(try fixture(disabled: true))
    }
    func testStaleSourceWrongTargetAndUnreadyOrOpenReplyAreRejected() throws {
        let q = try fixture(), good = reply(q)
        for key in ["epoch", "sequence", "principalId", "requestId", "expectedETag", "width"] {
            var bad = good; bad[key] = .string("stale")
            XCTAssertThrowsError(try q.checkedReply(bad), key)
        }
        var bad = good; bad["saved"]["revision"] = .integer(10); XCTAssertThrowsError(try q.checkedReply(bad))
        bad = good; bad["saved"]["draftId"] = .string(UUID().uuidString.lowercased()); XCTAssertThrowsError(try q.checkedReply(bad))
        bad = good; bad["target"]["assetId"] = .string(UUID().uuidString.lowercased()); XCTAssertThrowsError(try q.checkedReply(bad))
        bad = good; bad["target"]["ready"] = .bool(false); XCTAssertThrowsError(try q.checkedReply(bad))
        bad = good; bad["target"]["centerRay"] = .string("fully-visible"); XCTAssertThrowsError(try q.checkedReply(bad))
        bad = good; bad["camera"]["extra"] = .integer(1); XCTAssertThrowsError(try q.checkedReply(bad))
        bad = good; bad["extra"] = .integer(1); XCTAssertThrowsError(try q.checkedReply(bad))
        bad = good; bad["pngBase64"] = .string(String(repeating: "A", count: SceneSnapshotViewportRequest.maximumBase64Bytes + 1)); XCTAssertThrowsError(try q.checkedReply(bad))
    }
    func testTransparentFrameRejectedWithoutChangingItAndUnplacedArtworkDoesNotBlock() throws {
        let q = try fixture(), f = q.frame
        let transparent = ViewportFrame(epoch: f.epoch, sequence: f.sequence, tenantId: f.tenantId, principalId: f.principalId, draft: f.draft, selection: nil, mode: f.mode, view: f.view, disabled: false, transparent: true)
        XCTAssertThrowsError(try SceneSnapshotViewportRequest(frame: transparent, saved: q.saved, viewpointId: q.value["viewpointId"].string!, placementId: q.value["placementId"].string!))
        XCTAssertTrue(transparent.transparent)
        var draft = f.draft, candidate = draft.candidate
        candidate["artworks"] = .array(candidate["artworks"].items! + [.object(["revisionId": .string(UUID().uuidString.lowercased()), "rights": .object(["permissions": .object(["display": .bool(false), "export": .bool(false)])])])])
        try draft.replaceCandidate(candidate)
        let frame = ViewportFrame(epoch: f.epoch, sequence: f.sequence + 1, tenantId: f.tenantId, principalId: f.principalId, draft: draft, selection: nil, mode: f.mode, view: f.view, disabled: false, transparent: false)
        let saved = NativeStudioResponse(revision: 9, etag: q.saved.etag, draft: draft)
        XCTAssertNoThrow(try SceneSnapshotViewportRequest(frame: frame, saved: saved, viewpointId: q.value["viewpointId"].string!, placementId: q.value["placementId"].string!))
    }
    @MainActor
    func testOutputCommitsNewFileAndPreservesExistingDestinationOnFailure() async throws {
        let q = try fixture(), result = try q.checkedReply(reply(q))
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let destination = directory.appendingPathComponent("scene.png")
        try await result.writeNewPNG(to: destination)
        XCTAssertEqual(try Data(contentsOf: destination), result.png)
        let prior = Data("preserve me".utf8), existing = directory.appendingPathComponent("existing.png")
        try prior.write(to: existing)
        do { try await result.writeNewPNG(to: existing); XCTFail("Existing output must not be replaced") } catch {}
        XCTAssertEqual(try Data(contentsOf: existing), prior)
        XCTAssertEqual(Set(try FileManager.default.contentsOfDirectory(atPath: directory.path)), Set(["scene.png", "existing.png"]))
    }
}
