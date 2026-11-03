// SPDX-License-Identifier: Apache-2.0
import XCTest
import SwiftUI
import AppKit
import CryptoKit
import ManagerCore
@testable import ManagerApp

#if DEBUG
/// Opt-in application-hosted native/WK integration. The owner supplies a real
/// foreground application host, a stopped isolated profile and execution resources.
/// No fixture, credentials or artwork bytes are bundled in the test target.
@MainActor
final class SceneSnapshotHostTests: XCTestCase {
    private var evidence: [String: String] = [:]
    private func captureFailure(_ error: Error, phase: String, controller _: NativeViewportController) -> String {
        // Do not publish diagnostics into the live view while observing it.
        // Retain bounded codes only, never arbitrary JS text or request data.
        let native = error as NSError
        evidence[phase + "NativeCode"] = String(native.code)
        evidence[phase + "NativeDomain"] = native.domain.range(of: "^[A-Za-z0-9_.-]{1,80}$", options: .regularExpression) == nil ? "Native" : native.domain
        let known = ["SCENE_SNAPSHOT_NOT_READY", "SCENE_SNAPSHOT_UNAVAILABLE", "SCENE_SNAPSHOT_STALE",
                     "SCENE_SNAPSHOT_RIGHTS_UNVERIFIED", "SCENE_SNAPSHOT_SAVED_CHANGED", "SCENE_SNAPSHOT_TARGET_NOT_READY",
                     "SCENE_SNAPSHOT_CAMERA_INVALID", "VIEWING_TARGET_OUTSIDE_ARTWORK", "VIEWING_IMAGE_FRONT"]
        let raw = (error as? AuthoringFailure)?.code ?? ((error as NSError).userInfo["WKJavaScriptExceptionMessage"] as? String) ?? ""
        let code = raw.hasPrefix("Error: ") ? String(raw.dropFirst(7)) : raw
        let result = known.contains(code) ? code : "NATIVE_ERROR"
        evidence[phase + "Code"] = result
        return result
    }
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
    private func waitForSnapshotPolicy(_ controller: NativeViewportController, finished: () -> Bool, phase: String) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(10))
        while !controller.sceneSnapshotPolicyActiveForTesting {
            guard !finished() else { throw BridgeFailure("HOST_TEST_REQUEST_FINISHED_BEFORE_POLICY_OBSERVATION") }
            guard ContinuousClock.now < deadline else { throw BridgeFailure("HOST_TEST_POLICY_NOT_INSTALLED") }
            try await Task.sleep(for: .milliseconds(1))
        }
        evidence[phase + "PolicyObserved"] = "true"
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
        guard Bundle.main.bundleURL.pathExtension == "app", NSApp?.isRunning == true else {
            throw XCTSkip("Requires a normal application-hosted runner; a standalone XCTest CLI is not a foreground WK host")
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
        var hostController: NativeViewportController?
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
            let selected = status.selectedOex == true
            let boundedWorkspace = context.installations.first(where: { $0.id == context.activeId })?.kind == "oex-import"
            await model.connect(api: api, session: session, credentials: credentials,
                selectedBoundedPolicy: selected, ordinaryBoundedRuntime: boundedWorkspace && !selected,
                boundedRuntimeQualified: status.oex027Qualified == true,
                read: { try await bridge.documentState(context) },
                write: { _ in throw BridgeFailure("CAPTURE_MUST_NOT_WRITE") })
            await model.oexImport.connect(api: api, session: session,
                read: { try await bridge.oexImportState(context) },
                save: { _ in throw BridgeFailure("CAPTURE_MUST_NOT_WRITE") })
            if boundedWorkspace && !selected { await model.prepareOrdinaryBoundedPlacement() }
            try require(model.draft == saved.draft && !model.dirty && !model.pending, "HOST_TEST_MODEL_CHANGED")
            // Configure the fresh inspection host before its first frame; the
            // capture method itself never toggles the editor's transparency.
            model.viewportTransparent = false
            let application = NSApplication.shared
            let host = NSHostingView(rootView: NativeViewportArea(model: model, ready: true, controller: model.viewportController, workbench: true))
            let liveWindow = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 700), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            liveWindow.isReleasedWhenClosed = false; liveWindow.title = "Native snapshot integration"
            liveWindow.contentView = host; liveWindow.makeKeyAndOrderFront(nil); window = liveWindow
            application.activate(ignoringOtherApps: true)
            let deadline = ContinuousClock.now.advanced(by: .seconds(60))
            while !application.isActive || !liveWindow.occlusionState.contains(.visible) || !model.canCaptureSceneSnapshot || !model.viewportController.sceneSnapshotInspectionSupported || model.viewportController.loaded != saved.draft.placements.count {
                guard ContinuousClock.now < deadline else { throw BridgeFailure("HOST_TEST_ACK_TIMEOUT") }
                try await Task.sleep(for: .milliseconds(100))
            }
            let controller = model.viewportController
            hostController = controller
            let epoch = controller.hostEpoch, selection = model.viewportSelection
            var sequence = controller.acceptedSequence
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
            } catch {
                try require(captureFailure(error, phase: "savedGuard", controller: controller) == "SCENE_SNAPSHOT_NOT_READY", "HOST_TEST_WRONG_SAVED_GUARD_ERROR")
                try require(preserved(), "HOST_TEST_SAVED_FAILURE_STATE_CHANGED")
            }
            // A finite in-room camera aimed away from this artwork reaches the
            // shared validator, then the native policy/lane must be restored.
            let wrongTarget = try SceneSnapshotInspectionCamera(roomId: f.roomId, position: f.position, target: [0, 0, 0], fov: f.fov)
            do {
                _ = try await model.captureSceneSnapshot(camera: .inspection(wrongTarget), placementId: f.placementId)
                XCTFail("Camera outside the artwork viewing bounds was accepted")
            } catch {
                try require(captureFailure(error, phase: "wrongTarget", controller: controller) == "VIEWING_TARGET_OUTSIDE_ARTWORK", "HOST_TEST_WRONG_INSPECTION_ERROR")
                try require(preserved(), "HOST_TEST_INSPECTION_FAILURE_STATE_CHANGED")
            }
            let camera = try SceneSnapshotInspectionCamera(roomId: f.roomId, position: f.position, target: f.target, fov: f.fov)
            var staleFinished = false
            let stale = Task {
                defer { staleFinished = true }
                do { return try await model.captureSceneSnapshot(camera: .inspection(camera), placementId: f.placementId) }
                catch { _ = captureFailure(error, phase: "staleRequest", controller: controller); throw error }
            }
            requests.append(stale)
            try await waitForSnapshotPolicy(controller, finished: { staleFinished }, phase: "stale")
            controller.editingContextActive = false
            var staleRejected = false
            do { _ = try await stale.value } catch { staleRejected = true }
            try require(staleRejected, "HOST_TEST_STALE_GUARD_MISSING")
            controller.editingContextActive = true
            // A new editing context must receive a NEW exact ordinary ACK.
            // Keeping the old security sequence would conceal the real boundary.
            let recoveryDeadline = ContinuousClock.now.advanced(by: .seconds(60))
            while !model.canCaptureSceneSnapshot || controller.hostEpoch != epoch || controller.acceptedSequence <= sequence {
                guard ContinuousClock.now < recoveryDeadline else { throw BridgeFailure("HOST_TEST_CONTEXT_ACK_TIMEOUT") }
                try await Task.sleep(for: .milliseconds(20))
            }
            evidence["stalePreviousSequence"] = String(sequence)
            sequence = controller.acceptedSequence
            evidence["staleRecoveredSequence"] = String(sequence)
            try require(preserved(), "HOST_TEST_STALE_STATE_CHANGED")
            var cancelFinished = false
            let cancelled = Task {
                defer { cancelFinished = true }
                do { return try await model.captureSceneSnapshot(camera: .inspection(camera), placementId: f.placementId) }
                catch { _ = captureFailure(error, phase: "cancelRequest", controller: controller); throw error }
            }
            requests.append(cancelled)
            try await waitForSnapshotPolicy(controller, finished: { cancelFinished }, phase: "cancel")
            cancelled.cancel()
            var cancelRejected = false
            do { _ = try await cancelled.value } catch { cancelRejected = true }
            try require(cancelRejected && cancelled.isCancelled, "HOST_TEST_CANCEL_GUARD_MISSING")
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
        } catch {
            failure = error
            if let hostController { _ = captureFailure(error, phase: "failure", controller: hostController) }
        }
        for request in requests { request.cancel(); _ = try? await request.value }
        evidence["applicationActive"] = String(NSApp?.isActive == true)
        evidence["windowVisible"] = String(window?.isVisible == true)
        evidence["windowUnoccluded"] = String(window?.occlusionState.contains(.visible) == true)
        do { try JSONEncoder().encode(evidence).write(to: output.appendingPathComponent("phase-evidence.json"), options: .withoutOverwriting) }
        catch { if failure == nil { failure = error } }
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
