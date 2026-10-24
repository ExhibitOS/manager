// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class PersonalAuthoringTests: XCTestCase {
    func testActualPrivateHelperPersonalWorkspaceWithoutLoginAndReopen() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let helper = environment["EXHIBITOS_PERSONAL_TEST_HELPER"], let directory = environment["EXHIBITOS_PERSONAL_TEST_ROOT"] else {
            throw XCTSkip("Requires the explicit isolated personal native helper/payload harness")
        }
        let root = URL(fileURLWithPath: directory, isDirectory: true)
        let profile = root.appendingPathComponent("profile")
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.path))
        let bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
        let context = try await bridge.context()
        func idle(ready: Bool = false) async throws -> NativeHostStatus {
            let deadline = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < deadline {
                let value = try await bridge.nativeStatus(context)
                if !value.busy && (!ready || value.runtime?.readiness.ready == true) { return value }
                if !value.busy, let code = value.errorCode { throw BridgeFailure(code) }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        do {
            let unconfigured = try await bridge.nativeStatus(context)
            XCTAssertFalse(unconfigured.accountRequired)
            _ = try await bridge.prepare(context) // no account, subject or password
            let configured = try await idle()
            XCTAssertEqual(configured.configuration?.kind, "personal")
            XCTAssertFalse(configured.accountRequired)
            XCTAssertNil(configured.errorCode)
            _ = try await bridge.start(context)
            let started = try await idle(ready: true)
            let first = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: started.configuration!.ports.web, tenantId: first.tenantId)
            let owner = try await api.connect(first) // no /auth/login request
            let artist = try await api.createArtist(name: "개인 작업 공간 합성 작가", bio: "No password or user account onboarding")
            var metadata = ArtworkMetadata()
            metadata.title = "개인 공간 영속 저장 시험"
            metadata.rights.holder = artist.name
            metadata.rights.creditLine = "Synthetic fixture"
            metadata.provenance.source = "synthetic"
            let artwork = try await api.saveArtwork(id: nil, revision: nil, artistId: artist.id, metadata: metadata)
            XCTAssertEqual(artwork.metadata, metadata)
            _ = try await bridge.stop(context)
            let stopped = try await idle()
            XCTAssertFalse(stopped.needsShutdown)
            XCTAssertEqual(stopped.runtime?.exits["postgres"], 0)
            XCTAssertEqual(stopped.runtime?.exits["node"], 0)
            _ = try await bridge.start(context)
            _ = try await idle(ready: true)
            let renewed = try await bridge.authoringSession(context)
            XCTAssertTrue(first.token != renewed.token)
            let reopened = try await api.connect(renewed)
            XCTAssertEqual(owner.userId, reopened.userId)
            let saved = try await api.artwork(artwork.id)
            XCTAssertEqual(saved.metadata, metadata)
            _ = try await bridge.stop(context)
            let final = try await idle()
            XCTAssertFalse(final.needsShutdown)
            let summary: [String: Any] = ["result":"PASS", "scope":"NO_ACCOUNT_NATIVE_HELPER_AND_SWIFT_METADATA_NOT_WHOLE_FLOW", "tenantId":first.tenantId,"principalId":owner.userId,"artworkId":artwork.id,"postgresExit":final.runtime?.exits["postgres"] ?? -999,"nodeExit":final.runtime?.exits["node"] ?? -999]
            try JSONSerialization.data(withJSONObject: summary, options: [.prettyPrinted,.sortedKeys]).write(to: root.appendingPathComponent("swift-result.json"), options: .atomic)
        } catch {
            _ = try? await bridge.stop(context)
            _ = try? await idle()
            throw error
        }
    }
}
