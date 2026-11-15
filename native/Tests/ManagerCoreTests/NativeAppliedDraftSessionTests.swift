// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeAppliedDraftSessionTests: XCTestCase {
    func testPublicSessionPrepareApplyAndResumeNeedsNoFutureInputAndRetainsLostReplyIdentity() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        for name in ["profile", "out", "tmp"] {
            try FileManager.default.createDirectory(at: root.appendingPathComponent(name), withIntermediateDirectories: false,
                attributes: [.posixPermissions: 0o700])
        }
        let helper = root.appendingPathComponent("helper")
        let fixture = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Fixtures/applied-draft-session-helper.py")
        try FileManager.default.copyItem(at: fixture, to: helper)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
        let manifest = "41e741a15bedf72750de1033c3afd462503fc5901100a1660caf14ce25a980a1"
        let light = "aaaaaaaa-0000-4000-8000-000000000001"
        var draft = try NativeExhibitionDraft.new(title: "Synthetic same-session recovery")
        var candidate = draft.candidate
        candidate["lights"] = .array([.object(["id": .string(light), "roomId": draft.rooms[0]["id"],
            "type": .string("spot"), "unit": .string("candela"), "transform": NativeExhibitionDraft.identity,
            "color": .vector([1,1,1]), "intensity": .integer(100), "castsShadow": .bool(false), "beamAngle": .number(0.5)])])
        try draft.replaceCandidate(candidate)
        let state = NativeDocumentState(format: 1, revision: 40,
            workspaceId: "11111111-1111-4111-8111-111111111111", tenantId: "22222222-2222-4222-8222-222222222222",
            principalId: "33333333-3333-4333-8333-333333333333", draft: draft,
            etag: "\"studio-r12-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let value = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(state))
        let asset = try SharedFurnishingCatalog.asset(catalogId: "museum-track-spot", id: "bbbbbbbb-0000-4000-8000-000000000001")
        try JSONEncoder().encode(DocumentValue.object(["document": value, "asset": asset, "manifest": .string(manifest)]))
            .write(to: root.appendingPathComponent("fixture.json"))
        let server = Process(); server.executableURL = helper; server.arguments = ["--serve"]
        server.standardOutput = FileHandle.nullDevice; server.standardError = FileHandle.nullDevice
        try server.run()
        defer {
            try? Data().write(to: root.appendingPathComponent("stop"))
            server.waitUntilExit() // this fixture polls its own stop marker every 100 ms
        }
        let readyPath = root.appendingPathComponent("server-ready.json"), end = ContinuousClock.now.advanced(by: .seconds(5))
        while !FileManager.default.fileExists(atPath: readyPath.path), server.isRunning, ContinuousClock.now < end {
            try await Task.sleep(for: .milliseconds(10))
        }
        let ready = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: readyPath))
        let port = UInt16(try XCTUnwrap(ready["web"].integer))
        let ports: [String: UInt16] = ["web": port, "api": port == 18102 ? 18104 : 18102, "database": port == 18103 ? 18105 : 18103]
        let owner: DocumentValue = .object(["workspaceId": .string(state.workspaceId), "tenantId": .string(state.tenantId),
            "principalId": .string(state.principalId), "manifestSHA256": .string(manifest)])
        let existing = try JSONDecoder().decode(NativeAutomationSession.ExistingWorkspaceV5.self, from: JSONEncoder().encode(owner))
        let session = try NativeAutomationSession(helper: helper, profile: root.appendingPathComponent("profile"), ports: ports,
            outputRoot: root.appendingPathComponent("out"), temporaryRoot: root.appendingPathComponent("tmp"), inputs: [:],
            existingWorkspaceV5: existing)
        do {
            let opened = try await session.open()
            func request(_ operation: String, id: String = UUID().uuidString.lowercased(), document: DocumentValue? = nil) -> DocumentValue {
                var fields: [String: DocumentValue] = ["version": .integer(1), "requestId": .string(id), "operation": .string(operation)]
                for key in ["workspaceId", "tenantId", "principalId", "capability"] { fields[key] = opened[key] }
                if let document { fields["expectedRevision"] = document["revision"]; fields["expectedEditVersion"] = document["draft"]["editVersion"] }
                return .object(fields)
            }
            var prepare = request("fixture-prepare", document: opened["document"])
            prepare["command"] = .object(["lightId": .string(light), "pose": .object(["position": .vector([0,0.15,0.0755]), "yaw": .number(0), "scale": .number(1)])])
            let prepared = try await session.execute(prepare), applyId = UUID().uuidString.lowercased()
            var apply = request("fixture-apply", id: applyId, document: opened["document"])
            apply["proposalId"] = prepared["proposalId"]; apply["retainSaveIntent"] = .bool(true)
            _ = try await session.execute(apply) // simulate the caller losing the external SDK reply
            let applied = try await session.execute(request("read"))["document"]
            XCTAssertEqual(applied["pending"]["requestId"], .string(applyId))
            var resume = request("applied-draft-save-resume", id: applyId, document: applied)
            resume["command"] = .object(["applyRequestId": .string(applyId)])
            do { _ = try await session.execute(resume); XCTFail("RPC identity must differ from the retained apply ID") }
            catch let error as BridgeFailure { XCTAssertEqual(error.code, "AUTOMATION_REQUEST_CHANGED") }
            resume["requestId"] = .string(UUID().uuidString.lowercased())
            _ = try await session.execute(resume) // also lose this reply after the local Save confirmation
            let clean = try await session.execute(request("read"))["document"]
            XCTAssertEqual(clean["revision"], .integer(42))
            XCTAssertEqual(clean["pending"], .null)
            let firstSave = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: root.appendingPathComponent("server-state.json")))
            XCTAssertEqual(firstSave["commits"], .integer(1)); XCTAssertEqual(firstSave["puts"], .integer(1))
            resume = request("applied-draft-save-resume", document: clean)
            resume["command"] = .object(["applyRequestId": .string(applyId)])
            let already = try await session.execute(resume)
            XCTAssertEqual(already["status"], .string("already-saved"))
            let stats = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: root.appendingPathComponent("server-state.json")))
            XCTAssertEqual(stats["commits"], .integer(1)); XCTAssertEqual(stats["puts"], .integer(2))
            XCTAssertEqual(stats["lastRequestId"], .string(applyId))
            let local = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: root.appendingPathComponent("helper-state.json")))
            XCTAssertEqual(local["documentWrites"], .integer(2))
            XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: root.appendingPathComponent("out").path).isEmpty)
            try await session.close()
        } catch { try? await session.close(); throw error }
    }
}
