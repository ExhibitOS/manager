// SPDX-License-Identifier: Apache-2.0
import XCTest
import SwiftUI
import AppKit
import CryptoKit
import ManagerCore
@testable import ManagerApp

#if DEBUG
/// Opt-in actual native/WK integration. The owner supplies an already prepared,
/// stopped isolated profile and holds the application's execution resources.
/// No fixture, credentials or artwork bytes are bundled in the test target.
@MainActor
final class SceneSnapshotHostTests: XCTestCase {
    private struct Fixture: Decodable {
        let app: String, profile: String, documentFile: String, documentSHA256: String
        let manifestSHA256: String, exhibitionId: String, etag: String
        let revision: Int, placementId: String, roomId: String, otherRoomViewpointId: String
        let assetId: String, outputDirectory: String
        let position: [Double], target: [Double], fov: Double
    }
    private func require(_ condition: Bool, _ code: String) throws {
        guard condition else { throw BridgeFailure(code) }
    }
    private func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
    private func waitForSnapshotPolicy(_ controller: NativeViewportController) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(10))
        while !controller.sceneSnapshotPolicyActiveForTesting {
            guard ContinuousClock.now < deadline else { throw BridgeFailure("HOST_TEST_POLICY_NOT_INSTALLED") }
            await Task.yield()
        }
    }
    private func waitForRuntime(_ bridge: NativeBridge, _ context: Context, ready: Bool) async throws -> NativeHostStatus {
        let deadline = ContinuousClock.now.advanced(by: .seconds(120))
        while ContinuousClock.now < deadline {
            let status = try await bridge.nativeStatus(context)
            if !status.busy {
                if let code = status.errorCode { throw BridgeFailure(code) }
                if ready ? status.runtime?.readiness.ready == true : status.runtime?.hasOwnedProcesses != true { return status }
            }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw BridgeFailure("HOST_TEST_RUNTIME_TIMEOUT")
    }
    func testLiveInspectionCameraWithoutPanels() async throws {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_NATIVE_SNAPSHOT_HOST_FIXTURE"] else {
            throw XCTSkip("Requires an explicitly owned isolated live native host fixture")
        }
        let f = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: path)))
        let profile = URL(fileURLWithPath: f.profile), output = URL(fileURLWithPath: f.outputDirectory)
        let documentURL = profile.appendingPathComponent(f.documentFile)
        let documentBytes = try Data(contentsOf: documentURL)
        try require(digest(documentBytes) == f.documentSHA256, "HOST_TEST_BASELINE_CHANGED")
        let bridge = try NativeBridge(helper: URL(fileURLWithPath: f.app).appendingPathComponent("Contents/Helpers/exhibitos-native"), profile: profile)
        let context = try await bridge.context()
        let initial = try await bridge.nativeStatus(context)
        try require(!initial.needsShutdown && !initial.busy, "HOST_TEST_ALREADY_OWNED")
        var window: NSWindow?
        var requests: [Task<SceneSnapshotResult, Error>] = []
        var failure: Error?
        do {
            _ = try await bridge.start(context)
            let status = try await waitForRuntime(bridge, context, ready: true)
            guard let configuration = status.configuration else { throw BridgeFailure("HOST_TEST_CONFIGURATION_MISSING") }
            try require(configuration.manifestSha256 == f.manifestSHA256, "HOST_TEST_MANIFEST_CHANGED")
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: configuration.ports.web, tenantId: credentials.tenantId)
            let session = try await api.connect(credentials)
            let local = try await bridge.documentState(context)
            let saved = try await api.exhibition(f.exhibitionId)
            try require(local.pending == nil && !local.dirty && local.draft == saved.draft && local.etag == f.etag &&
                        saved.etag == f.etag && saved.revision == f.revision, "HOST_TEST_SAVED_CHANGED")
            let model = ExhibitionModel()
            await model.connect(api: api, session: session, credentials: credentials,
                read: { try await bridge.documentState(context) },
                write: { _ in throw BridgeFailure("CAPTURE_MUST_NOT_WRITE") })
            try require(model.draft == saved.draft && !model.dirty && !model.pending, "HOST_TEST_MODEL_CHANGED")
            _ = NSApplication.shared
            let host = NSHostingView(rootView: NativeViewportArea(model: model, ready: true, controller: model.viewportController, workbench: true))
            let liveWindow = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 700), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            liveWindow.isReleasedWhenClosed = false; liveWindow.title = "Native snapshot integration"
            liveWindow.contentView = host; liveWindow.orderFront(nil); window = liveWindow
            let deadline = ContinuousClock.now.advanced(by: .seconds(60))
            while !model.canCaptureSceneSnapshot || !model.viewportController.sceneSnapshotInspectionSupported || model.viewportController.loaded != saved.draft.placements.count {
                guard ContinuousClock.now < deadline else { throw BridgeFailure("HOST_TEST_ACK_TIMEOUT") }
                try await Task.sleep(for: .milliseconds(100))
            }
            let controller = model.viewportController
            let epoch = controller.hostEpoch, sequence = controller.acceptedSequence, selection = model.viewportSelection
            let mode = model.viewportMode, view = model.viewportView, cameraSequence = model.viewportCameraSequence
            func preserved() -> Bool {
                model.draft == saved.draft && !model.dirty && !model.pending && !model.sceneSnapshotting && !controller.sceneSnapshotPolicyActiveForTesting &&
                controller.connected && controller.editingContextActive && controller.hostEpoch == epoch && controller.acceptedSequence == sequence &&
                model.viewportSelection == selection && model.viewportMode == mode && model.viewportView == view && model.viewportCameraSequence == cameraSequence &&
                model.viewportPresentation == .threeD && !model.viewportTransparent && model.canCaptureSceneSnapshot
            }
            // The original saved-viewpoint path must still reject a target in a
            // different room. No saved viewing point is created or modified.
            do {
                _ = try await model.captureSceneSnapshot(camera: .savedViewpoint(f.otherRoomViewpointId), placementId: f.placementId)
                XCTFail("Cross-room saved viewpoint was accepted")
            } catch { try require(preserved(), "HOST_TEST_SAVED_FAILURE_STATE_CHANGED") }
            // A finite in-room camera aimed away from this artwork reaches the
            // shared validator, then the native policy/lane must be restored.
            let wrongTarget = try SceneSnapshotInspectionCamera(roomId: f.roomId, position: f.position, target: [0, 0, 0], fov: f.fov)
            do {
                _ = try await model.captureSceneSnapshot(camera: .inspection(wrongTarget), placementId: f.placementId)
                XCTFail("Camera outside the artwork viewing bounds was accepted")
            } catch { try require(preserved(), "HOST_TEST_INSPECTION_FAILURE_STATE_CHANGED") }
            let camera = try SceneSnapshotInspectionCamera(roomId: f.roomId, position: f.position, target: f.target, fov: f.fov)
            let stale = Task { try await model.captureSceneSnapshot(camera: .inspection(camera), placementId: f.placementId) }
            requests.append(stale)
            try await waitForSnapshotPolicy(controller)
            controller.editingContextActive = false
            do { _ = try await stale.value; XCTFail("Replaced editing context was accepted") } catch {}
            controller.editingContextActive = true
            try require(preserved(), "HOST_TEST_STALE_STATE_CHANGED")
            let cancelled = Task { try await model.captureSceneSnapshot(camera: .inspection(camera), placementId: f.placementId) }
            requests.append(cancelled)
            try await waitForSnapshotPolicy(controller)
            cancelled.cancel()
            do { _ = try await cancelled.value; XCTFail("Cancelled capture was accepted") } catch {}
            try require(preserved(), "HOST_TEST_CANCEL_STATE_CHANGED")
            let image = try await model.captureSceneSnapshot(camera: .inspection(camera), placementId: f.placementId)
            try require(preserved() && image.savedRevision == f.revision && image.savedETag == f.etag &&
                        image.target["assetId"].string == f.assetId && image.target["inFrustum"] == .bool(true), "HOST_TEST_RESULT_CHANGED")
            try saved.requireSameComparison(try await api.exhibition(f.exhibitionId))
            try require(try Data(contentsOf: documentURL) == documentBytes, "HOST_TEST_LOCAL_DOCUMENT_CHANGED")
            try await image.writeNewPNG(to: output.appendingPathComponent("inspection.png"))
            let receipt: DocumentValue = .object(["exhibitionId": .string(f.exhibitionId), "revision": .integer(Int64(f.revision)), "etag": .string(f.etag),
                "camera": image.camera, "target": image.target, "candidateSha256": .string(image.candidateSha256), "pngSHA256": .string(digest(image.png)),
                "pngBytes": .integer(Int64(image.png.count)), "documentUnchanged": .bool(true), "hostStatePreserved": .bool(true),
                "programmaticLiveWK": .bool(true), "filePanelShown": .bool(false),
                "inflightCancellationPolicyRestored": .bool(true), "inflightStaleContextPolicyRestored": .bool(true)])
            try JSONEncoder().encode(receipt).write(to: output.appendingPathComponent("capture.json"), options: .withoutOverwriting)
        } catch { failure = error }
        for request in requests { request.cancel(); _ = try? await request.value }
        window?.contentView = nil; window?.close(); window = nil
        await Task.yield()
        _ = try await bridge.stop(context)
        let stopped = try await waitForRuntime(bridge, context, ready: false)
        try require(stopped.runtime?.hasOwnedProcesses != true && !stopped.needsShutdown, "HOST_TEST_STOP_PENDING")
        try await bridge.close()
        try require(try Data(contentsOf: documentURL) == documentBytes, "HOST_TEST_DOCUMENT_CHANGED_ON_STOP")
        if let failure { throw failure }
    }
}
#endif
