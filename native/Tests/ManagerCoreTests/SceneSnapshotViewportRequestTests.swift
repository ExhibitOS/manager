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
        XCTAssertNil(result.furnishings)
        XCTAssertEqual(q.frame.draft, before)
        let legacy = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true}".utf8))
        XCTAssertFalse(legacy.supportsSceneSnapshot)
        let current = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"scene-snapshot-v1\"]}".utf8))
        XCTAssertTrue(current.supportsSceneSnapshot)
        XCTAssertThrowsError(try fixture(disabled: true))
    }
    func testOptionalFurnishingEvidenceIsClosedAndGraphBoundWithoutChangingLegacyReplies() throws {
        let q = try fixture()
        var value = reply(q)
        let empty: DocumentValue = .object(["catalog": .array([]), "workspace": .array([])])
        value["furnishings"] = empty
        XCTAssertEqual(try q.checkedReply(value).furnishings, empty)
        value["furnishings"] = .null
        XCTAssertThrowsError(try q.checkedReply(value))
        value["furnishings"] = .object(["catalog": .array([]), "workspace": .array([]), "ready": .bool(true)])
        XCTAssertThrowsError(try q.checkedReply(value))
    }
    func testOnlyExactPreEncodingWebKitNotReadyExceptionIsTransient() {
        func error(_ domain: String = "WKErrorDomain", _ code: Int = 4, _ message: String?) -> NSError {
            NSError(domain: domain, code: code, userInfo: message.map { ["WKJavaScriptExceptionMessage": $0] } ?? [:])
        }
        XCTAssertTrue(SceneSnapshotViewportRequest.isNotReadyJavaScriptError(error("WKErrorDomain", 4, "SCENE_SNAPSHOT_NOT_READY")))
        XCTAssertTrue(SceneSnapshotViewportRequest.isNotReadyJavaScriptError(error("WKErrorDomain", 4, "Error: SCENE_SNAPSHOT_NOT_READY")))
        for message in [nil, "SCENE_SNAPSHOT_STALE", "SCENE_SNAPSHOT_UNAVAILABLE", "Error: SCENE_SNAPSHOT_STALE",
                        "Error: SCENE_SNAPSHOT_UNAVAILABLE", "TypeError: SCENE_SNAPSHOT_NOT_READY", "prefix SCENE_SNAPSHOT_NOT_READY",
                        "Error: SCENE_SNAPSHOT_NOT_READY suffix", "Error: SCENE_SNAPSHOT_NOT_READY\n"] as [String?] {
            XCTAssertFalse(SceneSnapshotViewportRequest.isNotReadyJavaScriptError(error("WKErrorDomain", 4, message)))
        }
        XCTAssertFalse(SceneSnapshotViewportRequest.isNotReadyJavaScriptError(error("OtherDomain", 4, "SCENE_SNAPSHOT_NOT_READY")))
        XCTAssertFalse(SceneSnapshotViewportRequest.isNotReadyJavaScriptError(error("WKErrorDomain", 3, "SCENE_SNAPSHOT_NOT_READY")))
        XCTAssertFalse(SceneSnapshotViewportRequest.isNotReadyJavaScriptError(AuthoringFailure("SCENE_SNAPSHOT_STALE")))
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
    func testInspectionNeedsNoSavedStopAndBindsItsExactLocalPose() throws {
        let old = try fixture(), f = old.frame
        var draft = f.draft, candidate = draft.candidate
        candidate["extensions"]["org.exhibitos.studio/presentation"]["viewpoints"] = .array([])
        try draft.replaceCandidate(candidate)
        let saved = NativeStudioResponse(revision: 9, etag: old.saved.etag, draft: draft)
        let frame = ViewportFrame(epoch: f.epoch, sequence: f.sequence, tenantId: f.tenantId, principalId: f.principalId, draft: draft, selection: nil, mode: f.mode, view: f.view, disabled: false, transparent: false)
        let camera = try SceneSnapshotInspectionCamera(roomId: old.placement["roomId"].string!, position: [0, 1.2, 2], target: [0, 1.2, 0], fov: 50)
        let q = try SceneSnapshotViewportRequest(frame: frame, saved: saved, camera: .inspection(camera), placementId: old.placement["id"].string!)
        XCTAssertEqual(q.value["viewpointId"], .null)
        XCTAssertEqual(q.value["inspectionCamera"], camera.value)
        XCTAssertThrowsError(try SceneSnapshotViewportRequest(frame: frame, saved: saved, viewpointId: old.value["viewpointId"].string!, placementId: old.placement["id"].string!))
        var good = reply(q)
        good["camera"] = .object(["inspection": .bool(true), "roomId": camera.value["roomId"], "position": .vector([10, 1.2, 2]), "target": .vector([10, 1.2, 0]), "fov": .number(50), "localPosition": camera.value["position"], "localTarget": camera.value["target"]])
        XCTAssertEqual(try q.checkedReply(good).camera["inspection"], .bool(true))
        for key in ["inspection", "roomId", "localPosition", "localTarget", "fov"] {
            var bad = good; bad["camera"][key] = .null
            XCTAssertThrowsError(try q.checkedReply(bad), key)
        }
        var bad = good; bad["camera"]["viewpointId"] = old.value["viewpointId"]
        XCTAssertThrowsError(try q.checkedReply(bad))
        XCTAssertEqual(q.frame.draft, draft)
        let legacy = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"scene-snapshot-v1\"]}".utf8))
        XCTAssertFalse(legacy.supportsSceneSnapshotInspection)
        let current = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"scene-snapshot-inspection-v1\"]}".utf8))
        XCTAssertTrue(current.supportsSceneSnapshotInspection)
    }
    func testInspectionRejectsNonfiniteCoincidentOutOfRoomAndForeignRoomInputs() throws {
        let q = try fixture(), room = q.placement["roomId"].string!
        for point in [[Double.nan, 1, 2], [0, Double.infinity, 2], [0, 1, 1_000_001], [0, 1]] {
            XCTAssertThrowsError(try SceneSnapshotInspectionCamera(roomId: room, position: point, target: [0, 1, 0], fov: 50))
        }
        XCTAssertThrowsError(try SceneSnapshotInspectionCamera(roomId: room, position: [0, 1, 0], target: [0, 1, 0], fov: 50))
        for fov in [9, 121, Double.nan] {
            XCTAssertThrowsError(try SceneSnapshotInspectionCamera(roomId: room, position: [0, 1, 2], target: [0, 1, 0], fov: fov))
        }
        for point in [[6.001, 1, 2], [0, -0.001, 2], [0, 4.001, 2], [0, 1, 4.001]] {
            let camera = try SceneSnapshotInspectionCamera(roomId: room, position: point, target: [0, 1, 0], fov: 50)
            XCTAssertThrowsError(try SceneSnapshotViewportRequest(frame: q.frame, saved: q.saved, camera: .inspection(camera), placementId: q.placement["id"].string!))
        }
        let other = try SceneSnapshotInspectionCamera(roomId: UUID().uuidString.lowercased(), position: [0, 1, 2], target: [0, 1, 0], fov: 50)
        XCTAssertThrowsError(try SceneSnapshotViewportRequest(frame: q.frame, saved: q.saved, camera: .inspection(other), placementId: q.placement["id"].string!))
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
