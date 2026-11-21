// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeFloorRecordingAttachmentTests: XCTestCase {
    private let surface = "bdedd1da-0a03-442d-a58d-181877fe0652"
    private let audio = "4657f277-fdf6-46f7-8fe5-6110555697c3"

    #if DEBUG && os(macOS)
    func testEntranceRecordingSDKOnlyAdmitsTheExactSealedTargetAndCommands() throws {
        let target = "e22b8b0f69d45c29acf9883369c29825e8503f2c651893d07eb14f0f1826e0b8"
        let predecessor = "3b35d0d81021b7118e7092c4406976307a0ec1d0ff0f58310a06b745f078dce1"
        XCTAssertTrue(NativeAutomationSession.supportsEntranceRecordingRuntime(manifest: target))
        XCTAssertFalse(NativeAutomationSession.supportsEntranceRecordingRuntime(manifest: predecessor))
        XCTAssertFalse(NativeAutomationSession.supportsEntranceRecordingRuntime(manifest: String(target.dropLast()) + "0"))
        for operation in ["read", "room-audio-intake", "room-audio-status", "room-audio-approve",
                          "floor-recording-attach-apply", "applied-draft-save-resume", "reopen-saved"] {
            XCTAssertNoThrow(try NativeAutomationSession.checkFloorRecordingOperation(manifest: target, operation: operation))
        }
        for operation in ["exhibition-new", "save", "room-audio-apply", "export-oex", "export-static", "light-object-apply"] {
            XCTAssertThrowsError(try NativeAutomationSession.checkFloorRecordingOperation(manifest: target, operation: operation)) {
                XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_OPERATION_UNSUPPORTED")
            }
        }
        XCTAssertNoThrow(try NativeAutomationSession.checkFloorRecordingOperation(manifest: predecessor, operation: "save"))
    }
    func testClosingDispatchRequiresItsExactRuntimeAndApprovedFixture() throws {
        let closing = "cedec8a0ee0d4eba1d81304a8f65a61c48b9f1a261aca5529547512210990e88"
        let entrance = "e22b8b0f69d45c29acf9883369c29825e8503f2c651893d07eb14f0f1826e0b8"
        let etag = "saved-head"
        var selected = command
        selected["etag"] = .string(etag)
        selected["surfaceId"] = .string("5537fdc6-44a8-45c2-a867-a3481a754796")
        selected["materialId"] = .string("5e97508e-ae16-4e54-a3a9-44c7bb30424d")
        selected["expectedMediaAsset"]["bytes"] = .integer(59_564)
        selected["expectedMediaAsset"]["sha256"] = .string("9fb1851c98c8394c1639ace68e9a9091989cf3eac92ba2037b20798921e49226")
        var request = DocumentValue.object(["command": selected])
        XCTAssertFalse(try NativeAutomationSession.floorRecordingAttachment(request, manifest: closing, etag: etag).initializeMissingStoneRow)
        request["command"]["initializeMissingStoneRow"] = .bool(true)
        XCTAssertTrue(try NativeAutomationSession.floorRecordingAttachment(request, manifest: closing, etag: etag).initializeMissingStoneRow)
        for manifest in [entrance, String(closing.dropLast()) + "0"] {
            XCTAssertThrowsError(try NativeAutomationSession.floorRecordingAttachment(request, manifest: manifest, etag: etag))
        }
        for refused in [DocumentValue.bool(false), .null] {
            request["command"]["initializeMissingStoneRow"] = refused
            XCTAssertThrowsError(try NativeAutomationSession.floorRecordingAttachment(request, manifest: closing, etag: etag))
        }
        request["command"]["initializeMissingStoneRow"] = .bool(true)
        XCTAssertThrowsError(try NativeAutomationSession.floorRecordingAttachment(request, manifest: closing, etag: "stale"))
        request["command"]["materialVersion"] = .integer(2)
        XCTAssertThrowsError(try NativeAutomationSession.floorRecordingAttachment(request, manifest: closing, etag: etag))
        XCTAssertNoThrow(try NativeAutomationSession.checkFloorRecordingOperation(manifest: closing, operation: "floor-recording-attach-apply"))
        XCTAssertThrowsError(try NativeAutomationSession.checkFloorRecordingOperation(manifest: closing, operation: "export-static"))
    }
    #endif

    private func exactDraft() throws -> NativeExhibitionDraft {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_TEST_R219_WHEAT_FIT_JOURNAL"] else {
            throw XCTSkip("Requires the detached r219 journal fixture")
        }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
        return try XCTUnwrap(JSONDecoder().decode(NativeDocumentState.self, from: bytes).draft)
    }

    private var command: DocumentValue {
        .object(["surfaceId": .string(surface), "materialId": .string("cc0bfbc1-b1c8-4250-9014-dfdb6467b810"),
                 "materialVersion": .integer(1), "audioId": .string(audio), "audioRevision": .integer(1),
                 "expectedMediaAsset": .object(["id": .string(audio), "path": .string("media/\(audio)/audio.wav"),
                    "mime": .string("audio/wav"), "bytes": .integer(60_146),
                    "sha256": .string("989659bd0b4e01e1cd80287e3c5932f2220cafa07485d6d1b1d23907dedcbce4"),
                    "rights": .object(["holder": .string("Synthetic owner"), "ownership": .string("owner"),
                        "licenseId": .string("CC0-1.0"), "creditLine": .string("Synthetic fixture"),
                        "permissions": .object(["display": .bool(true), "export": .bool(true),
                                                "download": .bool(false), "commercial": .bool(false)])])])])
    }

    func testEntranceCandidatePreservesOtherRowsAndExactReplay() throws {
        let original = try exactDraft()
        let attachment = try NativeFloorRecordingAttachment(command)
        let candidate = try attachment.expectedCandidate(in: original)
        let namespace = ConcreteFootsteps.experienceNamespace
        var expected = original.candidate
        var rows = try XCTUnwrap(expected["extensions"][namespace]["footsteps"].items)
        let index = try XCTUnwrap(rows.firstIndex { $0["surfaceId"] == .string(surface) })
        rows[index]["assetId"] = .string(audio)
        expected["extensions"][namespace]["footsteps"] = .array(rows)
        let media = try XCTUnwrap(expected["mediaAssets"].items)
        expected["mediaAssets"] = .array(media + [command["expectedMediaAsset"]])
        XCTAssertEqual(candidate, expected)
        var applied = original
        try applied.replaceCandidate(expected)
        XCTAssertEqual(try attachment.expectedCandidate(in: applied), expected)
    }

    func testSelectedClosingRequiresExplicitInitializationAndPreservesOtherRows() throws {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_TEST_R224_RECORDING_JOURNAL"] else {
            throw XCTSkip("Requires the detached r224 recording journal fixture")
        }
        let base = try XCTUnwrap(JSONDecoder().decode(NativeDocumentState.self,
            from: Data(contentsOf: URL(fileURLWithPath: path))).draft)
        let closing = "5537fdc6-44a8-45c2-a867-a3481a754796"
        var selected = command
        selected["surfaceId"] = .string(closing)
        selected["materialId"] = .string("5e97508e-ae16-4e54-a3a9-44c7bb30424d")
        selected["expectedMediaAsset"]["sha256"] = .string("9fb1851c98c8394c1639ace68e9a9091989cf3eac92ba2037b20798921e49226")
        selected["expectedMediaAsset"]["bytes"] = .integer(59_564)
        XCTAssertThrowsError(try NativeFloorRecordingAttachment(selected))
        let omitted = try NativeFloorRecordingAttachment(selected, scope: .selectedFloor)
        XCTAssertThrowsError(try omitted.expectedCandidate(in: base))
        selected["initializeMissingStoneRow"] = .bool(false)
        XCTAssertThrowsError(try NativeFloorRecordingAttachment(selected, scope: .selectedFloor))
        selected["initializeMissingStoneRow"] = .bool(true)
        let attachment = try NativeFloorRecordingAttachment(selected, scope: .selectedFloor)
        let experience = ConcreteFootsteps.experienceNamespace
        let prior = try XCTUnwrap(base.candidate["extensions"][experience]["footsteps"].items)
        let priorMedia = try XCTUnwrap(base.candidate["mediaAssets"].items)
        var expected = base.candidate
        expected["extensions"][experience]["footsteps"] = .array(prior + [.object([
            "surfaceId": .string(closing), "material": .string("stone"), "assetId": .string(audio)
        ])])
        expected["mediaAssets"] = .array(priorMedia + [selected["expectedMediaAsset"]])
        XCTAssertEqual(try attachment.expectedCandidate(in: base), expected)
        var reopened = base
        try reopened.replaceCandidate(expected)
        XCTAssertEqual(try attachment.expectedCandidate(in: reopened), expected)
        selected["materialVersion"] = .integer(2)
        let wrongVersion = try NativeFloorRecordingAttachment(selected, scope: .selectedFloor)
        XCTAssertThrowsError(try wrongVersion.expectedCandidate(in: base))
    }

    private func withExplicitSound(_ draft: NativeExhibitionDraft, assets: [DocumentValue]) throws -> NativeExhibitionDraft {
        var draft = draft, candidate = draft.candidate
        let target = DocumentValue.object(["kind": .string("surface"), "surfaceId": .string(surface)])
        let namespace = FloorSettings.namespace
        var bindings = try XCTUnwrap(candidate["extensions"][namespace]["bindings"].items)
        let index: Int
        if let existing = bindings.firstIndex(where: { $0["target"] == target }) {
            index = existing
        } else {
            var row = try XCTUnwrap(bindings.first)
            row["target"] = target
            index = bindings.count
            bindings.append(row)
        }
        bindings[index]["sound"] = .object(["mode": .string("explicit"), "gain": .number(0.5), "assetIds": .array(assets)])
        candidate["extensions"][namespace]["bindings"] = .array(bindings)
        try draft.replaceCandidate(candidate)
        return draft
    }

    @MainActor
    func testIdenticalNoChangeRequestRechecksRevocationAndHeadWithoutStagingSave() async throws {
        let attachment = try NativeFloorRecordingAttachment(command)
        let base = try exactDraft()
        var applied = base
        try applied.replaceCandidate(attachment.expectedCandidate(in: base))
        applied = try withExplicitSound(applied, assets: [.string(audio.uppercased())])
        XCTAssertEqual(try attachment.expectedCandidate(in: applied), applied.candidate)
        let other = "65e85388-eec5-49ee-8956-e00c26478d6e"
        var conflict = applied
        var candidate = conflict.candidate, otherMedia = command["expectedMediaAsset"]
        otherMedia["id"] = .string(other)
        otherMedia["path"] = .string("media/\(other)/audio.wav")
        candidate["mediaAssets"] = .array(try XCTUnwrap(candidate["mediaAssets"].items) + [otherMedia])
        try conflict.replaceCandidate(candidate)
        conflict = try withExplicitSound(conflict, assets: [.string(audio), .string(other)])
        XCTAssertThrowsError(try attachment.expectedCandidate(in: conflict)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "FLOOR_RECORDING_ALREADY_BOUND")
        }

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
        let state = NativeDocumentState(format: 1, revision: 622,
            workspaceId: "11111111-1111-4111-8111-111111111111", tenantId: "22222222-2222-4222-8222-222222222222",
            principalId: "33333333-3333-4333-8333-333333333333", draft: applied,
            etag: "\"studio-r219-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: applied.document["editVersion"].integer, pending: nil)
        let decoder = JSONDecoder(), encoder = JSONEncoder()
        let value = try decoder.decode(DocumentValue.self, from: encoder.encode(state))
        try encoder.encode(DocumentValue.object(["document": value, "floorRecording": command, "manifest": .string(manifest)]))
            .write(to: root.appendingPathComponent("fixture.json"))
        let server = Process(); server.executableURL = helper; server.arguments = ["--serve"]
        server.standardOutput = FileHandle.nullDevice; server.standardError = FileHandle.nullDevice
        try server.run()
        defer { try? Data().write(to: root.appendingPathComponent("stop")); server.waitUntilExit() }
        let readyPath = root.appendingPathComponent("server-ready.json"), end = ContinuousClock.now.advanced(by: .seconds(5))
        while !FileManager.default.fileExists(atPath: readyPath.path), server.isRunning, ContinuousClock.now < end {
            try await Task.sleep(for: .milliseconds(10))
        }
        let ready = try decoder.decode(DocumentValue.self, from: Data(contentsOf: readyPath))
        let port = UInt16(try XCTUnwrap(ready["web"].integer))
        let ports: [String: UInt16] = ["web": port, "api": port == 18102 ? 18104 : 18102, "database": port == 18103 ? 18105 : 18103]
        let owner: DocumentValue = .object(["workspaceId": .string(state.workspaceId), "tenantId": .string(state.tenantId),
            "principalId": .string(state.principalId), "manifestSHA256": .string(manifest)])
        let existing = try decoder.decode(NativeAutomationSession.ExistingWorkspaceV5.self, from: encoder.encode(owner))
        let session = try NativeAutomationSession(helper: helper, profile: root.appendingPathComponent("profile"), ports: ports,
            outputRoot: root.appendingPathComponent("out"), temporaryRoot: root.appendingPathComponent("tmp"), inputs: [:],
            existingWorkspaceV5: existing)
        do {
            let opened = try await session.open()
            var fields: [String: DocumentValue] = ["version": .integer(1), "requestId": .string(UUID().uuidString.lowercased()),
                "operation": .string("floor-recording-attach-apply"), "retainSaveIntent": .bool(true),
                "expectedRevision": value["revision"], "expectedEditVersion": value["draft"]["editVersion"], "command": command]
            fields["command"]?["etag"] = value["etag"]
            for key in ["workspaceId", "tenantId", "principalId", "capability"] { fields[key] = opened[key] }
            let request = DocumentValue.object(fields)
            let first = try await session.execute(request)
            XCTAssertEqual(first, value)
            try Data().write(to: root.appendingPathComponent("revoke-floor-recording"))
            do { _ = try await session.execute(request); XCTFail("A revoked recording must refuse the identical request") }
            catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "ROOM_AUDIO_TRACK_UNAVAILABLE") }
            try Data().write(to: root.appendingPathComponent("advance-floor-head"))
            do { _ = try await session.execute(request); XCTFail("A moved head must refuse the identical request") }
            catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "EXHIBITION_COMPARISON_STALE") }
            let stats = try decoder.decode(DocumentValue.self, from: Data(contentsOf: root.appendingPathComponent("server-state.json")))
            XCTAssertEqual(stats["audioReads"], .integer(3))
            XCTAssertEqual(stats["floorChecks"], .integer(1))
            XCTAssertEqual(stats["puts"], .integer(0))
            XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("helper-state.json").path))
            try await session.close()
        } catch { try? await session.close(); throw error }
    }

    func testRejectsWrongAssetAndMutesWithoutChangingDraft() throws {
        var command = command
        command["expectedMediaAsset"]["sha256"] = .string(String(repeating: "0", count: 64))
        XCTAssertThrowsError(try NativeFloorRecordingAttachment(command))
        var draft = try exactDraft()
        let attachment = try NativeFloorRecordingAttachment(self.command)
        var candidate = draft.candidate
        let floorNamespace = "org.exhibitos.studio/floors"
        var bindings = try XCTUnwrap(candidate["extensions"][floorNamespace]["bindings"].items)
        var muted = try XCTUnwrap(bindings.first)
        muted["target"]["surfaceId"] = .string(surface)
        muted["sound"] = .object(["mode": .string("none"), "gain": .number(0)])
        bindings.append(muted)
        candidate["extensions"][floorNamespace]["bindings"] = .array(bindings)
        try draft.replaceCandidate(candidate)
        XCTAssertThrowsError(try attachment.expectedCandidate(in: draft)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "FLOOR_RECORDING_MUTED")
        }
        XCTAssertEqual(draft.candidate, candidate)
    }
}
