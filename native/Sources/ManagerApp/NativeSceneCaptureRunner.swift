// SPDX-License-Identifier: Apache-2.0
import AppKit
import CryptoKit
import Darwin
import Foundation
import ManagerCore
import SwiftUI

/// One local, saved-scene CLI invocation on the normal owned Native WK host.
/// It never exposes credentials, saves a document, or changes a camera contract.
@MainActor
final class NativeSceneCaptureRunner: ObservableObject {
    static let argument = "--native-scene-capture-request"
    let requested: Bool
    private let requestPath: String?
    @Published private(set) var captureModel: ExhibitionModel?
    @Published private(set) var message = "장면 이미지를 준비하고 있습니다."
    private var operation: Task<Void, Never>?
    private(set) var mayTerminate = false
    private var started = false

    init(arguments: [String] = CommandLine.arguments) {
        let positions = arguments.indices.filter { arguments[$0] == Self.argument }
        requested = !positions.isEmpty
        if positions.count == 1, let index = positions.first, index + 1 < arguments.count,
           arguments[index + 1].hasPrefix("/") { requestPath = arguments[index + 1] }
        else { requestPath = nil }
    }
    func start() {
        guard requested, !started else { return }; started = true
        operation = Task { await execute() }
    }
    func cancel() { operation?.cancel() }

