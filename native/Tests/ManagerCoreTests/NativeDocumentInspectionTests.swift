// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class NativeDocumentInspectionTests: XCTestCase {
    private func document(revision: UInt64 = 17, saved: Int64 = 1, pending: Bool = false) throws -> NativeDocumentState {
        let draft = try NativeExhibitionDraft.new(title: "PRIVATE_CANDIDATE_MUST_NOT_BE_OUTPUT")
        return NativeDocumentState(format: 1, revision: revision,
            workspaceId: "11111111-1111-4111-8111-111111111111",
            tenantId: "22222222-2222-4222-8222-222222222222",
            principalId: "33333333-3333-4333-8333-333333333333",
            draft: revision == 0 ? nil : draft,
            etag: revision == 0 ? nil : "\"studio-r12-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: revision == 0 ? nil : saved,
            pending: pending ? .init(requestId: "44444444-4444-4444-8444-444444444444",
                etag: "\"studio-r12-" + String(repeating: "a", count: 64) + "\"", draft: draft) : nil)
    }

    private func fixture(_ document: NativeDocumentState, startupFailure: Bool = false) throws -> (URL, [String]) {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        let profile = root.appendingPathComponent("profile"), helper = root.appendingPathComponent("helper")
        try FileManager.default.createDirectory(at: profile, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        try JSONEncoder().encode(document).write(to: root.appendingPathComponent("document.json"))
        let script: String
        if startupFailure {
            // Wait for the first frame so the bridge observes EOF rather than
            // racing a write to an already-closed pipe. Real startup stderr is
            // intentionally not exposed by the existing NativeBridge.
            script = "#!/bin/sh\nIFS= read -r frame\nprintf 'PROFILE_BUSY\\n' >&2\nexit 1\n"
        } else {
            script = #"""
            #!/bin/sh
            [ "$#" = 3 ] && [ "$1" = --profile ] && [ "$2" = "${0%/*}/profile" ] && [ "$3" = --existing-managed-profile ] || exit 2
            count=0
            while IFS= read -r frame; do
              count=$((count+1))
              printf '%s\n' "$frame" >> "${0%/*}/trace"
              case "$frame" in
                *'"command":"context"'*)
                  [ "$count" = 1 ] || exit 3
                  result='{"selectionToken":"55555555-5555-4555-8555-555555555555","activeId":"66666666-6666-4666-8666-666666666666","installations":[]}' ;;
                *'"command":"document_state"'*)
                  [ "$count" = 2 ] || exit 4
                  result=$(/bin/cat "${0%/*}/document.json") ;;
                *) exit 5 ;;
              esac
              printf '{"version":1,"id":%s,"result":%s}\n' "$count" "$result"
            done
            : > "${0%/*}/eof"
            """#
        }
        try script.write(to: helper, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
        return (root, ["exhibitos-automation", "--inspect-existing-document", "--helper", helper.path, "--profile", profile.path])
    }

    func testOnlyContextDocumentAndEOFWithRedactedPendingSummary() async throws {
        let state = try document(pending: true), (root, arguments) = try fixture(state)
        defer { try? FileManager.default.removeItem(at: root) }
        let result = try await NativeDocumentInspection.inspect(arguments: arguments)
        XCTAssertEqual(result["nativeRevision"], .integer(17))
        XCTAssertEqual(result["dirty"], .bool(true))
        XCTAssertEqual(result["pendingSave"]["requestId"], .string(state.pending!.requestId))
        XCTAssertEqual(result["liveRuntimeOwnership"], .string("unknown"))
        XCTAssertEqual(result["applyOperationAttribution"], .string("unknown"))
        let text = String(decoding: try JSONEncoder().encode(result), as: UTF8.self)
        XCTAssertFalse(text.contains("PRIVATE_CANDIDATE_MUST_NOT_BE_OUTPUT"))
        XCTAssertFalse(text.contains("selectionToken"))
        let frames = try String(contentsOf: root.appendingPathComponent("trace"), encoding: .utf8).split(separator: "\n")
        XCTAssertEqual(frames.count, 2)
        let requests = try frames.map { try JSONDecoder().decode(DocumentValue.self, from: Data($0.utf8)) }
        XCTAssertEqual(requests.map { $0["command"] }, [.string("context"), .string("document_state")])
        XCTAssertEqual(requests[1]["selection_token"], .string("55555555-5555-4555-8555-555555555555"))
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appendingPathComponent("eof").path))
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: arguments[5]).isEmpty)
    }

    func testDirtyWithoutPendingAndCleanStateRemainDistinct() throws {
        let clean = try NativeDocumentInspection.summary(document())
        XCTAssertEqual(clean["dirty"], .bool(false))
        XCTAssertEqual(clean["pendingSave"], .null)
        XCTAssertEqual(clean["savedEditVersion"], clean["draftEditVersion"])
        var unsaved = try document(), envelope = unsaved.draft!.document
        envelope["editVersion"] = .integer(2)
        unsaved.draft = try NativeExhibitionDraft(document: envelope)
        let dirty = try NativeDocumentInspection.summary(unsaved)
        XCTAssertEqual(dirty["dirty"], .bool(true))
        XCTAssertEqual(dirty["pendingSave"], .null)
        XCTAssertEqual(dirty["applyOperationAttribution"], .string("unknown"))
    }

    func testMissingDocumentDefaultIsRefusedAndHelperReceivesEOF() async throws {
        let (root, arguments) = try fixture(document(revision: 0))
        defer { try? FileManager.default.removeItem(at: root) }
        do { _ = try await NativeDocumentInspection.inspect(arguments: arguments); XCTFail("Missing existing document must fail") }
        catch let error as BridgeFailure { XCTAssertEqual(error.code, "NATIVE_EXISTING_DOCUMENT_REQUIRED") }
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appendingPathComponent("eof").path))
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: arguments[5]).isEmpty)
    }

    func testStartupFailureRemainsGenericAndDoesNotInferOwnership() async throws {
        let (root, arguments) = try fixture(document(), startupFailure: true)
        defer { try? FileManager.default.removeItem(at: root) }
        do { _ = try await NativeDocumentInspection.inspect(arguments: arguments); XCTFail("Startup refusal must fail") }
        catch let error as BridgeFailure { XCTAssertEqual(error.code, "NATIVE_HELPER_EXITED") }
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: arguments[5]).isEmpty)
    }

    func testClosedArgumentsRejectRelativeSymlinkAndExtraInput() throws {
        let (root, arguments) = try fixture(document())
        defer { try? FileManager.default.removeItem(at: root) }
        _ = try NativeDocumentInspection.paths(arguments)
        XCTAssertThrowsError(try NativeDocumentInspection.paths(arguments + ["--configuration", "ignored"]))
        var bad = arguments; bad[3] = "relative/helper"
        XCTAssertThrowsError(try NativeDocumentInspection.paths(bad))
        let link = root.appendingPathComponent("helper-link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: URL(fileURLWithPath: arguments[3]))
        bad = arguments; bad[3] = link.path
        XCTAssertThrowsError(try NativeDocumentInspection.paths(bad))
        bad = arguments; bad[5] += "/missing"
        XCTAssertThrowsError(try NativeDocumentInspection.paths(bad))
    }
}