    private static func profileURL() throws -> URL {
        #if DEBUG
        let fixed = Bundle.main.object(forInfoDictionaryKey: "ExhibitOSValidationProfile") as? String
        #else
        let fixed: String? = nil
        #endif
        if let path = fixed ?? ProcessInfo.processInfo.environment["EXHIBITOS_MANAGER_PROFILE"] {
            guard path.hasPrefix("/") else { throw BridgeFailure("NATIVE_PROFILE_REQUIRED") }
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        return try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
            .appendingPathComponent("org.exhibitos.manager", isDirectory: true)
    }
    private static func load(_ path: String) throws -> NativeSceneCaptureRequest {
        let fd = Darwin.open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_REQUEST_INVALID") }
        let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true); defer { try? file.close() }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_uid == geteuid(),
              info.st_nlink == 1, info.st_mode & 0o077 == 0, info.st_size > 0, info.st_size <= 65_536 else {
            throw AuthoringFailure("NATIVE_SCENE_CAPTURE_REQUEST_INVALID")
        }
        return try JSONDecoder().decode(NativeSceneCaptureRequest.self, from: file.readToEnd() ?? Data())
    }
    private static func outputIdentity(_ path: String, excluding profile: URL) throws -> (UInt64, UInt64) {
        let url = URL(fileURLWithPath: path, isDirectory: true).standardizedFileURL
        let profilePath = profile.resolvingSymlinksInPath().path
        let bundlePath = Bundle.main.bundleURL.resolvingSymlinksInPath().path
        guard url.path == path, url.resolvingSymlinksInPath().path == path,
              path != profilePath, !path.hasPrefix(profilePath + "/"),
              path != bundlePath, !path.hasPrefix(bundlePath + "/") else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_INVALID") }
        var info = stat()
        guard lstat(path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR, info.st_uid == geteuid(),
              info.st_mode & 0o777 == 0o700 else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_INVALID") }
        return (UInt64(truncatingIfNeeded: info.st_dev), UInt64(info.st_ino))
    }
    private static func stateBytes(_ value: NativeDocumentState) throws -> Data {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(value)
    }
    private static func code(_ error: Error) -> String {
        if error is CancellationError { return "NATIVE_SCENE_CAPTURE_CANCELLED" }
        let value = (error as? AuthoringFailure)?.code ?? (error as? BridgeFailure)?.code ?? "NATIVE_SCENE_CAPTURE_FAILED"
        return value.range(of: "^[A-Z][A-Z0-9_]{0,79}$", options: .regularExpression) != nil ? value : "NATIVE_SCENE_CAPTURE_FAILED"
    }
    private static func writeReceipt(_ value: DocumentValue, directory: String) throws {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(value)
        guard data.count <= 192 * 1024 else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_INVALID") }
        let parent = Darwin.open(directory, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard parent >= 0 else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_INVALID") }
        defer { Darwin.close(parent) }
        let fd = openat(parent, "capture.json", O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard fd >= 0 else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_EXISTS") }
        let output = FileHandle(fileDescriptor: fd, closeOnDealloc: true); defer { try? output.close() }
        try output.write(contentsOf: data)
        guard fsync(fd) == 0 else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_INVALID") }
    }
    nonisolated private static func waitForRuntime(_ bridge: NativeBridge, context: Context, ready: Bool) async throws -> NativeHostStatus {
        let deadline = ContinuousClock.now.advanced(by: .seconds(120))
        while ContinuousClock.now < deadline {
            try Task.checkCancellation()
            let status = try await bridge.nativeStatus(context)
            if !status.busy {
                if ready, let code = status.errorCode { throw BridgeFailure(code) }
                if ready ? status.runtime?.readiness.ready == true : status.runtime?.hasOwnedProcesses != true { return status }
            }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw BridgeFailure("NATIVE_SCENE_CAPTURE_RUNTIME_TIMEOUT")
    }
    nonisolated private static func cleanup(_ bridge: NativeBridge, context: Context?, runtimeStarted: Bool) async -> (Bool, Bool, String?) {
        var drained = !runtimeStarted, failure: String?
        if runtimeStarted, let context {
            do {
                _ = try await bridge.stop(context)
                let stopped = try await waitForRuntime(bridge, context: context, ready: false)
                drained = !stopped.needsShutdown && stopped.runtime?.hasOwnedProcesses != true && !stopped.busy
                if !drained { failure = "NATIVE_SCENE_CAPTURE_STOP_PENDING" }
            } catch { failure = "NATIVE_SCENE_CAPTURE_STOP_FAILED" }
        }
        do { try await bridge.close(); return (drained, true, failure) }
        catch { return (drained, false, "NATIVE_SCENE_CAPTURE_CLOSE_FAILED") }
    }
    private func execute() async {
        var request: NativeSceneCaptureRequest?, bridge: NativeBridge?, context: Context?
        var profile: URL?, outputIdentity: (UInt64, UInt64)?, runtimeStarted = false
        var image: SceneSnapshotResult?, document: NativeDocumentState?, failure: String?
        var preserved = false
        do {
            guard Bundle.main.bundleURL.pathExtension == "app", NSApp.isRunning, let requestPath else {
                throw AuthoringFailure("NATIVE_SCENE_CAPTURE_APP_HOST_REQUIRED")
            }
            let input = try Self.load(requestPath); request = input
            guard input.expectedNativeRevision <= UInt64(Int64.max) else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_REQUEST_INVALID") }
            let root = try Self.profileURL(); profile = root
            outputIdentity = try Self.outputIdentity(input.outputDirectory, excluding: root)
            for name in ["scene.png", "capture.json"] {
                guard !FileManager.default.fileExists(atPath: URL(fileURLWithPath: input.outputDirectory).appendingPathComponent(name).path) else {
                    throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_EXISTS")
                }
            }
            let owned = try NativeBridge(helper: Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/exhibitos-native"),
                profile: root, requireExistingManagedProfile: true)
            bridge = owned
            let selected = try await owned.context(); context = selected
            let initial = try await owned.nativeStatus(selected)
            guard !initial.needsShutdown, !initial.busy, initial.runtime?.hasOwnedProcesses != true else {
                throw AuthoringFailure("NATIVE_SCENE_CAPTURE_STATE_CHANGED")
            }
            if let initialConfiguration = initial.configuration, initialConfiguration.manifestSha256 != input.runtimeManifestSHA256 {
                throw AuthoringFailure("NATIVE_SCENE_CAPTURE_STATE_CHANGED")
            }
            let local = try await owned.documentState(selected)
            try input.validate(document: local)
            let before = try Self.stateBytes(local); document = local
            runtimeStarted = true
            _ = try await owned.start(selected)
            let status = try await Self.waitForRuntime(owned, context: selected, ready: true)
            guard let configuration = status.configuration, configuration.manifestSha256 == input.runtimeManifestSHA256 else {
                throw AuthoringFailure("NATIVE_SCENE_CAPTURE_STATE_CHANGED")
            }
            let credentials = try await owned.authoringSession(selected)
            guard credentials.tenantId == input.tenantId, credentials.principalId == input.principalId else {
                throw AuthoringFailure("NATIVE_SCENE_CAPTURE_STATE_CHANGED")
            }
            let api = try AuthoringAPI(port: configuration.ports.web, tenantId: credentials.tenantId,
                nativeRuntimeManifestSHA256: configuration.manifestSha256)
            let session = try await api.connect(credentials)
            let saved = try await api.exhibition(input.exhibitionId)
            try input.validate(document: local, saved: saved)
            let model = ExhibitionModel()
            let selectedOEX = status.selectedOex == true
            let bounded = selected.installations.first(where: { $0.id == selected.activeId })?.kind == "oex-import"
            await model.connect(api: api, session: session, credentials: credentials,
                selectedBoundedPolicy: selectedOEX, ordinaryBoundedRuntime: bounded && !selectedOEX,
                boundedRuntimeQualified: status.oex027Qualified == true,
                read: { try await owned.documentState(selected) },
                write: { _ in throw BridgeFailure("NATIVE_SCENE_CAPTURE_MUST_NOT_WRITE") })
            await model.oexImport.connect(api: api, session: session,
                read: { try await owned.oexImportState(selected) },
                save: { _ in throw BridgeFailure("NATIVE_SCENE_CAPTURE_MUST_NOT_WRITE") })
            if bounded && !selectedOEX { await model.prepareOrdinaryBoundedPlacement() }
            model.viewportTransparent = false
            captureModel = model
            NSApp.activate(ignoringOtherApps: true)
            NSApp.windows.first(where: { $0.isVisible })?.makeKeyAndOrderFront(nil)
            let deadline = ContinuousClock.now.advanced(by: .seconds(60))
            while image == nil {
                while !NSApp.isActive || !NSApp.windows.contains(where: { $0.isVisible && $0.occlusionState.contains(.visible) }) || !model.canCaptureSceneSnapshot || model.viewportController.loaded != saved.draft.placements.count {
                    try Task.checkCancellation()
                    guard ContinuousClock.now < deadline else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_ACK_TIMEOUT") }
                    try await Task.sleep(for: .milliseconds(100))
                }
                try Task.checkCancellation()
                guard ContinuousClock.now < deadline else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_ACK_TIMEOUT") }
                do {
                    image = try await model.captureSceneSnapshot(camera: .savedViewpoint(input.viewpointId), placementId: input.placementId)
                } catch let error as AuthoringFailure where error.code == "SCENE_SNAPSHOT_NOT_READY" {
                    // The renderer rejects before encoding while its furnishing
                    // build is pending. Never retry stale/rights/integrity errors.
                    try await Task.sleep(for: .milliseconds(100))
                }
            }
            let after = try await owned.documentState(selected)
            try input.validate(document: after, saved: saved)
            guard try Self.stateBytes(after) == before else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_STATE_CHANGED") }
            preserved = true
        } catch { failure = Self.code(error) }
        captureModel?.viewportController.editingContextActive = false
        captureModel = nil
        await Task.yield()
        var drained = !runtimeStarted, closed = bridge == nil
        if let bridge {
            let cleanupContext = context, shouldStop = runtimeStarted
            let cleanup = await Task.detached { await Self.cleanup(bridge, context: cleanupContext, runtimeStarted: shouldStop) }.value
            drained = cleanup.0; closed = cleanup.1
            if let code = cleanup.2 { failure = code }
        }
        if !drained || !closed { failure = failure ?? "NATIVE_SCENE_CAPTURE_CLOSE_FAILED" }
        var receipt: [String: DocumentValue] = ["version": .integer(1), "status": .string("failed"),
            "pid": .integer(Int64(ProcessInfo.processInfo.processIdentifier)),
            "bundleId": .string(Bundle.main.bundleIdentifier ?? ""),
            "runtimeDrained": .bool(drained), "helperClosed": .bool(closed), "documentUnchanged": .bool(preserved)]
        if let request { receipt["requestId"] = .string(request.requestId) }
        do {
            if let request, let profile, let outputIdentity {
                let current = try Self.outputIdentity(request.outputDirectory, excluding: profile)
                guard current == outputIdentity else { throw AuthoringFailure("NATIVE_SCENE_CAPTURE_OUTPUT_INVALID") }
                if failure == nil, let image, let document {
                    try await image.writeNewPNG(to: URL(fileURLWithPath: request.outputDirectory).appendingPathComponent("scene.png"))
                    if let furnishings = image.furnishings { receipt["furnishings"] = furnishings }
                    receipt.merge(["status": .string("succeeded"), "workspaceId": .string(document.workspaceId),
                        "tenantId": .string(document.tenantId), "principalId": .string(document.principalId),
                        "nativeRevision": .integer(Int64(request.expectedNativeRevision)), "editVersion": .integer(image.localEditVersion),
                        "exhibitionId": .string(request.exhibitionId), "serverRevision": .integer(Int64(image.savedRevision)),
                        "etag": .string(image.savedETag), "runtimeManifestSHA256": .string(request.runtimeManifestSHA256),
                        "placementId": .string(request.placementId), "viewpointId": .string(request.viewpointId),
                        "candidateSha256": .string(image.candidateSha256), "camera": image.camera, "target": image.target,
                        "png": .object(["file": .string("scene.png"), "bytes": .integer(Int64(image.png.count)),
                            "sha256": .string(SHA256.hash(data: image.png).map { String(format: "%02x", $0) }.joined()),
                            "width": .integer(Int64(SceneSnapshotViewportRequest.width)), "height": .integer(Int64(SceneSnapshotViewportRequest.height))])]) { _, new in new }
                }
                if let failure { receipt["errorCode"] = .string(failure) }
                try Self.writeReceipt(.object(receipt), directory: request.outputDirectory)
            }
        } catch { failure = Self.code(error) }
        mayTerminate = closed
        message = failure == nil ? "장면 이미지 준비를 마쳤습니다." : "장면 이미지를 준비하지 못했습니다. " + (failure ?? "")
        if mayTerminate { NSApp.terminate(nil) }
    }
}

struct NativeSceneCaptureView: View {
    @ObservedObject var runner: NativeSceneCaptureRunner
    var body: some View {
        Group {
            if let model = runner.captureModel {
                NativeViewportArea(model: model, ready: true, controller: model.viewportController, workbench: true)
                    .allowsHitTesting(false)
            } else {
                VStack(spacing: 12) { ProgressView(); Text(runner.message) }.padding(32)
            }
        }.frame(minWidth: 640, minHeight: 420)
    }
}
