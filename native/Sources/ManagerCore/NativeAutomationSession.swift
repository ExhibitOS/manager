// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import Darwin

/// Command adapter to the same native document journal and authenticated API
/// used by ManagerApp. Each installed CLI admits only its scoped commands.
public actor NativeAutomationSession {
    private static let freshResponseOperations: Set<String> = [
        "thumbnail-status", "required-model-original-status", "required-model-original-discover",
        "artwork-status", "artwork-icc-validation-status", "artwork-prepare-reviewed", "artwork-replace-reviewed",
        "hayes-original-release", "hayes-finished-release", "workspace-frame-lineage-status",
        "workspace-frame-lineage-prepare", "export-static-native-original", "export-static-saved-head", "export-static-status",
        "compact-model-prepare", "compact-model-status", "compact-model-retry", "compact-model-cancel", "compact-model-approve", "compact-model-refresh-placement"
    ]

    static func replayResponse(for operation: String, prior: (Data, DocumentValue)?, matching request: Data) throws -> DocumentValue? {
        guard !freshResponseOperations.contains(operation), let prior else { return nil }
        guard prior.0 == request else { throw BridgeFailure("AUTOMATION_REQUEST_CHANGED") }
        return prior.1
    }

    public struct ExistingWorkspaceV5: Decodable, Sendable {
        public let workspaceId: String
        public let tenantId: String
        public let principalId: String
        public let manifestSHA256: String

        func check() throws {
            guard [workspaceId, tenantId, principalId].allSatisfy({ UUID(uuidString: $0)?.uuidString.lowercased() == $0 }),
                  nativeImportDigest(manifestSHA256) else { throw BridgeFailure("AUTOMATION_CONFIGURATION_INVALID") }
        }
    }

    private var bridge: NativeBridge
    private var bridgeOpen = true
    private var closeFailure: Error?
    private var closing = false
    private var artworkReplacementRecoveryOnly = false
    private let helper: URL
    private let profile: URL
    private let temporaryRoot: URL
    private let ports: [String: UInt16]
    private let workspaceV5ValidationRuntime: URL?
    private let workspaceV6ValidationRuntime: URL?
    private let workspaceV7ValidationRuntime: URL?
    private let existingWorkspaceV5: ExistingWorkspaceV5?
    private let root: URL
    private let installedAuthoringCLI: Bool
    private let installedImageCLI: Bool
    private let installedExportRootIdentity: (dev_t, ino_t)?
    private let requiresReleaseImageRuntime: Bool
    private let inputs: [String: String]
    private let capability = UUID().uuidString.lowercased()
    private var context: Context?
    private var api: AuthoringAPI?
    private var authoringSession: AuthoringSession?
    private var runtimeManifest: String?
    private var identity: NativeDocumentState?
    private var closed = false
    private var busy = false
    private var replay: [String: (Data, DocumentValue)] = [:]
    private var seenRequests: [String: Data] = [:]
    private var proposals: [String: BenchViewportRequest] = [:]
    private var sharedBenchProposals: [String: SharedFurnishingViewportRequest] = [:]
    private var surfaceProposals: [String: SurfaceMaterialTextureRequest] = [:]
    private var plaqueProposals: [String: (PlaquePreview, String)] = [:]
    private var wallFinishProposals: [String: (WallFinishPreview, String)] = [:]
    private enum SceneProposal: Sendable {
        case scene(NativeAutomationScene.ScenePreview)
        case light(LightPreview)
        case fixture(NativeAutomationFixture.FixturePreview)
        func apply(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
            switch self {
            case .scene(let preview): return try preview.apply(to: draft)
            case .light(let preview): return try preview.apply(to: draft, selectedLightId: preview.command.lightId, command: preview.command)
            case .fixture(let preview): return try preview.apply(to: draft)
            }
        }
    }
    private var sceneProposals: [String: (SceneProposal, String, String)] = [:]
    private var pendingRevisions: [String: UInt64] = [:]
    private var staticIntents: [String: NativePublicationPending] = [:]
    private var selectedOEXFormat: String?
    private var runtimeStopped = false

    public init(helper: URL, profile: URL, ports: [String: UInt16], outputRoot: URL, temporaryRoot: URL, inputs: [String: String], selectedOEX9: Bool = false, selectedOEX10: Bool = false, workspaceV5ValidationRuntime: URL? = nil, existingWorkspaceV5: ExistingWorkspaceV5? = nil, workspaceV6ValidationRuntime: URL? = nil, workspaceV7ValidationRuntime: URL? = nil) throws {
        let draft12 = workspaceV6ValidationRuntime != nil || workspaceV7ValidationRuntime != nil
        let ordinaryExisting = !draft12 && workspaceV5ValidationRuntime == nil
            && existingWorkspaceV5.map {
                NativeHayesOriginalIntent.supports(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsProductImageRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsFreshImageRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsLargeModelRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsInfoStandPreparationRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsPreReadyPreservationReadback(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsOrdinaryNativePublicationRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsDefaultSeatRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsCarriedRestoreBenchRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsWalkingCollisionRuntime(manifest: $0.manifestSHA256)
                    || AuthoringAPI.supportsRenderedSculptureThumbnailOexRuntime(manifest: $0.manifestSHA256)
            } == true
        guard !(selectedOEX9 && selectedOEX10),
              workspaceV6ValidationRuntime == nil || workspaceV7ValidationRuntime == nil,
              !draft12 || (!selectedOEX9 && !selectedOEX10 && workspaceV5ValidationRuntime == nil),
              existingWorkspaceV5 == nil || (!selectedOEX9 && !selectedOEX10
                  && (workspaceV5ValidationRuntime != nil || workspaceV7ValidationRuntime != nil || ordinaryExisting)),
              workspaceV5ValidationRuntime == nil || selectedOEX10 || existingWorkspaceV5 != nil else {
            throw BridgeFailure("AUTOMATION_CONFIGURATION_INVALID")
        }
        try existingWorkspaceV5?.check()
        if existingWorkspaceV5 != nil { _ = try Self.profileIdentity(profile) }
        guard outputRoot.isFileURL, outputRoot.path.hasPrefix("/"),
              FileManager.default.fileExists(atPath: outputRoot.path),
              outputRoot.resolvingSymlinksInPath().path == outputRoot.path,
              !profile.path.hasPrefix(outputRoot.path + "/") else { throw BridgeFailure("AUTOMATION_PATH_INVALID") }
        self.root = outputRoot; self.inputs = inputs; self.installedImageCLI = false
        self.installedAuthoringCLI = false
        self.installedExportRootIdentity = nil
        self.requiresReleaseImageRuntime = false
        self.helper = helper; self.profile = profile; self.temporaryRoot = temporaryRoot; self.ports = ports
        self.workspaceV5ValidationRuntime = workspaceV5ValidationRuntime
        self.workspaceV6ValidationRuntime = workspaceV6ValidationRuntime
        self.existingWorkspaceV5 = existingWorkspaceV5
        self.workspaceV7ValidationRuntime = workspaceV7ValidationRuntime
        let selectedDraft12 = draft12 && existingWorkspaceV5 == nil
        selectedOEXFormat = selectedDraft12 ? "1.0.0-draft.12" : selectedOEX10 ? "1.0.0-draft.10" : selectedOEX9 ? "1.0.0-draft.9" : nil
        for url in [outputRoot, temporaryRoot] {
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard attributes[.type] as? FileAttributeType == .typeDirectory,
                  (attributes[.posixPermissions] as? NSNumber)?.intValue == 0o700,
                  (attributes[.ownerAccountID] as? NSNumber)?.uint32Value == getuid(),
                  url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_PATH_INVALID") }
        }
        bridge = try NativeBridge(helper: helper, profile: profile, freshSelectedOEX: selectedOEXFormat != nil,
                                  freshWorkspacePorts: existingWorkspaceV5 == nil ? ports : nil, temporaryDirectory: temporaryRoot,
                                  workspaceV5ValidationRuntime: workspaceV5ValidationRuntime, workspaceV6ValidationRuntime: workspaceV6ValidationRuntime,
                                  workspaceV7ValidationRuntime: workspaceV7ValidationRuntime)
    }
    /// The installed entry supplies its bundled helper and the default or
    /// explicitly selected managed profile.
    public init(installedImageCLIHelper helper: URL, profile: URL, requireExistingManagedProfile: Bool = false) throws {
        try self.init(installedHelper: helper, profile: profile, requireExistingManagedProfile: requireExistingManagedProfile,
                      exportRoot: nil)
    }

    public init(installedExportCLIHelper helper: URL, profile: URL, outputRoot: URL) throws {
        try self.init(installedHelper: helper, profile: profile, requireExistingManagedProfile: true, exportRoot: outputRoot)
    }

    public init(installedAuthoringCLIHelper helper: URL, profile: URL, outputRoot: URL) throws {
        try self.init(installedHelper: helper, profile: profile, requireExistingManagedProfile: true, exportRoot: outputRoot, authoring: true)
    }

    private init(installedHelper helper: URL, profile: URL, requireExistingManagedProfile: Bool, exportRoot: URL?, authoring: Bool = false) throws {
        var metadata = stat()
        if lstat(profile.path, &metadata) != 0 && errno == ENOENT {
            throw BridgeFailure("IMAGE_CLI_PERSONAL_WORKSPACE_REQUIRED")
        }
        _ = try Self.profileIdentity(profile)
        self.helper = helper
        self.profile = profile
        self.root = exportRoot ?? profile
        self.temporaryRoot = profile
        self.ports = [:]
        self.inputs = [:]
        self.installedAuthoringCLI = authoring
        self.installedImageCLI = exportRoot == nil
        self.installedExportRootIdentity = try exportRoot.map { try Self.exportRootIdentity($0, profile: profile) }
        self.requiresReleaseImageRuntime = requireExistingManagedProfile
        self.workspaceV5ValidationRuntime = nil
        self.workspaceV6ValidationRuntime = nil
        self.workspaceV7ValidationRuntime = nil
        self.existingWorkspaceV5 = nil
        bridge = try NativeBridge(helper: helper, profile: profile,
                                  requireExistingManagedProfile: requireExistingManagedProfile)
    }

    private func value<T: Encodable>(_ v: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(v))
    }
    public static func digest(_ bytes: Data) -> String { SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined() }
    private func waitReady() async throws -> NativeHostStatus {
        guard let context else { throw BridgeFailure("AUTOMATION_NOT_READY") }
        for _ in 0..<1200 {
            try Task.checkCancellation()
            let status = try await bridge.nativeStatus(context)
            if !status.busy {
                if let code = status.errorCode { throw BridgeFailure(code) }
                return status
            }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw BridgeFailure("NATIVE_TIMEOUT")
    }
    private func waitStartup(manifest: String) async throws -> NativeHostStatus {
        guard manifest == "f738f33c777de39629b6a0ab2dd3a477b45d078b738189e173d00d2cd7ab652a"
            || AuthoringAPI.supportsRenderedSculptureThumbnailOexRuntime(manifest: manifest)
            || AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest) else {
            return try await waitReady()
        }
        // These runtimes own their startup deadlines. Keep the helper channel
        // open while busy, including when the calling task is cancelled.
        guard let context else { throw BridgeFailure("AUTOMATION_NOT_READY") }
        let ownedBridge = bridge
        let status = try await Task.detached {
            while true {
                let status = try await ownedBridge.nativeStatus(context)
                if !status.busy {
                    if let code = status.errorCode { throw BridgeFailure(code) }
                    return status
                }
                try await Task.sleep(for: .milliseconds(100))
            }
        }.value
        try Task.checkCancellation()
        return status
    }
    public func open() async throws -> DocumentValue {
        guard context == nil, !closed else { throw BridgeFailure("AUTOMATION_ALREADY_OPEN") }
        let context = try await bridge.context(); self.context = context
        if installedImageCLI || installedExportRootIdentity != nil {
            return try await openInstalledImageWorkspace(context)
        }
        if let existingWorkspaceV5 { return try await openExistingWorkspace(existingWorkspaceV5, context: context) }
        var preparedManifest: String?
        if selectedOEXFormat == nil {
            _ = try await bridge.prepare(context)
            preparedManifest = try await waitReady().configuration?.manifestSha256
        }
        let preparedDefaultSeatRuntime = preparedManifest.map(AuthoringAPI.supportsDefaultSeatRuntime) == true
        _ = try await bridge.start(context)
        let status: NativeHostStatus
        if let preparedManifest, preparedDefaultSeatRuntime {
            status = try await waitStartup(manifest: preparedManifest)
        } else {
            status = try await waitReady()
        }
        guard status.runtime?.readiness.ready == true, let config = status.configuration else { throw BridgeFailure("NATIVE_NOT_READY") }
        let defaultSeatRuntime = preparedDefaultSeatRuntime || AuthoringAPI.supportsDefaultSeatRuntime(manifest: config.manifestSha256)
        guard !defaultSeatRuntime || config.manifestSha256 == preparedManifest else {
            throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
        }
        let credentials = try await bridge.authoringSession(context)
        let api = try AuthoringAPI(port: config.ports.web, tenantId: credentials.tenantId, nativeRuntimeManifestSHA256: config.manifestSha256)
        let session = try await api.connect(credentials)
        if defaultSeatRuntime, !(await api.admitsWorkspaceDefaultSeatProposal) {
            throw AuthoringFailure("WORKSPACE_BENCH_SEAT_UNAVAILABLE")
        }
        authoringSession = session; self.api = api; runtimeManifest = config.manifestSha256
        identity = try await bridge.documentState(context)
        return try await statusValue()
    }
    private func openInstalledImageWorkspace(_ context: Context) async throws -> DocumentValue {
        let directory = try profileIdentity()
        let stopped = try await bridge.nativeStatus(context)
        if let code = stopped.errorCode { throw BridgeFailure(code) }
        guard !stopped.needsShutdown, !stopped.accountRequired, stopped.selectedOex != true,
              let configured = stopped.configuration, configured.kind == "personal",
              stopped.restoration.map({ $0.state == "activated" }) ?? true else {
            throw BridgeFailure("IMAGE_CLI_PERSONAL_WORKSPACE_REQUIRED")
        }
        guard installedAuthoringCLI || installedExportRootIdentity != nil || stopped.freshImageReleaseConfigured == true
                || (!requiresReleaseImageRuntime
                    && AuthoringAPI.supportsFreshImageRuntime(manifest: configured.manifestSha256)) else {
            throw AuthoringFailure("IMAGE_PREPARATION_UNAVAILABLE")
        }
        _ = try await bridge.start(context)
        let ready = try await waitReady()
        guard ready.runtime?.readiness.ready == true, ready.selectedOex != true,
              ready.configuration?.manifestSha256 == configured.manifestSha256,
              ready.configuration?.ports.web == configured.ports.web,
              try profileIdentity() == directory else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        let credentials = try await bridge.authoringSession(context)
        let document = try await bridge.documentState(context)
        guard credentials.tenantId == configured.tenantId, credentials.tenantId == document.tenantId,
              credentials.principalId == document.principalId else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        let api = try AuthoringAPI(port: configured.ports.web, tenantId: credentials.tenantId,
                                  nativeRuntimeManifestSHA256: configured.manifestSha256)
        authoringSession = try await api.connect(credentials)
        if installedAuthoringCLI {
            let canCreate = await api.admitsPersonalExhibitionCreation
            guard Self.installedAuthoringEntryAllowed(canCreate: canCreate, session: authoringSession) else { throw AuthoringFailure("NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE") }
        } else if installedExportRootIdentity != nil {
            guard Self.installedExportEntryAllowed(session: authoringSession) else { throw AuthoringFailure("NATIVE_EXPORT_UNAVAILABLE") }
        } else {
            guard await api.admitsFreshImagePreparation else { throw AuthoringFailure("IMAGE_PREPARATION_UNAVAILABLE") }
        }
        self.api = api; runtimeManifest = configured.manifestSha256; identity = document
        return try await statusValue()
    }

    private func openExistingWorkspace(_ expected: ExistingWorkspaceV5, context: Context) async throws -> DocumentValue {
        let directory = try profileIdentity()
        let status = try await bridge.nativeStatus(context)
        let saved = try await bridge.documentState(context)
        guard !status.needsShutdown, !status.accountRequired, status.selectedOex != true, status.errorCode == nil,
              let config = status.configuration, config.manifestSha256 == expected.manifestSHA256,
              config.tenantId == expected.tenantId, config.ports.web == ports["web"],
              config.ports.api == ports["api"], config.ports.database == ports["database"],
              saved.workspaceId == expected.workspaceId, saved.tenantId == expected.tenantId,
              saved.principalId == expected.principalId, !saved.dirty, let draft = saved.draft, let etag = saved.etag else {
            throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
        }
        _ = try await bridge.start(context)
        let ready = try await waitStartup(manifest: expected.manifestSHA256)
        guard ready.runtime?.readiness.ready == true, ready.selectedOex != true,
              ready.configuration?.manifestSha256 == expected.manifestSHA256,
              try profileIdentity() == directory else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        let credentials = try await bridge.authoringSession(context)
        guard credentials.tenantId == expected.tenantId, credentials.principalId == expected.principalId else {
            throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
        }
        let api = try AuthoringAPI(port: config.ports.web, tenantId: expected.tenantId, nativeRuntimeManifestSHA256: config.manifestSha256)
        let session = try await api.connect(credentials)
        let admitsMaterials = await api.admitsWorkspaceMaterials
        let admitsFreshImages = await api.admitsFreshImagePreparation
        let admitsLargeModels = await api.admitsLargeModelPreparation
        let admitsInfoStandPreparation = await api.admitsWorkspaceInfoStandPreparation
        let admitsSavedHeadAuthoring = await api.admitsSavedHeadAuthoring
        let admitsRenderedSculptureThumbnailOex = await api.admitsRenderedSculptureThumbnailOex
        let admitsNativePublication = AuthoringAPI.supportsOrdinaryNativePublicationRuntime(manifest: config.manifestSha256)
            && session.nativePublicationProfile == "bounded-local-v1"
        let admitsDefaultSeats = await api.admitsWorkspaceDefaultSeatProposal
        let requiresDefaultSeatProfile = AuthoringAPI.supportsDefaultSeatRuntime(manifest: config.manifestSha256)
            || AuthoringAPI.supportsCarriedRestoreBenchRuntime(manifest: config.manifestSha256)
        if requiresDefaultSeatProfile, !admitsDefaultSeats {
            throw AuthoringFailure("WORKSPACE_BENCH_SEAT_UNAVAILABLE")
        }
        let walkingCollisionRuntime = AuthoringAPI.supportsWalkingCollisionRuntime(manifest: config.manifestSha256)
        let admitsWalkingCollision = walkingCollisionRuntime && session.advertisesFurnishingWalkingCollision
        if walkingCollisionRuntime, !admitsWalkingCollision {
            throw AuthoringFailure("FURNISHING_WALKING_COLLISION_UNAVAILABLE")
        }
        let admitsPreservationReadback = AuthoringAPI.supportsPreReadyPreservationReadback(manifest: config.manifestSha256)
            && session.nativeAuthoringProposalProfile == "saved-head-v1"
        guard admitsMaterials || admitsFreshImages || admitsLargeModels || admitsInfoStandPreparation || admitsSavedHeadAuthoring
                || admitsRenderedSculptureThumbnailOex || admitsNativePublication || admitsPreservationReadback || admitsWalkingCollision
                || (requiresDefaultSeatProfile && admitsDefaultSeats) else {
            throw AuthoringFailure("WORKSPACE_MATERIAL_UNAVAILABLE")
        }
        let stored = try await api.exhibition(draft.id)
        guard try value(await bridge.documentState(context)) == value(saved) else {
            throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
        }
        let recovery = try Self.replacementRecoveryAttempt(ordinaryPath: replacementAttemptPath,
            repairPath: repairAttemptPath, document: saved)
        if stored.draft != draft || stored.etag != etag {
            guard !admitsPreservationReadback, let recovery,
                  let command = try? NativeAutomationReplacementCommand(recovery.attempt),
                  await admitsReplacement(command, api: api), stored.revision == command.studioRevision + 1 else {
                throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
            }
            artworkReplacementRecoveryOnly = true
        } else if let recovery {
            // An intent on the original head remains uncertain even when POST
            // may not have happened. Only its exact original replay can proceed.
            guard !admitsPreservationReadback,
                  let command = try? NativeAutomationReplacementCommand(recovery.attempt),
                  await admitsReplacement(command, api: api) else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN") }
            artworkReplacementRecoveryOnly = true
        }
        self.api = api; authoringSession = session; runtimeManifest = config.manifestSha256; identity = saved
        return try await statusValue()
    }
    private func reopenSaved(_ current: NativeDocumentState, context: Context, api: AuthoringAPI) async throws -> DocumentValue {
        guard selectedOEXFormat == nil, !current.dirty, let draft = current.draft, let etag = current.etag,
              let manifest = runtimeManifest else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
        let prior = try await api.exhibition(draft.id)
        guard prior.draft == draft, prior.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        let previous = try await bridge.nativeStatus(context)
        guard previous.runtime?.readiness.ready == true, let previousJob = previous.runtime?.jobId,
              let previousConfiguration = previous.configuration else { throw BridgeFailure("NATIVE_NOT_READY") }
        let installed = installedImageCLI || installedExportRootIdentity != nil
        let expectedPorts = try Self.expectedReopenPorts(previousConfiguration, manifest: manifest,
            tenantId: current.tenantId, requested: ports, installed: installed)
        let directory = try profileIdentity()

        runtimeStopped = true
        proposals.removeAll(); sharedBenchProposals.removeAll(); surfaceProposals.removeAll(); plaqueProposals.removeAll(); wallFinishProposals.removeAll()
        sceneProposals.removeAll()
        _ = try await bridge.stop(context)
        let stopped = try await waitReady()
        guard let runtime = stopped.runtime, runtime.state == "stopped", !runtime.hasOwnedProcesses,
              (workspaceV6ValidationRuntime == nil && workspaceV7ValidationRuntime == nil) || (runtime.exits["node"] == 0 && runtime.exits["postgres"] == 0) else {
            throw BridgeFailure("AUTOMATION_DRAIN_UNCERTAIN")
        }
        try await closeBridge()
        try Task.checkCancellation()
        guard try profileIdentity() == directory else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        bridge = try NativeBridge(helper: helper, profile: profile, temporaryDirectory: temporaryRoot,
                                  workspaceV5ValidationRuntime: workspaceV5ValidationRuntime, workspaceV6ValidationRuntime: workspaceV6ValidationRuntime,
                                  workspaceV7ValidationRuntime: workspaceV7ValidationRuntime,
                                  requireExistingManagedProfile: requiresReleaseImageRuntime)
        bridgeOpen = true
        let reopened = try await bridge.context()
        self.context = reopened
        guard try profileIdentity() == directory, reopened.activeId == context.activeId,
              try value(await bridge.documentState(reopened)) == value(current) else {
            throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED")
        }
        let persisted = try await bridge.nativeStatus(reopened)
        guard let config = persisted.configuration,
              !persisted.busy, persisted.runtime?.hasOwnedProcesses != true else {
            throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
        }
        try Self.checkReopenedConfiguration(config, previous: previousConfiguration, ports: expectedPorts)
        try Task.checkCancellation()
        _ = try await bridge.start(reopened)
        let pinnedStartup = existingWorkspaceV5 != nil || AuthoringAPI.supportsDefaultSeatRuntime(manifest: manifest)
        let ready = try await (pinnedStartup ? waitStartup(manifest: manifest) : waitReady())
        guard ready.runtime?.readiness.ready == true, let job = ready.runtime?.jobId, job != previousJob,
              let readyConfiguration = ready.configuration else { throw BridgeFailure("NATIVE_NOT_READY") }
        try Self.checkReopenedConfiguration(readyConfiguration, previous: previousConfiguration, ports: expectedPorts)
        let credentials = try await bridge.authoringSession(reopened)
        let reopenedAPI = try AuthoringAPI(port: config.ports.web, tenantId: current.tenantId, nativeRuntimeManifestSHA256: manifest)
        let session = try await reopenedAPI.connect(credentials)
        if (AuthoringAPI.supportsDefaultSeatRuntime(manifest: manifest)
                || AuthoringAPI.supportsCarriedRestoreBenchRuntime(manifest: manifest)),
           !(await reopenedAPI.admitsWorkspaceDefaultSeatProposal) {
            throw AuthoringFailure("WORKSPACE_BENCH_SEAT_UNAVAILABLE")
        }
        if AuthoringAPI.supportsWalkingCollisionRuntime(manifest: manifest), !session.advertisesFurnishingWalkingCollision {
            throw AuthoringFailure("FURNISHING_WALKING_COLLISION_UNAVAILABLE")
        }
        guard try profileIdentity() == directory, session.userId == current.principalId,
              try value(await bridge.documentState(reopened)) == value(current) else {
            throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
        }
        try prior.requireSameComparison(await reopenedAPI.exhibition(draft.id))
        self.api = reopenedAPI
        authoringSession = session
        runtimeStopped = false
        return .object(["document": try value(current), "previousRuntimeJobId": .string(previousJob),
                        "runtimeJobId": .string(job), "studioRevision": .integer(Int64(prior.revision))])
    }
    private func profileIdentity() throws -> (dev_t, ino_t) {
        try Self.profileIdentity(profile)
    }
    private static func profileIdentity(_ profile: URL) throws -> (dev_t, ino_t) {
        var metadata = stat()
        guard profile.resolvingSymlinksInPath().path == profile.path, lstat(profile.path, &metadata) == 0,
              metadata.st_mode & S_IFMT == S_IFDIR, metadata.st_mode & 0o777 == 0o700,
              metadata.st_uid == getuid() else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        return (metadata.st_dev, metadata.st_ino)
    }
    private func statusValue() async throws -> DocumentValue {
        guard let context else { throw BridgeFailure("AUTOMATION_NOT_READY") }
        let state = try await bridge.documentState(context)
        return .object(["workspaceId": .string(state.workspaceId), "tenantId": .string(state.tenantId), "principalId": .string(state.principalId),
                        "capability": .string(capability), "revision": .integer(Int64(state.revision)), "document": try value(state), "closed": .bool(closed)])
    }
    private func checked(_ request: DocumentValue) async throws -> (Context, AuthoringAPI, NativeDocumentState) {
        guard !closed, !closing, closeFailure == nil, let context, let api, let identity,
              request["capability"].string == capability,
              request["workspaceId"].string == identity.workspaceId,
              request["tenantId"].string == identity.tenantId,
              request["principalId"].string == identity.principalId else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        let current = try await bridge.documentState(context)
        if installedImageCLI || installedExportRootIdentity != nil {
            guard current.workspaceId == identity.workspaceId, current.tenantId == identity.tenantId,
                  current.principalId == identity.principalId else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        }
        return (context, api, current)
    }
    private func checkedRevision(_ request: DocumentValue, _ state: NativeDocumentState) throws {
        try Self.checkDocumentRevision(request, state)
    }
    private static func checkDocumentRevision(_ request: DocumentValue, _ state: NativeDocumentState) throws {
        guard case .object(let fields) = request, fields["expectedRevision"] != nil, fields["expectedEditVersion"] != nil,
              request["expectedRevision"].integer == Int64(state.revision),
              request["expectedEditVersion"] == (state.draft?.document["editVersion"] ?? .null) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
    }
    private func thumbnailCandidate(_ request: DocumentValue, id: String, context: Context,
                                    api: AuthoringAPI, current: NativeDocumentState) async throws -> DocumentValue {
        try checkedRevision(request, current)
        guard let session = authoringSession, await api.hasCatalogThumbnailAuthority(session),
              let operation = request["operation"].string else { throw BridgeFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let authority = CatalogThumbnailAuthority()
        defer { authority.invalidate() }
        if operation == "thumbnail-prepare" {
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "artworkRevision"])
            guard let artworkId = command["artworkId"].string, let revision = command["artworkRevision"].integer,
                  (1...Int64(Int32.max)).contains(revision), !current.dirty, current.pending == nil,
                  let draft = current.draft, let etag = current.etag else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            let candidatePath = root.appendingPathComponent("thumbnail-\(id).json").path
            let pngPath = root.appendingPathComponent("thumbnail-\(id).png").path
            if FileManager.default.fileExists(atPath: candidatePath) {
                let stored = try Self.sharedBenchReply(root: root, path: candidatePath)
                let candidate = try JSONDecoder().decode(NativeThumbnailCandidate.self, from: JSONEncoder().encode(stored))
                let rendered = try candidate.rendered(in: current)
                guard rendered.artworkId == artworkId, rendered.revision == Int(revision) else { throw BridgeFailure("AUTOMATION_REQUEST_CHANGED") }
                try NativeThumbnailCandidate.materializePreview(rendered.pngData, root: root, name: URL(fileURLWithPath: pngPath).lastPathComponent)
                return .object(["candidatePath": .string(candidatePath), "previewPath": .string(pngPath),
                    "pngSha256": .string(rendered.pngSha256), "selectionVersion": .integer(Int64(candidate.selectionVersion)), "selected": .bool(false)])
            }
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let selection = try await api.artworkThumbnailSelection(artworkId: artworkId, revision: Int(revision), session: session, authority: authority)
            let credentials = try await api.thumbnailRendererCredentials(for: session)
            let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1,
                tenantId: current.tenantId, principalId: current.principalId, draft: draft,
                selection: nil, mode: "translate", view: "isometric", disabled: false,
                workspaceId: current.workspaceId, baseETag: etag)
            let renderer = await NativeThumbnailRenderer()
            let (renderRequest, reply) = try await withTaskCancellationHandler {
                try await renderer.render(credentials: credentials, frame: frame, artworkId: artworkId, revision: Int(revision))
            } onCancel: {
                Task { @MainActor in renderer.cancel() }
            }
            let (_, _, refreshed) = try await checked(request)
            guard try value(refreshed) == value(current), await api.hasCatalogThumbnailAuthority(session) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
            let candidate = NativeThumbnailCandidate(workspaceId: current.workspaceId, tenantId: current.tenantId,
                principalId: current.principalId, selectionVersion: selection.selectionVersion, request: renderRequest, reply: reply)
            let rendered = try candidate.rendered(in: current)
            try candidate.persist(root: root, name: URL(fileURLWithPath: candidatePath).lastPathComponent)
            try NativeThumbnailCandidate.materializePreview(rendered.pngData, root: root, name: URL(fileURLWithPath: pngPath).lastPathComponent)
            return .object(["candidatePath": .string(candidatePath), "previewPath": .string(pngPath),
                "pngSha256": .string(rendered.pngSha256), "selectionVersion": .integer(Int64(selection.selectionVersion)), "selected": .bool(false)])
        }
        let command = try NativeAutomationArtworkRules.command(request, keys: ["candidatePath", "expectedPngSha256"])
        guard let path = command["candidatePath"].string, let expected = command["expectedPngSha256"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        let stored = try Self.sharedBenchReply(root: root, path: path)
        let candidate = try JSONDecoder().decode(NativeThumbnailCandidate.self, from: JSONEncoder().encode(stored))
        let rendered = try candidate.rendered(in: current, expectedSHA256: expected)
        let preview = URL(fileURLWithPath: path).deletingPathExtension().appendingPathExtension("png")
        try NativeThumbnailCandidate.materializePreview(rendered.pngData, root: root, name: preview.lastPathComponent)
        if operation == "thumbnail-status" {
            let selection = try await api.artworkThumbnailSelection(artworkId: rendered.artworkId, revision: rendered.revision, session: session, authority: authority)
            return .object(["candidate": stored, "selection": try value(selection)])
        }
        guard operation == "thumbnail-select", let selectionID = UUID(uuidString: id) else { throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED") }
        let receipt = try await api.saveRenderedArtworkThumbnail(rendered, requestId: selectionID,
            expectedSelectionVersion: candidate.selectionVersion, session: session, authority: authority)
        let selection = try await api.artworkThumbnailSelection(artworkId: rendered.artworkId, revision: rendered.revision, session: session, authority: authority)
        guard selection.bindingId == receipt.id, selection.selectionVersion == receipt.selectionVersion else {
            throw BridgeFailure("CATALOG_SELECTION_READBACK_UNCERTAIN")
        }
        return .object(["receipt": try value(receipt), "selection": try value(selection)])
    }
    private func furnishingThumbnail(_ request: DocumentValue, context: Context,
                                      api: AuthoringAPI, current: NativeDocumentState) async throws -> DocumentValue {
        try checkedRevision(request, current)
        let command = try NativeAutomationArtworkRules.command(request, keys: ["id", "version", "modelSha256"])
        guard selectedOEXFormat == nil, !current.dirty, current.pending == nil,
              let draft = current.draft, let etag = current.etag, let session = authoringSession,
              await api.hasFurnishingThumbnailAuthority(session), command["version"] == .integer(1),
              let id = command["id"].string, workspaceFurnishingID(id), UUID(uuidString: id) != nil,
              let expectedHash = command["modelSha256"].string else { throw BridgeFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let saved = try await api.exhibition(draft.id)
        guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        let item = try await api.workspaceFurnishing(id, version: 1)
        guard item.id == id, item.version == 1, item.details.model.sha256 == expectedHash else { throw BridgeFailure("MODEL_CONFLICT") }
        try NativeFurnishingThumbnailRules.requireDisplay(item.details.rights)
        let credentials = try await api.furnishingThumbnailRendererCredentials(for: session)
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1,
            tenantId: current.tenantId, principalId: current.principalId, draft: draft,
            selection: nil, mode: "translate", view: "isometric", disabled: false,
            workspaceId: current.workspaceId, baseETag: etag)
        let renderer = await NativeFurnishingThumbnailRenderer()
        let capture = try await withTaskCancellationHandler {
            try await renderer.render(credentials: credentials, frame: frame, furnishing: item)
        } onCancel: {
            Task { @MainActor in renderer.cancel() }
        }
        try Task.checkCancellation()
        let (_, _, refreshed) = try await checked(request)
        let currentItem = try await api.workspaceFurnishing(id, version: 1)
        let currentSaved = try await api.exhibition(draft.id)
        guard try value(refreshed) == value(current), currentItem == item,
              currentSaved.draft == draft, currentSaved.etag == etag,
              await api.hasFurnishingThumbnailAuthority(session) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
        try NativeFurnishingThumbnailRules.requireDisplay(currentItem.details.rights)
        guard capture.pngData.count <= 512 * 1024,
              nativeImportSHA256(capture.pngData) == capture.pngSha256 else { throw BridgeFailure("CATALOG_PREVIEW_INVALID") }
        return .object(["workspaceId": .string(current.workspaceId), "nativeRevision": .integer(Int64(current.revision)),
            "editVersion": draft.document["editVersion"], "baseETag": .string(etag), "request": try value(capture.request),
            "png": .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
                "bytes": .integer(Int64(capture.pngData.count)), "sha256": .string(capture.pngSha256),
                "base64": .string(capture.pngData.base64EncodedString())])])
    }

    private func finishedHayes(_ context: Context, api: AuthoringAPI, document: NativeDocumentState,
                               artworkId: String, jobId: String, revision: Int) async throws
        -> (artwork: NativeArtwork, fullAssetId: String, receipt: NativeHayesReceipt) {
        let transfer = try await bridge.uploadState(context)
        try NativeAutomationArtworkRules.checkUploadScope(transfer, document: document)
        guard selectedOEXFormat == nil, api.hayesOriginalRuntimeQualified, !document.dirty,
              transfer.intent == nil, transfer.jobId == nil, [1, 2].contains(revision) else {
            throw BridgeFailure("HAYES_RELEASE_UNCONFIRMED")
        }
        let artwork = try await api.artwork(artworkId), job = try await api.importJob(jobId)
        guard artwork.id == artworkId, artwork.revision == revision, !artwork.archived,
              job.id == jobId, job.state == "approved", let full = job.assetId,
              let reference = artwork.metadata?.displayReference,
              artwork.assets.contains(where: { $0.id == full && $0.sha256 == reference.assetSha256
                  && $0.mime == "model/gltf-binary" && $0.state == "approved" }) else {
            throw BridgeFailure("HAYES_RELEASE_UNCONFIRMED")
        }
        let receipt = try await bridge.finishedHayesRelease(context, jobId: jobId, artworkId: artworkId,
                                                            revision: revision, displaySHA256: reference.assetSha256)
        return (artwork, full, receipt)
    }
    static func exportRootIdentity(_ root: URL, profile: URL) throws -> (dev_t, ino_t) {
        guard root.isFileURL, root.path != profile.path, !root.path.hasPrefix(profile.path + "/"),
              !profile.path.hasPrefix(root.path + "/"), let identity = try? profileIdentity(root),
              let source = try? profileIdentity(profile), source.0 == identity.0 else {
            throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
        }
        return identity
    }

    static func checkExportDirectory(_ descriptor: Int32, path: String, expected: (dev_t, ino_t)) throws {
        var held = stat(), named = stat()
        guard fstat(descriptor, &held) == 0, lstat(path, &named) == 0,
              (held.st_dev, held.st_ino) == expected, (named.st_dev, named.st_ino) == expected,
              held.st_mode & S_IFMT == S_IFDIR, held.st_mode & 0o777 == 0o700, held.st_uid == getuid() else {
            throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
        }
    }

    private func installedExportDestination(_ path: String) throws -> URL {
        let url = URL(fileURLWithPath: path)
        guard path == url.path, url.deletingLastPathComponent().path == root.path else {
            throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
        }
        return url
    }

    private func output(_ path: String) throws -> URL {
        let url = URL(fileURLWithPath: path)
        if installedExportRootIdentity != nil {
            var metadata = stat()
            _ = try installedExportDestination(path)
            guard lstat(url.path, &metadata) != 0, errno == ENOENT else {
                throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
            }
        }
        guard url.path.hasPrefix(root.path + "/"), url.deletingLastPathComponent().resolvingSymlinksInPath() == root,
              !FileManager.default.fileExists(atPath: url.path) else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
        return url
    }
    static func sharedBenchReply(root: URL, path: String) throws -> DocumentValue {
        let url = URL(fileURLWithPath: path)
        guard path == url.path, url.deletingLastPathComponent() == root else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        let parent = Darwin.open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard parent >= 0 else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        defer { Darwin.close(parent) }
        var held = stat(), named = stat()
        guard fstat(parent, &held) == 0, lstat(root.path, &named) == 0,
              held.st_mode & S_IFMT == S_IFDIR, held.st_uid == getuid(), held.st_mode & 0o777 == 0o700,
              (held.st_dev, held.st_ino) == (named.st_dev, named.st_ino) else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        let fd = openat(parent, url.lastPathComponent, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard fd >= 0 else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? file.close() }
        var info = stat(), after = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_nlink == 1,
              info.st_uid == getuid(), info.st_mode & 0o077 == 0, (1...1_100_000).contains(info.st_size),
              let bytes = try file.read(upToCount: 1_100_001), bytes.count == info.st_size,
              fstat(fd, &after) == 0, after.st_size == info.st_size,
              after.st_mtimespec.tv_sec == info.st_mtimespec.tv_sec,
              after.st_mtimespec.tv_nsec == info.st_mtimespec.tv_nsec else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        return try JSONDecoder().decode(DocumentValue.self, from: bytes)
    }
    private func checkSavedEditPolicy(_ state: NativeDocumentState, api: AuthoringAPI, unsupportedCode: String = "NATIVE_BENCH_CAPABILITY_UNAVAILABLE") async throws {
        guard let draft = state.draft, let etag = state.etag else { return }
        let policy = try await api.nativeViewportPolicy(draft.id)
        guard policy.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        guard !policy.boundedReadOnly, policy.boundedEditRequired != true else { throw BridgeFailure(unsupportedCode) }
    }
    /// Ordinary GUI staging is allowed only on a positive ordinary policy;
    /// bounded provenance retains the server's dedicated viewport check.
    static func operatorRequiresViewportValidation(_ policy: NativeViewportPolicy, etag: String) throws -> Bool {
        guard policy.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        if policy.confirmsOrdinaryDraft { return false }
        guard policy.boundedProfile == true, policy.boundedEditRequired == false,
              !policy.boundedReadOnly, policy.editingCapability == nil else { throw BridgeFailure("NATIVE_ARTWORK_CAPABILITY_UNAVAILABLE") }
        return true
    }
    @discardableResult
    private func checkSavedSurfacePolicy(_ state: NativeDocumentState, api: AuthoringAPI) async throws -> NativeViewportPolicy {
        guard !state.dirty, state.pending == nil, let draft = state.draft, state.etag != nil else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
        let policy = try await api.nativeViewportPolicy(draft.id)
        try SurfaceMaterialTextureRequest.requireEditingPolicy(policy, document: state)
        return policy
    }
    private func write(_ object: DocumentValue, to path: String) throws -> DocumentValue {
        let url = try output(path), bytes = try JSONEncoder().encode(object)
        if let expected = installedExportRootIdentity {
            let parent = Darwin.open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            guard parent >= 0 else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
            defer { Darwin.close(parent) }
            try Self.checkExportDirectory(parent, path: root.path, expected: expected)
            let fd = openat(parent, url.lastPathComponent, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
            guard fd >= 0 else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
            let output = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
            try output.write(contentsOf: bytes)
            try output.synchronize()
            try output.close()
        } else {
            try bytes.write(to: url, options: .withoutOverwriting)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        }
        return .object(["path": .string(url.path), "sha256": .string(Self.digest(bytes)), "bytes": .integer(Int64(bytes.count))])
    }
    static func recordCompletedDraft12Import(
        receipt: [String: DocumentValue],
        reopen: () async throws -> DocumentValue,
        record: (DocumentValue) throws -> DocumentValue,
        isolation: isolated (any Actor)? = #isolation
    ) async throws -> DocumentValue {
        let reopened = try await reopen()
        try Task.checkCancellation()
        var completed = receipt
        completed["reopen"] = .object([
            "previousRuntimeJobId": reopened["previousRuntimeJobId"],
            "runtimeJobId": reopened["runtimeJobId"],
            "studioRevision": reopened["studioRevision"]
        ])
        return try record(.object(completed))
    }

    // These existing CMS create endpoints have no idempotency key. Preserve
    // the exact attempt before POST and never automatically repeat an uncertain
    // creation. The consumer inspects catalog-read and this durable receipt.
    private func beginCatalogCreation(_ request: DocumentValue, id: String) throws {
        let path = root.appendingPathComponent(".automation-catalog-" + id + ".json").path
        guard !FileManager.default.fileExists(atPath: path) else { throw BridgeFailure("AUTOMATION_CREATE_UNCERTAIN") }
        guard case .object(let fields) = request else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        _ = try write(.object(fields.filter { $0.key != "capability" }), to: path)
    }

    private var replacementAttemptPath: String {
        root.appendingPathComponent(".automation-artwork-replacement.json").path
    }

    private var repairAttemptPath: String {
        root.appendingPathComponent(".automation-product-image-thumbnail-repair.json").path
    }

    /// Both bounded journals remain independent; matching is against the local
    /// original head, never an operator-provided newer ETag.
    static func replacementRecoveryAttempt(ordinaryPath: String, repairPath: String,
                                           document: NativeDocumentState) throws -> (path: String, attempt: DocumentValue)? {
        guard let draft = document.draft, let etag = document.etag else { return nil }
        var matching: [(path: String, attempt: DocumentValue)] = []
        for path in [ordinaryPath, repairPath] {
            guard let attempt = try readArtworkAttempt(at: path) else { continue }
            let command = try NativeAutomationReplacementCommand(attempt)
            if path == repairPath {
                guard case .productImageThumbnailRepair = command.target else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN") }
            }
            guard let id = attempt["requestId"].string, UUID(uuidString: id)?.uuidString.lowercased() == id else {
                throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN")
            }
            if attempt["operation"] == .string("artwork-replace-reviewed"),
               attempt["workspaceId"] == .string(document.workspaceId),
               attempt["tenantId"] == .string(document.tenantId), attempt["principalId"] == .string(document.principalId),
               attempt["exhibitionId"] == .string(draft.id),
               attempt["expectedRevision"].integer == Int64(document.revision),
               attempt["expectedEditVersion"] == draft.document["editVersion"], command.etag == etag {
                matching.append((path, attempt))
            }
        }
        guard matching.count <= 1 else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN") }
        return matching.first
    }

    static func replacementAttemptRoute(ordinaryPath: String, repairPath: String, attempt: DocumentValue,
                                         document: NativeDocumentState) throws -> String {
        let command = try NativeAutomationReplacementCommand(attempt)
        if let recovery = try replacementRecoveryAttempt(ordinaryPath: ordinaryPath, repairPath: repairPath, document: document),
           recovery.attempt != attempt { throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN") }
        let exact = try [ordinaryPath, repairPath].filter { try readArtworkAttempt(at: $0) == attempt }
        guard exact.count <= 1 else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN") }
        if let existing = exact.first { return existing } // Preserve a legacy repair's original path on replay.
        if case .productImageThumbnailRepair = command.target { return repairPath }
        return ordinaryPath
    }

    private func artworkAttempt(at path: String, errorCode: String = "ARTWORK_REPLACEMENT_UNCERTAIN") throws -> DocumentValue? {
        try Self.readArtworkAttempt(at: path, errorCode: errorCode)
    }

    private static func readArtworkAttempt(at path: String, errorCode: String = "ARTWORK_REPLACEMENT_UNCERTAIN") throws -> DocumentValue? {
        let fd = Darwin.open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard fd >= 0 else {
            if errno == ENOENT { return nil }
            throw BridgeFailure(errorCode)
        }
        let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? file.close() }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_nlink == 1,
              info.st_uid == getuid(), info.st_mode & 0o077 == 0, (1...8192).contains(info.st_size),
              let bytes = try file.read(upToCount: 8193), bytes.count == info.st_size else {
            throw BridgeFailure(errorCode)
        }
        return try JSONDecoder().decode(DocumentValue.self, from: bytes)
    }

    private func saveReplacementAttempt(_ attempt: DocumentValue, at path: String) throws {
        guard try JSONEncoder().encode(attempt).count <= 8192 else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        _ = try write(attempt, to: path)
    }

    private func syncArtworkAttempt(at attemptPath: String, errorCode: String = "ARTWORK_REPLACEMENT_UNCERTAIN") throws {
        try Self.syncArtworkAttemptFiles(at: attemptPath, errorCode: errorCode)
    }

    private static func syncArtworkAttemptFiles(at attemptPath: String, errorCode: String = "ARTWORK_REPLACEMENT_UNCERTAIN") throws {
        for path in [attemptPath, URL(fileURLWithPath: attemptPath).deletingLastPathComponent().path] {
            let fd = Darwin.open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
            guard fd >= 0 else { throw BridgeFailure(errorCode) }
            let synced = Darwin.fsync(fd)
            _ = Darwin.close(fd)
            guard synced == 0 else { throw BridgeFailure(errorCode) }
        }
    }

    private func exportSavedHead(_ request: DocumentValue, id: String, context: Context,
                                 current: NativeDocumentState, api: AuthoringAPI) async throws -> NativeStaticExportReceipt {
        let publicationProfile = "bounded-local-v1"
        guard let owner = existingWorkspaceV5, let manifest = runtimeManifest, owner.manifestSHA256 == manifest,
              AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest), selectedOEXFormat == nil,
              authoringSession?.nativePublicationProfile == publicationProfile,
              authoringSession?.userId == owner.principalId,
              current.workspaceId == owner.workspaceId, current.tenantId == owner.tenantId,
              current.principalId == owner.principalId else { throw BridgeFailure("NATIVE_SAVED_HEAD_EXPORT_UNAVAILABLE") }
        try checkedRevision(request, current)
        let command = try NativeAutomationArtworkRules.command(request, keys: ["etag"])
        guard !current.dirty, current.pending == nil, let draft = current.draft, let etag = current.etag,
              command["etag"] == .string(etag), let path = request["outputPath"].string else {
            throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED")
        }
        let parent = try Self.exportRootIdentity(root, profile: profile)
        let destination = URL(fileURLWithPath: path)
        guard path == destination.path, destination.deletingLastPathComponent().path == root.path else {
            throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
        }
        let reopened = try await api.exhibition(draft.id)
        let policy = try await api.nativeViewportPolicy(draft.id)
        guard reopened.draft == draft, reopened.etag == etag, policy.etag == etag else {
            throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
        }
        guard policy.confirmsOrdinaryDraft else { throw BridgeFailure("NATIVE_SAVED_HEAD_EXPORT_UNAVAILABLE") }

        let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
        guard case .object(let fields) = request else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        let attempt: DocumentValue = .object([
            "version": .integer(1), "request": .object(fields.filter { $0.key != "capability" }),
            "runtimeManifestSHA256": .string(manifest), "exhibitionId": .string(draft.id),
            "draftSHA256": .string(Self.digest(try encoder.encode(draft))),
            "outputDevice": .string(String(parent.0)), "outputInode": .string(String(parent.1))
        ])
        let journal = profile.appendingPathComponent(".automation-saved-head-export-" + id.lowercased() + ".json")
        let changed = "AUTOMATION_SAVED_HEAD_EXPORT_CHANGED"
        if let previous = try artworkAttempt(at: journal.path, errorCode: changed) {
            guard previous == attempt else { throw BridgeFailure(changed) }
        } else {
            _ = try output(path)
            let bytes = try encoder.encode(attempt)
            guard bytes.count <= 8192 else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            try bytes.write(to: journal, options: .withoutOverwriting)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: journal.path)
        }
        try syncArtworkAttempt(at: journal.path, errorCode: changed)
        try Task.checkCancellation()
        let readiness = try await api.publicationReady(exhibitionId: draft.id, etag: etag, publicationProfile: publicationProfile)
        guard readiness.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        guard readiness.status == "READY" else { throw AuthoringFailure("PUBLICATION_NOT_READY") }
        let frozen = try await api.preparePublication(exhibitionId: draft.id, etag: etag, requestId: id, publicationProfile: publicationProfile)
        let matches = try await api.publications(exhibitionId: draft.id).filter { $0.publicationId == frozen.publicationId }
        guard matches == [frozen], frozen.status == "published" else { throw BridgeFailure("PUBLICATION_SCOPE_CHANGED") }
        try reopened.requireSameComparison(await api.exhibition(draft.id))
        guard try Self.exportRootIdentity(root, profile: profile) == parent else { throw BridgeFailure(changed) }

        let head: DocumentValue = .object(["exhibitionId": .string(draft.id), "etag": .string(etag),
            "requestId": .string(id), "revision": request["expectedRevision"],
            "editVersion": request["expectedEditVersion"], "revisionSha256": .string(frozen.revisionSha256)])
        _ = try await bridge.startStaticExport(context, publicationId: frozen.publicationId, output: destination,
                                               privateEditor: true, nativeOriginalDetail: false,
                                               outputParentIdentity: parent, savedHead: head)
        let receipt = try await waitForStaticExport(context, publication: frozen, path: path, nativeOriginal: false, detailJobId: nil)
        guard receipt.publicationId == frozen.publicationId, receipt.revisionSha256 == frozen.revisionSha256,
              receipt.output == path, FileManager.default.fileExists(atPath: path),
              try Self.exportRootIdentity(root, profile: profile) == parent else { throw BridgeFailure(changed) }
        return receipt
    }

    private func executeCompactModel(_ operation: String, request: DocumentValue, id: String,
                                     context: Context, current: NativeDocumentState, api: AuthoringAPI) async throws -> DocumentValue {
        guard selectedOEXFormat == nil, !current.dirty, current.pending == nil,
              request["inputPath"] == .null, request["outputPath"] == .null,
              await api.admitsCompactModelVariant else { throw BridgeFailure("IMPORT_MODEL_UNAVAILABLE") }
        try checkedRevision(request, current)
        let creates = operation == "compact-model-prepare"
        var keys: Set<String> = creates ? ["artworkId", "input"] : ["artworkId", "input", "jobId"]
        if operation == "compact-model-refresh-placement" { keys.insert("placementId") }
        let command = try NativeAutomationArtworkRules.command(request, keys: keys)
        guard let artworkId = command["artworkId"].string, NativeCompactModelVariantRequest.validID(artworkId) else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        let input = try JSONDecoder().decode(NativeCompactModelVariantRequest.self,
            from: JSONEncoder().encode(command["input"]))
        guard input.valid, NativeCompactModelVariantRequest.validID(input.idempotencyKey),
              try value(input) == command["input"], !creates || id == input.idempotencyKey else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        let path = root.appendingPathComponent(".automation-compact-" + input.idempotencyKey + ".json").path
        let jobPath = root.appendingPathComponent(".automation-compact-" + input.idempotencyKey + ".job.json").path
        let errorCode = "COMPACT_VARIANT_CONTEXT_CHANGED"
        let attempt: DocumentValue = .object(["workspaceId": .string(current.workspaceId),
            "tenantId": .string(current.tenantId), "principalId": .string(current.principalId),
            "artworkId": .string(artworkId), "input": command["input"]])
        if let retained = try artworkAttempt(at: path, errorCode: errorCode) {
            guard retained == attempt else { throw BridgeFailure(errorCode) }
        } else {
            guard creates else { throw BridgeFailure(errorCode) }
            _ = try write(attempt, to: path)
        }
        try syncArtworkAttempt(at: path, errorCode: errorCode)
        if creates {
            let job = try await api.createCompactModelVariant(artworkId: artworkId, input: input)
            let binding: DocumentValue = .object(["jobId": .string(job.id)])
            if let retainedJob = try artworkAttempt(at: jobPath, errorCode: errorCode) {
                guard retainedJob == binding else { throw BridgeFailure(errorCode) }
            } else {
                _ = try write(binding, to: jobPath)
            }
            try syncArtworkAttempt(at: jobPath, errorCode: errorCode)
            return job.automationValue
        }
        guard let jobId = command["jobId"].string, NativeCompactModelVariantRequest.validID(jobId),
              try artworkAttempt(at: jobPath, errorCode: errorCode)?["jobId"] == .string(jobId) else {
            throw BridgeFailure(errorCode)
        }
        switch operation {
        case "compact-model-status":
            return try await api.compactModelVariantJob(jobId, artworkId: artworkId, input: input).automationValue
        case "compact-model-retry", "compact-model-cancel":
            let action: NativeImportAction = operation == "compact-model-retry" ? .retry : .cancel
            return try await api.changeCompactModelVariant(action, jobId: jobId, artworkId: artworkId,
                                                           input: input).automationValue
        case "compact-model-refresh-placement":
            guard var draft = current.draft, let etag = current.etag, let placement = command["placementId"].string,
                  NativeCompactModelVariantRequest.validID(placement) else { throw BridgeFailure("PLACEMENT_INVALID") }
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            _ = try await api.verifyApprovedCompactModelVariant(artworkId: artworkId, jobId: jobId, input: input)
            let snapshot = try await api.studioArtwork(artworkId)
            try NativeAutomationArtworkRules.checkRegistration(snapshot.artwork, artworkId: artworkId,
                revision: Int64(input.revision + 1), tenantId: current.tenantId)
            try draft.refreshApprovedCompactArtwork(snapshot.artwork, placementId: placement)
            var next = current
            next.draft = draft
            return try value(await bridge.saveDocumentState(context, state: next))
        case "compact-model-approve":
            return try value(await api.approveCompactModelVariant(artworkId: artworkId, jobId: jobId, input: input))
        default:
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
    }

    private func prepareReviewedArtwork(_ request: DocumentValue, id: String, context: Context,
                                        api: AuthoringAPI, current: NativeDocumentState) async throws -> DocumentValue {
        try checkedRevision(request, current)
        let product = request["command"]["kind"] == .string(NativeProductImagePreparationIntent.profileID)
        let common: Set<String> = ["artworkId", "artworkRevision", "retryFailed"]
        let command = try NativeAutomationArtworkRules.command(request, keys: product
            ? common.union(["kind"]) : common.union(["approvalId", "derivedSha256"]))
        let available = product ? await api.admitsProductImagePreparation : api.admitsSameOwnerArtworkReplacement
        guard available else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNAVAILABLE") }
        guard !current.dirty, let draft = current.draft, let etag = current.etag,
              let path = request["inputPath"].string, let sourceHash = inputs[path],
              let artworkId = command["artworkId"].string,
              let revision = command["artworkRevision"].integer, (1...2_147_483_647).contains(revision),
              case .bool(let retryFailed) = command["retryFailed"] else {
            throw BridgeFailure("AUTOMATION_INPUT_INVALID")
        }
        guard draft.candidate["artworks"].items?.contains(where: { item in
            let cms = item["extensions"]["org.exhibitos.studio/cms"]
            return item["id"].string == artworkId && cms["tenantId"].string == current.tenantId
                && cms["artworkId"].string == artworkId && cms["revision"].integer == revision
                && (!product || item["artworkType"].string == "image")
        }) == true else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
        let saved = try await api.exhibition(draft.id)
        guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        let state = try await bridge.uploadState(context)
        try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
        let url = URL(fileURLWithPath: path)
        guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        let original = try await ImportFileSnapshot.load(url, allowJPEG: true)
        guard original.mime == "image/jpeg", original.sha256 == sourceHash else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
        if product {
            let preparation = NativeProductImagePreparationIntent(artworkRevision: Int(revision))
            let intent: NativeImportIntent
            if let retained = state.intent {
                guard retained.artworkId == artworkId, retained.idempotencyKey == id,
                      retained.productImagePreparation == preparation, original.matches(retained) else {
                    throw BridgeFailure("IMPORT_INTENT_CHANGED")
                }
                intent = retained
            } else {
                let artwork = try await api.artwork(artworkId)
                try preparation.checkSource(original, artwork: artwork)
                guard artwork.id == artworkId, let rights = artwork.metadata?.rights else {
                    throw BridgeFailure("ARTWORK_SCOPE_CHANGED")
                }
                intent = NativeImportIntent(artworkId: artworkId, idempotencyKey: id, mime: original.mime,
                    sha256: original.sha256, bytes: original.data.count, scaleMeters: 1, rights: rights,
                    productImagePreparation: preparation)
            }
            var job = try await NativeAutomationArtworkTransfer.run(bridge: bridge, context: context, state: state,
                intent: intent, file: original, transport: api)
            if retryFailed, job.state == "failed", job.canRetry, job.retryAt != nil, job.confirmsIntakeDrain(for: intent) {
                let jobId = job.id
                job = try await api.importAction(jobId, action: .retry)
                guard job.id == jobId else { throw BridgeFailure("IMPORT_RECEIPT_INVALID") }
            }
            return .object(["job": try productImageJobValue(job, intent: intent, current: current),
                            "transfer": try value(await bridge.uploadState(context))])
        }
        guard let approvalId = command["approvalId"].string,
              let expectedDerived = command["derivedSha256"].string, nativeImportDigest(expectedDerived) else {
            throw BridgeFailure("AUTOMATION_INPUT_INVALID")
        }
        let coarse = NativeCoarsePreparationIntent(approvalId: approvalId, artworkRevision: Int(revision))
        guard coarse.valid else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        let artwork = try await api.artwork(artworkId)
        guard artwork.id == artworkId, Int64(artwork.revision) == revision, !artwork.archived,
              artwork.approvedRevision == artwork.revision, let rights = artwork.metadata?.rights else {
            throw BridgeFailure("ARTWORK_SCOPE_CHANGED")
        }
        let derived = try await NativePreparedJPEG.prepare(
            original: original, operationID: id, generation: id, isCurrent: { !Task.isCancelled }, allowICC: true
        )
        guard derived.derived.sha256 == expectedDerived, derived.derived.recipe.colourConversion != nil else {
            throw BridgeFailure("IMPORT_RECEIPT_INVALID")
        }
        let intent = NativeImportIntent(artworkId: artwork.id, idempotencyKey: id, mime: original.mime,
            sha256: original.sha256, bytes: original.data.count, scaleMeters: 1, rights: rights,
            derived: derived.derived, coarsePreparation: coarse)
        var job = try await NativeAutomationArtworkTransfer.run(bridge: bridge, context: context, state: state,
            intent: intent, file: original, derived: derived, transport: api)
        if retryFailed, job.state == "failed", job.canRetryReviewedImport(intent) {
            let id = job.id
            job = try await api.importAction(id, action: .retry)
            guard job.id == id else { throw BridgeFailure("IMPORT_RECEIPT_INVALID") }
        }
        return .object(["job": try NativeAutomationArtworkRules.reviewedJobValue(job, intent: intent),
                        "transfer": try value(await bridge.uploadState(context))])
    }

    private func productImageJobValue(_ job: NativeImportJob, intent: NativeImportIntent,
                                      current: NativeDocumentState) throws -> DocumentValue {
        guard let preparation = intent.productImagePreparation, let status = job.productImagePreparation,
              status.matches(preparation, job: job),
              status.status != .ready || status.validatesReady(intent: intent, preparation: preparation, job: job,
                  workspaceID: current.workspaceId, principalID: current.principalId) else {
            throw BridgeFailure("IMPORT_RECEIPT_INVALID")
        }
        var wire = NativeAutomationArtworkRules.jobValue(job)
        wire["imagePreparation"] = try value(status)
        return wire
    }

    private func admitsReplacement(_ command: NativeAutomationReplacementCommand, api: AuthoringAPI) async -> Bool {
        switch command.target {
        case .image: api.admitsSameOwnerArtworkReplacement
        case .productImage: await api.admitsProductImagePreparation
        case .approvedModel: await api.admitsApprovedModelCandidateReplacement
        case .productImageThumbnailRepair: await api.admitsProductImageThumbnailRepair
        }
    }

    private func replaceReviewedArtwork(_ request: DocumentValue, command: NativeAutomationReplacementCommand,
                                        id: String, context: Context, api: AuthoringAPI,
                                        current: NativeDocumentState) async throws -> DocumentValue {
        guard !current.dirty, let draft = current.draft, let currentETag = current.etag,
              let oldLocalRevision = request["expectedRevision"].integer,
              (0..<Int64.max).contains(oldLocalRevision), case .object(let fields) = request,
              fields["inputPath"] == nil, fields["outputPath"] == nil else {
            throw BridgeFailure("AUTOMATION_INPUT_INVALID")
        }
        let state = try await bridge.uploadState(context)
        try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
        switch command.target {
        case .image:
            guard let intent = state.intent, intent.artworkId == command.artworkId, state.jobId == command.importJobId,
                  intent.coarsePreparation?.artworkRevision == command.artworkRevision else {
                throw BridgeFailure("IMPORT_INTENT_CHANGED")
            }
        case .productImage:
            guard let intent = state.intent, intent.artworkId == command.artworkId, state.jobId == command.importJobId,
                  intent.productImagePreparation?.artworkRevision == command.artworkRevision else {
                throw BridgeFailure("IMPORT_INTENT_CHANGED")
            }
        case .productImageThumbnailRepair:
            // The retained transfer may belong to a different artwork/job. Never clear or rewrite it.
            guard current.pending == nil else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
        case .approvedModel:
            let empty = state.intent == nil && state.jobId == nil && state.compactVariant == nil
            let same = state.intent?.artworkId == command.artworkId && state.jobId == command.importJobId
                && state.intent?.modelPreparation?.artworkRevision == command.artworkRevision
            guard current.pending == nil, empty || same else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
        }
        var durableFields = fields.filter { $0.key != "capability" }
        durableFields["exhibitionId"] = .string(draft.id)
        let attempt = DocumentValue.object(durableFields)
        let onOriginal = currentETag == command.etag
        if onOriginal { try checkedRevision(request, current) }
        let repair: Bool
        if case .productImageThumbnailRepair = command.target { repair = true } else { repair = false }
        let attemptPath = try Self.replacementAttemptRoute(ordinaryPath: replacementAttemptPath,
            repairPath: repairAttemptPath, attempt: attempt, document: current)
        let completionPath = root.appendingPathComponent(".automation-artwork-repair-completed-" + id + ".json").path
        if repair, let completed = try artworkAttempt(at: completionPath) {
            guard completed["attempt"] == attempt, completed["etag"].string == currentETag,
                  completed["localRevision"].integer == Int64(current.revision) else {
                throw BridgeFailure("AUTOMATION_REQUEST_CHANGED")
            }
            let observed = try await api.exhibition(draft.id)
            guard observed.draft == draft, observed.etag == currentETag,
                  observed.revision == command.studioRevision + 1 else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            return try value(current)
        }
        if repair, attemptPath == repairAttemptPath,
           let pending = try artworkAttempt(at: attemptPath), pending != attempt,
           let previousId = pending["requestId"].string,
           UUID(uuidString: previousId)?.uuidString.lowercased() == previousId {
            let previousPath = root.appendingPathComponent(".automation-artwork-repair-completed-" + previousId + ".json").path
            _ = try Self.releaseCompletedRepairAttempt(at: attemptPath, expected: pending, completedAt: previousPath)
        }
        if let pending = try artworkAttempt(at: attemptPath) {
            guard pending == attempt, onOriginal || current.revision == UInt64(oldLocalRevision + 1) else {
                throw BridgeFailure("AUTOMATION_REQUEST_CHANGED")
            }
        } else {
            guard onOriginal, !artworkReplacementRecoveryOnly else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let previous = try await api.exhibition(draft.id)
            guard previous.draft == draft, previous.etag == command.etag, previous.revision == command.studioRevision else {
                throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
            }
            if case .productImage(let receiptSHA256) = command.target, let intent = state.intent {
                _ = try await api.reviewProductImage(importJobId: command.importJobId, intent: intent,
                    workspaceId: current.workspaceId, principalId: current.principalId, expectedReceiptSHA256: receiptSHA256)
            }
            if case .productImageThumbnailRepair = command.target {
                try await api.reviewProductImageThumbnailRepair(draft: draft, command: command,
                    workspaceId: current.workspaceId, principalId: current.principalId)
            }
            try saveReplacementAttempt(attempt, at: attemptPath)
        }
        try syncArtworkAttempt(at: attemptPath)
        // A lost repair response keeps this session on the exact retained attempt.
        if repair { artworkReplacementRecoveryOnly = true }
        let stored: NativeStudioResponse
        switch command.target {
        case .image(let coarseSHA256):
            guard let intent = state.intent else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            stored = try await api.replaceReviewedArtwork(
                exhibitionId: draft.id, studioRevision: command.studioRevision, etag: command.etag, requestId: id,
                importJobId: command.importJobId, intent: intent, expectedCoarseSHA256: coarseSHA256
            )
        case .productImage(let receiptSHA256):
            guard let intent = state.intent else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            stored = try await api.replaceProductImage(exhibitionId: draft.id, studioRevision: command.studioRevision,
                etag: command.etag, requestId: id, importJobId: command.importJobId, intent: intent,
                workspaceId: current.workspaceId, principalId: current.principalId, expectedReceiptSHA256: receiptSHA256)
        case .productImageThumbnailRepair:
            stored = try await api.repairProductImageThumbnail(draft: draft, command: command,
                requestId: id, onOriginal: onOriginal)
        case .approvedModel(let placementID):
            stored = try await api.replaceApprovedModelCandidate(
                draft: draft, studioRevision: command.studioRevision, etag: command.etag, requestId: id,
                artworkId: command.artworkId, artworkRevision: command.artworkRevision,
                importJobId: command.importJobId, placementId: placementID
            )
        }
        try Task.checkCancellation()
        if !onOriginal {
            guard stored.draft == draft, stored.etag == currentETag else {
                throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
            }
            if repair { try completeRepairAttempt(attempt, state: current, path: completionPath) }
            artworkReplacementRecoveryOnly = false
            return try value(current)
        }
        var next = current
        next.draft = stored.draft
        next.etag = stored.etag
        next.savedEditVersion = stored.draft.document["editVersion"].integer
        let persisted = try await bridge.saveDocumentState(context, state: next)
        artworkReplacementRecoveryOnly = false
        if repair { try completeRepairAttempt(attempt, state: persisted, path: completionPath) }
        return try value(persisted)
    }

    /// A completed repair can release the singleton slot only after its retained
    /// completion receipt is durable. An uncertain or ordinary replacement never rotates.
    static func releaseCompletedRepairAttempt(at path: String, expected: DocumentValue, completedAt completionPath: String) throws -> Bool {
        guard let command = try? NativeAutomationReplacementCommand(expected),
              case .productImageThumbnailRepair = command.target,
              try readArtworkAttempt(at: path) == expected,
              let completed = try readArtworkAttempt(at: completionPath), completed["attempt"] == expected,
              let localRevision = completed["localRevision"].integer, localRevision > 0,
              let etag = completed["etag"].string,
              etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil,
              etag.hasPrefix("\"studio-r\(command.studioRevision + 1)-") else { return false }
        try syncArtworkAttemptFiles(at: completionPath)
        guard Darwin.unlink(path) == 0 else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN") }
        try syncArtworkAttemptFiles(at: completionPath) // Includes parent fsync after unlink.
        return true
    }

    private func completeRepairAttempt(_ attempt: DocumentValue, state: NativeDocumentState, path: String) throws {
        let completed: DocumentValue = .object(["attempt": attempt, "etag": state.etag.map(DocumentValue.string) ?? .null,
            "localRevision": .integer(Int64(state.revision))])
        if let previous = try artworkAttempt(at: path) {
            guard previous == completed else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN") }
        } else {
            guard try JSONEncoder().encode(completed).count <= 8192 else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            _ = try write(completed, to: path)
        }
        try syncArtworkAttempt(at: path)
    }

    private func executeWorkspaceMaterial(_ operation: String, request: DocumentValue, api: AuthoringAPI) async throws -> DocumentValue {
        switch operation {
        case "workspace-material-intake-create":
            var command = try NativeAutomationArtworkRules.command(request, keys: ["kind", "name", "rights", "provenance", "maps"])
            command["requestId"] = request["requestId"]
            let declaration = try JSONDecoder().decode(WorkspaceMaterialDeclaration.self, from: JSONEncoder().encode(command))
            guard try value(declaration) == command else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            return try value(await api.createWorkspaceMaterial(declaration))
        case "workspace-material-intake-status":
            let command = try NativeAutomationArtworkRules.command(request, keys: ["intakeId"])
            guard let id = command["intakeId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            return try value(await api.workspaceMaterialIntake(id))
        case "workspace-material-intake-upload":
            let command = try NativeAutomationArtworkRules.command(request, keys: ["intakeId", "role"])
            guard let id = command["intakeId"].string, let name = command["role"].string,
                  let role = WorkspaceMaterialRole(rawValue: name),
                  let path = request["inputPath"].string, let pin = inputs[path] else {
                throw BridgeFailure("AUTOMATION_INPUT_INVALID")
            }
            let url = URL(fileURLWithPath: path)
            guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let snapshot = try await ImportFileSnapshot.load(url, byteLimit: WorkspaceMaterialMap.byteLimit)
            guard snapshot.sha256 == pin else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
            return try value(await api.uploadWorkspaceMaterial(id, role: role, snapshot: snapshot))
        case "workspace-material-intake-complete", "workspace-material-intake-cancel", "workspace-material-intake-retry":
            let command = try NativeAutomationArtworkRules.command(request, keys: ["intakeId"])
            guard let id = command["intakeId"].string, let name = operation.split(separator: "-").last,
                  let action = WorkspaceMaterialIntakeAction(rawValue: String(name)) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            return try value(await api.workspaceMaterialAction(id, action))
        case "workspace-material-list":
            let command = try NativeAutomationArtworkRules.command(request, keys: ["cursor"])
            guard command["cursor"] == .null || command["cursor"].string != nil else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let page = try await api.workspaceMaterials(cursor: command["cursor"].string)
            return .object(["items": try value(page.items), "nextCursor": page.nextCursor.map(DocumentValue.string) ?? .null])
        case "workspace-material-read", "workspace-material-preview":
            let preview = operation == "workspace-material-preview"
            let command = try NativeAutomationArtworkRules.command(request, keys: preview ? ["id", "version", "role"] : ["id", "version"])
            guard let id = command["id"].string, let version = command["version"].integer,
                  (1...2_147_483_647).contains(version) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let material = try await api.workspaceMaterial(id, version: Int(version))
            guard preview else { return try value(material) }
            guard let name = command["role"].string, let role = WorkspaceMaterialRole(rawValue: name) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let bytes = try await api.workspaceMaterialPreview(material, role: role)
            return .object(["id": .string(material.id), "version": .integer(version), "role": .string(role.rawValue),
                            "bytes": .integer(Int64(bytes.count)), "sha256": .string(Self.digest(bytes))])
        default:
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
    }

    private func executeWorkspaceFurnishing(_ operation: String, request: DocumentValue,
                                            api: AuthoringAPI) async throws -> DocumentValue {
        switch operation {
        case "workspace-furnishing-intake-create":
            let commandKeys: Set<String> = ["kind", "name", "rights", "provenance", "model"]
            guard case .object(var fields) = request["command"] else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let policy: WorkspaceFurnishingPreparationPolicy?
            if let input = fields.removeValue(forKey: "preparationPolicy") {
                guard let name = input.string, let selected = WorkspaceFurnishingPreparationPolicy(rawValue: name) else {
                    throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
                }
                policy = selected
            } else {
                policy = nil
            }
            let fitProfile: WorkspaceFurnishingFitProfile?
            if let input = fields.removeValue(forKey: "fitProfile") {
                guard let name = input.string, let selected = WorkspaceFurnishingFitProfile(rawValue: name) else {
                    throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
                }
                fitProfile = selected
            } else {
                fitProfile = nil
            }
            guard Set(fields.keys) == commandKeys else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let command = DocumentValue.object(fields)
            let details = try JSONDecoder().decode(WorkspaceFurnishingDetails.self, from: JSONEncoder().encode(command))
            guard let id = request["requestId"].string, try value(details) == command else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            return try value(await api.createWorkspaceFurnishing(requestId: id, details: details, preparationPolicy: policy, fitProfile: fitProfile))
        case "workspace-furnishing-intake-list", "workspace-furnishing-list":
            let command = try NativeAutomationArtworkRules.command(request, keys: ["cursor"])
            guard command["cursor"] == .null || command["cursor"].string != nil else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            if operation == "workspace-furnishing-intake-list" {
                let page = try await api.workspaceFurnishingIntakes(cursor: command["cursor"].string)
                return .object(["items": try value(page.items), "nextCursor": page.nextCursor.map(DocumentValue.string) ?? .null])
            }
            let page = try await api.workspaceFurnishings(cursor: command["cursor"].string)
            return .object(["items": try value(page.items), "nextCursor": page.nextCursor.map(DocumentValue.string) ?? .null])
        case "workspace-furnishing-intake-status", "workspace-furnishing-intake-upload",
             "workspace-furnishing-intake-complete", "workspace-furnishing-intake-cancel", "workspace-furnishing-intake-retry":
            let command = request["command"]
            let optional: Set<String> = operation == "workspace-furnishing-intake-status" ? ["producerManifestSHA256"] : []
            guard case .object(let fields) = command, Set(fields.keys).subtracting(optional) == ["intakeId"] else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            guard let id = command["intakeId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let expected = fields["producerManifestSHA256"]
            if let expected {
                guard let hash = expected.string, nativeImportDigest(hash), await api.infoStandPreparationManifest == hash else {
                    throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
                }
            }
            let intake = try await api.workspaceFurnishingIntake(id, requireCurrentInfoStandProducer: expected != nil)
            if operation == "workspace-furnishing-intake-status" { return try value(intake) }
            if operation == "workspace-furnishing-intake-upload" {
                guard let path = request["inputPath"].string, let pin = inputs[path] else {
                    throw BridgeFailure("AUTOMATION_INPUT_INVALID")
                }
                let url = URL(fileURLWithPath: path)
                guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
                let snapshot = try await ImportFileSnapshot.load(url, byteLimit: WorkspaceFurnishingModel.byteLimit)
                guard snapshot.sha256 == pin else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
                return try value(await api.uploadWorkspaceFurnishing(intake, snapshot: snapshot))
            }
            guard let name = operation.split(separator: "-").last,
                  let action = WorkspaceFurnishingIntakeAction(rawValue: String(name)) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            return try value(await api.workspaceFurnishingAction(intake, action))
        case "workspace-furnishing-read", "workspace-furnishing-model-check":
            let command = try NativeAutomationArtworkRules.command(request, keys: ["id", "version"])
            guard let id = command["id"].string, command["version"] == .integer(1) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let furnishing = try await api.workspaceFurnishing(id, version: 1)
            if operation == "workspace-furnishing-read" { return try value(furnishing) }
            let bytes = try await api.workspaceFurnishingModel(furnishing)
            return .object(["id": .string(id), "version": .integer(1), "bytes": .integer(Int64(bytes.count)),
                            "sha256": .string(Self.digest(bytes))])
        default:
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
    }

    static func checkInstalledExportRequest(_ operation: String, fields: Set<String>) throws {
        let common: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId"]
        let expected: Set<String>
        switch operation {
        case "read": expected = common
        case "export-oex", "export-static", "export-static-status": expected = common.union(["expectedRevision", "expectedEditVersion", "outputPath"])
        default: throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED")
        }
        guard fields == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
    }

    static func checkPreservationOperation(_ operation: String?, manifest: String?, allowed: String) throws {
        if let manifest, AuthoringAPI.supportsPreReadyPreservationReadback(manifest: manifest), operation != allowed {
            throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED")
        }
    }

    static func checkInstalledPendingOperation(_ operation: String, hasPending: Bool, authoring: Bool) throws {
        guard !hasPending || operation == "read" || (authoring && operation == "save") else {
            throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED")
        }
    }

    static func installedAuthoringEntryAllowed(canCreate: Bool, session: AuthoringSession?) -> Bool {
        guard let session else { return false }
        return canCreate || session.nativeAuthoringProposalProfile == "saved-head-v1"
            || session.advertisesRenderedArtworkThumbnail
    }

    static func installedExportEntryAllowed(session: AuthoringSession?) -> Bool {
        guard let session else { return false }
        return session.nativeOexExportProfile == "bounded-file-v1"
            || session.nativePublicationProfile == "bounded-local-v1"
    }

    static func expectedReopenPorts(_ configuration: NativeConfiguration, manifest: String, tenantId: String,
                                    requested: [String: UInt16], installed: Bool) throws -> NativePorts {
        guard configuration.manifestSha256 == manifest, configuration.tenantId == tenantId,
              !installed || configuration.kind == "personal" else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        let ports = configuration.ports
        if !installed {
            guard ports.web == requested["web"], ports.api == requested["api"], ports.database == requested["database"] else {
                throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
            }
        }
        return ports
    }

    static func checkReopenedConfiguration(_ configuration: NativeConfiguration, previous: NativeConfiguration,
                                           ports: NativePorts) throws {
        guard configuration.manifestSha256 == previous.manifestSha256,
              configuration.tenantId == previous.tenantId, configuration.kind == previous.kind,
              configuration.ports.web == ports.web, configuration.ports.api == ports.api,
              configuration.ports.database == ports.database else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
    }

    static func emptyExhibitionDraft(_ request: DocumentValue, in current: NativeDocumentState) throws -> NativeExhibitionDraft {
        guard current.draft == nil, current.etag == nil, current.pending == nil, current.savedEditVersion == nil else {
            throw BridgeFailure("EXHIBITION_CREATE_UNAVAILABLE")
        }
        let command = try NativeAutomationArtworkRules.command(request, keys: ["title"])
        guard let title = command["title"].string,
              !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, title.count <= 200 else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        return try NativeExhibitionDraft.new(title: title)
    }

    static func checkPendingSaveRequest(_ request: DocumentValue, _ current: NativeDocumentState,
                                        knownRevision: UInt64?, ordinary: Bool) throws {
        guard let draft = current.draft, let pending = current.pending,
              pending.requestId == request["requestId"].string, pending.draft == draft,
              pending.etag == current.etag,
              request["expectedEditVersion"] == draft.document["editVersion"] else {
            throw BridgeFailure("EXHIBITION_PENDING")
        }
        if ordinary {
            guard pending.origin == nil, pending.modelCandidate == nil, pending.productImage == nil else {
                throw BridgeFailure("EXHIBITION_PENDING")
            }
            if knownRevision != current.revision { try checkDocumentRevision(request, current) }
        } else {
            guard knownRevision == current.revision else { throw BridgeFailure("EXHIBITION_PENDING") }
        }
    }

    public func execute(_ request: DocumentValue) async throws -> DocumentValue {
        guard request["version"] == .integer(1), let id = request["requestId"].string, UUID(uuidString: id) != nil,
              let operation = request["operation"].string, case .object(let fields) = request,
              Set(fields.keys).isSubset(of: ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion", "inputPath", "outputPath", "command", "proposalId", "archiveFormat"]) else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        try Self.checkPreservationOperation(operation, manifest: runtimeManifest, allowed: "read")
        if installedAuthoringCLI {
            let allowed: Set<String> = ["read", "exhibition-new", "save", "reopen-saved", "plaque-prepare", "plaque-apply", "plaque-cancel", "pedestal-prepare", "pedestal-apply", "pedestal-cancel", "light-prepare", "light-apply", "light-cancel", "fixture-prepare", "fixture-apply", "fixture-cancel", "light-attachment-prepare", "light-attachment-apply", "light-attachment-cancel", "thumbnail-prepare", "thumbnail-status", "thumbnail-select", "required-model-original-discover", "required-model-original-status", "required-model-original-approve"]
            guard allowed.contains(operation) else { throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED") }
        } else if installedExportRootIdentity != nil {
            try Self.checkInstalledExportRequest(operation, fields: Set(fields.keys))
        }
        if ["required-model-original-discover", "required-model-original-status", "required-model-original-approve"].contains(operation) {
            let expected: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion", "command"]
            guard Set(fields.keys) == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if ["thumbnail-prepare", "thumbnail-status", "thumbnail-select"].contains(operation) {
            let expected: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion", "command"]
            guard Set(fields.keys) == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if operation == "artwork-model-preparation-discard" {
            let expected: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion", "command"]
            guard Set(fields.keys) == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if operation == "exhibition-new" {
            let expected: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion", "command"]
            guard Set(fields.keys) == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if ["plaque-prepare", "plaque-apply", "plaque-cancel", "wall-finish-prepare", "wall-finish-apply", "wall-finish-cancel", "pedestal-prepare", "pedestal-apply", "pedestal-cancel", "light-prepare", "light-apply", "light-cancel", "fixture-prepare", "fixture-apply", "fixture-cancel", "light-attachment-prepare", "light-attachment-apply", "light-attachment-cancel"].contains(operation) {
            let common: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion"]
            let extra: Set<String> = operation.hasSuffix("-prepare") ? ["command"] : ["proposalId"]
            guard Set(fields.keys) == common.union(extra) else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if operation == "shared-bench-prepare" || operation == "shared-bench-apply" {
            let common: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion"]
            let extra: Set<String> = operation == "shared-bench-prepare" ? ["command", "outputPath"] : ["proposalId", "inputPath"]
            guard Set(fields.keys) == common.union(extra) else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if ["workspace-bench-seat-attach", "workspace-default-seat-reconcile"].contains(operation) {
            let expected: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId",
                                         "expectedRevision", "expectedEditVersion", "command"]
            guard Set(fields.keys) == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if ["furnishing-walking-collision-evaluate", "furnishing-walking-collision-set"].contains(operation) {
            let common: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId",
                                       "expectedRevision", "expectedEditVersion"]
            let expected = operation.hasSuffix("-set") ? common.union(["command"]) : common
            guard Set(fields.keys) == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if operation == "export-static-saved-head" {
            let expected: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId",
                                         "expectedRevision", "expectedEditVersion", "outputPath", "command"]
            guard Set(fields.keys) == expected, id == UUID(uuidString: id)?.uuidString.lowercased() else {
                throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID")
            }
        }
        if ["artwork-placement-move", "artwork-interpretation-set"].contains(operation) {
            let expected: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion", "command"]
            guard Set(fields.keys) == expected else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        if installedImageCLI {
            guard ["read", "import-image", "artwork-status", "artwork-image-preparation-retry", "artwork-approve",
                   "artwork-register", "save", "artwork-finish-reviewed"].contains(operation) else {
                throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED")
            }
            let common: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId"]
            let revision: Set<String> = ["expectedRevision", "expectedEditVersion"]
            let extra: Set<String>
            switch operation {
            case "read": extra = []
            case "artwork-status": extra = ["command"]
            case "import-image": extra = revision.union(["inputPath", "command"])
            case "save": extra = revision
            default: extra = revision.union(["command"])
            }
            guard Set(fields.keys) == common.union(extra) else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        }
        let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
        let bytes = try encoder.encode(request)
        // Scope is checked even for replay: correlation is not authorization.
        let (context, api, current) = try await checked(request)
        if installedExportRootIdentity != nil {
            try Self.checkInstalledPendingOperation(operation, hasPending: current.pending != nil, authoring: installedAuthoringCLI)
        }
        if let seen = seenRequests[id], seen != bytes { throw BridgeFailure("AUTOMATION_REQUEST_CHANGED") }
        let prior = try Self.replayResponse(for: operation, prior: replay[id], matching: bytes)
        if let expected = installedExportRootIdentity,
           !(operation == "export-oex" && prior?["commitState"] == .string("committed-path-unconfirmed") && prior?["path"] == .null) {
            guard try Self.exportRootIdentity(root, profile: profile) == expected else {
                throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
            }
        }
        if let prior {
            if ["pedestal-prepare", "light-prepare", "fixture-prepare", "light-attachment-prepare"].contains(operation), sceneProposals[id] == nil { throw BridgeFailure("AUTOMATION_PROPOSAL_EXPIRED") }
            if operation == "plaque-prepare", plaqueProposals[id] == nil { throw BridgeFailure("AUTOMATION_PROPOSAL_EXPIRED") }
            if operation == "wall-finish-prepare", wallFinishProposals[id] == nil { throw BridgeFailure("AUTOMATION_PROPOSAL_EXPIRED") }
            return prior
        }
        guard !busy, seenRequests.count < 128 else { throw BridgeFailure("AUTOMATION_BUSY") }
        guard !runtimeStopped || operation == "read" else { throw BridgeFailure("NATIVE_NOT_READY") }
        guard !artworkReplacementRecoveryOnly || ["read", "artwork-replace-reviewed"].contains(operation) else {
            throw BridgeFailure("ARTWORK_REPLACEMENT_UNCERTAIN")
        }
        seenRequests[id] = bytes
        busy = true; defer { busy = false }
        let result: DocumentValue
        switch operation {
        case "exhibition-new":
            try checkedRevision(request, current)
            guard installedAuthoringCLI, await api.admitsPersonalExhibitionCreation else { throw BridgeFailure("EXHIBITION_CREATE_UNAVAILABLE") }
            var next = current; next.draft = try Self.emptyExhibitionDraft(request, in: current)
            try Task.checkCancellation()
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "required-model-original-discover", "required-model-original-status", "required-model-original-approve":
            try checkedRevision(request, current)
            let discovery = operation == "required-model-original-discover"
            let keys: Set<String> = discovery ? ["artworkId", "artworkRevision"] : ["artworkId", "artworkRevision", "importJobId", "serverReceiptSha256"]
            let command = try NativeAutomationArtworkRules.command(request, keys: keys)
            guard let artworkId = command["artworkId"].string, let revision = command["artworkRevision"].integer,
                  (1...Int64(Int32.max)).contains(revision) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let custody = try await api.discoverRequiredModelOriginal(artworkId: artworkId, revision: Int(revision), in: current)
            if discovery {
                result = try value(custody)
            } else {
                guard command["importJobId"] == .string(custody.importJobId), command["serverReceiptSha256"] == .string(custody.serverReceiptSha256) else {
                    throw BridgeFailure("REQUIRED_MODEL_ORIGINAL_UNAVAILABLE")
                }
                result = try value(await api.requiredModelOriginalApproval(custody, in: current, approve: operation == "required-model-original-approve"))
            }
            let (_, _, refreshed) = try await checked(request)
            guard try value(refreshed) == value(current) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
        case "pedestal-prepare", "light-prepare", "fixture-prepare", "light-attachment-prepare":
            try checkedRevision(request, current)
            guard !current.dirty, current.pending == nil, let draft = current.draft, let etag = current.etag else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            try await checkSavedEditPolicy(current, api: api, unsupportedCode: "NATIVE_VIEWPORT_CAPABILITY_UNAVAILABLE")
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: current.tenantId,
                principalId: current.principalId, draft: draft, selection: nil, mode: "translate", view: "isometric",
                disabled: false, workspaceId: current.workspaceId, baseETag: etag)
            let preview: SceneProposal
            let candidate: DocumentValue
            if operation == "fixture-prepare" || operation == "light-attachment-prepare" {
                guard await api.admitsNativeFixtureAuthoringProposal else { throw AuthoringFailure("NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE") }
                if operation == "fixture-prepare" {
                    let proposal = try NativeAutomationFixture.fixtureRequest(request["command"], frame: frame)
                    let reply = try await api.nativeAuthoringProposal(exhibitionId: draft.id, studioRevision: saved.revision, etag: etag, proposal: proposal.value)
                    preview = .fixture(try NativeAutomationFixture.fixtureReply(reply, request: proposal)); candidate = reply["candidate"]
                } else {
                    let proposal = try NativeAutomationFixture.AttachmentRequest(request["command"], frame: frame)
                    let reply = try await api.nativeAuthoringProposal(exhibitionId: draft.id, studioRevision: saved.revision, etag: etag, proposal: proposal.value)
                    let checked = try proposal.checkedReply(reply)
                    preview = .scene(checked); candidate = checked.candidate
                }
            } else if operation == "light-prepare", request["command"]["type"] == .string("ensure-room-base-light") {
                guard authoringSession?.advertisesRoomBaseLight == true,
                      authoringSession?.nativeSceneAuthoringProposalProfile == "saved-head-scene-v1" else {
                    throw AuthoringFailure("NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE")
                }
                switch try NativeAutomationScene.roomBaseRequest(request["command"], frame: frame) {
                case .existing(let checked):
                    preview = .scene(checked); candidate = checked.candidate
                case .addition(let proposal):
                    let reply = try await api.nativeAuthoringProposal(exhibitionId: draft.id, studioRevision: saved.revision, etag: etag, proposal: proposal.value)
                    let checked = try proposal.checkedReply(reply)
                    preview = .scene(checked); candidate = checked.candidate
                }
            } else {
                let wire: DocumentValue
                let check: @Sendable (DocumentValue) throws -> SceneProposal
                if operation == "pedestal-prepare" {
                    let proposal = try NativeAutomationScene.pedestalRequest(request["command"], frame: frame)
                    wire = proposal.value; check = { .scene(try proposal.checkedReply($0)) }
                } else if request["command"]["light"] != .null {
                    let proposal = try NativeAutomationScene.additionRequest(request["command"], frame: frame)
                    wire = proposal.value; check = { .scene(try proposal.checkedReply($0)) }
                } else {
                    let proposal = try NativeAutomationScene.lightRequest(request["command"], frame: frame)
                    wire = proposal.value; check = { .light(try proposal.checkedReply($0, selectedLightId: proposal.command.lightId)) }
                }
                let reply = try await api.nativeAuthoringProposal(exhibitionId: draft.id, studioRevision: saved.revision, etag: etag, proposal: wire)
                preview = try check(reply); candidate = reply["candidate"]
            }
            try Task.checkCancellation()
            let now = try await api.exhibition(draft.id)
            let (_, _, refreshed) = try await checked(request)
            guard now.draft == draft, now.etag == etag, now.revision == saved.revision,
                  try value(refreshed) == value(current) else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            sceneProposals[id] = (preview, etag, operation)
            result = .object(["proposalId": .string(id), "candidate": candidate])
        case "pedestal-apply", "light-apply", "pedestal-cancel", "light-cancel", "fixture-apply", "fixture-cancel", "light-attachment-apply", "light-attachment-cancel":
            try checkedRevision(request, current)
            guard let proposalId = request["proposalId"].string, let (preview, etag, prepared) = sceneProposals[proposalId],
                  prepared == operation.replacingOccurrences(of: "-apply", with: "-prepare").replacingOccurrences(of: "-cancel", with: "-prepare") else { throw BridgeFailure("AUTOMATION_PROPOSAL_EXPIRED") }
            if operation.hasSuffix("-cancel") {
                sceneProposals.removeValue(forKey: proposalId)
                result = .object(["cancelled": .bool(true)])
            } else {
                guard !current.dirty, current.pending == nil, let draft = current.draft, current.etag == etag else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
                try await checkSavedEditPolicy(current, api: api, unsupportedCode: "NATIVE_VIEWPORT_CAPABILITY_UNAVAILABLE")
                let saved = try await api.exhibition(draft.id)
                let (_, _, refreshed) = try await checked(request)
                guard saved.draft == draft, saved.etag == etag, try value(refreshed) == value(current) else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
                var next = current; next.draft = try preview.apply(to: draft)
                try Task.checkCancellation()
                if next.draft == draft { result = try value(current) }
                else { result = try value(await bridge.saveDocumentState(context, state: next)) }
                sceneProposals.removeValue(forKey: proposalId)
            }
        case "thumbnail-prepare", "thumbnail-status", "thumbnail-select":
            result = try await thumbnailCandidate(request, id: id, context: context, api: api, current: current)
        case "read": result = try await statusValue()
        case "artwork-finish-reviewed":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "jobId", "artworkRevision"])
            var transfer = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(transfer, document: current)
            let requiredPreparationAvailable = await api.admitsRequiredModelPreparation
            let productPreparationAvailable = await api.admitsProductImagePreparation
            let freshPreparationAvailable = await api.admitsFreshImagePreparation
            guard selectedOEXFormat == nil, !current.dirty,
                  let draft = current.draft, let etag = current.etag,
                  let artworkId = command["artworkId"].string, let jobId = command["jobId"].string,
                  let revision = command["artworkRevision"].integer,
                  let intent = transfer.intent, intent.artworkId == artworkId, transfer.jobId == jobId,
                  (intent.coarsePreparation != nil && api.admitsSameOwnerArtworkReplacement)
                    || (intent.modelPreparation != nil && requiredPreparationAvailable)
                    || (intent.productImagePreparation.map { $0.isFresh ? freshPreparationAvailable : productPreparationAvailable } == true) else {
                throw BridgeFailure("IMPORT_INTENT_CHANGED")
            }
            guard !installedImageCLI || intent.productImagePreparation?.isFresh == true else {
                throw BridgeFailure("IMPORT_INTENT_CHANGED")
            }
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let job = try await api.importJob(jobId), artwork = try await api.artwork(artworkId)
            guard job.id == jobId, job.state == "approved", job.confirmsIntakeDrain(for: intent),
                  let assetId = job.assetId, !artwork.archived, Int64(artwork.revision) == revision,
                  artwork.approvedRevision == artwork.revision, artwork.approvedAssetId == assetId,
                  artwork.metadata?.rights == intent.rights, draft.artworks.contains(where: {
                      $0["id"].string == artworkId && $0["revision"].integer == revision && $0["primaryAssetId"].string == assetId
                  }) else { throw BridgeFailure("APPROVAL_REVIEW_REQUIRED") }
            let jobProof: DocumentValue
            if let preparation = intent.productImagePreparation {
                guard revision == Int64(preparation.artworkRevision + (preparation.isFresh ? 0 : 1)),
                      job.productImagePreparation?.status == .ready else { throw BridgeFailure("APPROVAL_REVIEW_REQUIRED") }
                jobProof = try productImageJobValue(job, intent: intent, current: current)
            } else if intent.modelPreparation != nil {
                guard job.modelPreparation?.permitsReview(of: artwork, request: intent, job: job) == true else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
                jobProof = NativeAutomationArtworkRules.jobValue(job)
            } else {
                jobProof = try NativeAutomationArtworkRules.reviewedJobValue(job, intent: intent)
            }
            transfer.intent = nil; transfer.jobId = nil; transfer.fileName = nil
            let cleared = try await bridge.saveUploadState(context, state: transfer)
            result = .object(["job": jobProof, "transfer": try value(cleared)])
        case "workspace-frame-lineage-status", "workspace-frame-lineage-prepare":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["intakeId", "requestId", "furnishingId"])
            guard selectedOEXFormat == nil, !current.dirty, let intakeId = command["intakeId"].string else {
                throw BridgeFailure("AUTOMATION_INPUT_INVALID")
            }
            let history = try await api.workspaceFurnishingIntake(intakeId)
            guard history.requestId == command["requestId"].string,
                  let furnishing = history.furnishing, furnishing.id == command["furnishingId"].string else {
                throw BridgeFailure("AUTOMATION_INPUT_CHANGED")
            }
            var original: ImportFileSnapshot?
            if operation == "workspace-frame-lineage-prepare" {
                guard let path = request["inputPath"].string, let expected = inputs[path] else {
                    throw BridgeFailure("AUTOMATION_INPUT_INVALID")
                }
                let url = URL(fileURLWithPath: path)
                guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
                let snapshot = try await ImportFileSnapshot.load(url, byteLimit: WorkspaceRegisteredFrameLineage.originalByteLimit)
                guard snapshot.sha256 == expected else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
                original = snapshot
            }
            result = try value(await api.registeredFrameLineage(history, workspaceId: current.workspaceId, original: original))
        case "artwork-prepare-reviewed":
            guard selectedOEXFormat == nil,
                  UUID(uuidString: id)?.uuidString.lowercased() == id else {
                throw BridgeFailure("ARTWORK_REPLACEMENT_UNAVAILABLE")
            }
            result = try await prepareReviewedArtwork(request, id: id, context: context, api: api, current: current)
        case "artwork-replace-reviewed":
            guard selectedOEXFormat == nil, UUID(uuidString: id)?.uuidString.lowercased() == id else {
                throw BridgeFailure("ARTWORK_REPLACEMENT_UNAVAILABLE")
            }
            let command = try NativeAutomationReplacementCommand(request)
            guard await admitsReplacement(command, api: api) else { throw BridgeFailure("ARTWORK_REPLACEMENT_UNAVAILABLE") }
            result = try await replaceReviewedArtwork(request, command: command, id: id, context: context, api: api, current: current)
        case "workspace-material-intake-create", "workspace-material-intake-status",
             "workspace-material-intake-upload", "workspace-material-intake-complete",
             "workspace-material-intake-cancel", "workspace-material-intake-retry",
             "workspace-material-preview":
            guard selectedOEXFormat == nil, await api.admitsWorkspaceMaterials else {
                throw BridgeFailure("WORKSPACE_MATERIAL_UNAVAILABLE")
            }
            result = try await executeWorkspaceMaterial(operation, request: request, api: api)
        case "workspace-material-list", "workspace-material-read":
            guard selectedOEXFormat == nil, await api.admitsWorkspaceMaterialCatalog else {
                throw BridgeFailure("WORKSPACE_MATERIAL_UNAVAILABLE")
            }
            result = try await executeWorkspaceMaterial(operation, request: request, api: api)
        case "workspace-furnishing-intake-create", "workspace-furnishing-intake-list", "workspace-furnishing-intake-status",
             "workspace-furnishing-intake-upload", "workspace-furnishing-intake-complete", "workspace-furnishing-intake-cancel",
             "workspace-furnishing-intake-retry", "workspace-furnishing-list", "workspace-furnishing-read", "workspace-furnishing-model-check":
            guard selectedOEXFormat == nil, await api.admitsWorkspaceFurnishings else {
                throw BridgeFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            result = try await executeWorkspaceFurnishing(operation, request: request, api: api)
        case "workspace-furnishing-thumbnail-render":
            result = try await furnishingThumbnail(request, context: context, api: api, current: current)
        case "workspace-furnishing-place":
            guard selectedOEXFormat == nil, await api.admitsWorkspaceFurnishings else {
                throw BridgeFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            try checkedRevision(request, current)
            guard case .object(var fields) = request["command"] else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let expectedModelSHA256: String?
            if let approved = fields.removeValue(forKey: "modelSha256") {
                guard let hash = approved.string, nativeImportDigest(hash) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                expectedModelSHA256 = hash
            } else {
                expectedModelSHA256 = nil
            }
            let infoStandIntakeId: String?
            if let intake = fields.removeValue(forKey: "intakeId") {
                guard let id = intake.string, workspaceFurnishingID(id) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                infoStandIntakeId = id
            } else {
                infoStandIntakeId = nil
            }
            guard Set(fields.keys) == ["id", "roomId", "furnishingId", "furnishingVersion", "position", "yaw", "scale"] else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let command = DocumentValue.object(fields)
            let placement = try JSONDecoder().decode(WorkspaceFurnishingPlacement.self, from: JSONEncoder().encode(command))
            guard try value(placement) == command else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            if let draft = try await api.prepareWorkspaceFurnishingPlacement(placement, in: current,
                expectedModelSHA256: expectedModelSHA256, infoStandIntakeId: infoStandIntakeId) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "workspace-info-stand-replace":
            guard selectedOEXFormat == nil, await api.admitsWorkspaceInfoStandReplacement else {
                throw BridgeFailure("WORKSPACE_INFO_STAND_REPLACEMENT_UNAVAILABLE")
            }
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["etag", "modelSha256", "intakeId", "placement", "plaque"])
            guard let etag = current.etag, command["etag"].string == etag,
                  let hash = command["modelSha256"].string, let intakeId = command["intakeId"].string else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let replacement = try WorkspaceInfoStandReplacement(value: .object(["placement": command["placement"], "plaque": command["plaque"]]))
            if let draft = try await api.prepareWorkspaceInfoStandReplacement(replacement, in: current,
                expectedModelSHA256: hash, intakeId: intakeId) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "workspace-bench-seat-attach":
            guard selectedOEXFormat == nil else { throw BridgeFailure("WORKSPACE_BENCH_SEAT_UNAVAILABLE") }
            try checkedRevision(request, current)
            let required: Set<String> = ["etag", "benchId", "placementId", "furnishingId", "furnishingVersion", "modelSha256",
                                         "dimensions", "viewpointId"]
            let command = request["command"]
            guard case .object(let commandFields) = command, Set(commandFields.keys).subtracting(["viewpoint"]) == required,
                  let etag = current.etag, command["etag"].string == etag,
                  let benchId = command["benchId"].string, let placementId = command["placementId"].string,
                  let furnishingId = command["furnishingId"].string, command["furnishingVersion"].integer == 1,
                  let hash = command["modelSha256"].string, let viewpointId = command["viewpointId"].string,
                  case .object(let dimensions) = command["dimensions"], Set(dimensions.keys) == ["width", "depth", "height"],
                  let width = dimensions["width"]?.double, let depth = dimensions["depth"]?.double,
                  let height = dimensions["height"]?.double else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let viewpoint = try commandFields["viewpoint"].map(BenchViewpoint.init(value:))
            let attachment = try WorkspaceBenchSeatAttachment(benchId: benchId, placementId: placementId,
                furnishingId: furnishingId, furnishingVersion: 1, modelSha256: hash,
                width: width, depth: depth, height: height, viewpointId: viewpointId, viewpoint: viewpoint)
            if let draft = try await api.prepareWorkspaceBenchSeat(attachment, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "workspace-default-seat-reconcile":
            guard selectedOEXFormat == nil else { throw BridgeFailure("WORKSPACE_BENCH_SEAT_UNAVAILABLE") }
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["etag", "placementIds"])
            guard let etag = current.etag, command["etag"].string == etag, let ids = command["placementIds"].items else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let placementIds = ids.compactMap(\.string)
            guard placementIds.count == ids.count else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            if let draft = try await api.prepareWorkspaceDefaultSeatRetrofit(placementIds: placementIds, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "furnishing-walking-collision-evaluate":
            guard selectedOEXFormat == nil else { throw BridgeFailure("FURNISHING_WALKING_COLLISION_UNAVAILABLE") }
            try checkedRevision(request, current)
            result = try await api.evaluateFurnishingWalkingCollision(in: current)
        case "furnishing-walking-collision-set":
            guard selectedOEXFormat == nil else { throw BridgeFailure("FURNISHING_WALKING_COLLISION_UNAVAILABLE") }
            try checkedRevision(request, current)
            let (target, setting) = try FurnishingWalkingCollisionProposal.command(request["command"])
            if let draft = try await api.prepareFurnishingWalkingCollision(target: target, setting: setting, in: current) {
                try Task.checkCancellation()
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                let latest = try await bridge.documentState(context)
                try checkedRevision(request, latest)
                result = .object(["changed": .bool(false), "document": try value(latest)])
            }
        case "workspace-frame-attach", "workspace-frame-detach":
            guard selectedOEXFormat == nil, await api.admitsWorkspaceArtworkFrames else {
                throw BridgeFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            try checkedRevision(request, current)
            let command: WorkspaceArtworkFrameCommand
            if operation == "workspace-frame-attach" {
                guard let input = try? NativeAutomationArtworkRules.command(request, keys: [
                    "placementId", "artworkRevisionId", "assetId", "furnishingId", "furnishingVersion", "modelSha256"
                ]), let attachment = try? JSONDecoder().decode(WorkspaceArtworkFrameAttachment.self, from: JSONEncoder().encode(input)),
                      try value(attachment) == input else { throw BridgeFailure("VIEWPORT_COMMAND_DENIED") }
                command = .attach(attachment)
            } else {
                guard let input = try? NativeAutomationArtworkRules.command(request, keys: ["placementId"]),
                      let placementId = input["placementId"].string else { throw BridgeFailure("VIEWPORT_COMMAND_DENIED") }
                command = .detach(placementId: placementId)
            }
            if let draft = try await api.prepareWorkspaceArtworkFrame(command, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "workspace-portal-attach", "workspace-portal-detach":
            guard selectedOEXFormat == nil, await api.admitsWorkspacePortalFinish else {
                throw BridgeFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            try checkedRevision(request, current)
            let attaching = operation == "workspace-portal-attach"
            let fields = try NativeAutomationArtworkRules.command(request, keys: attaching
                ? ["openingId", "furnishingId", "furnishingVersion", "modelSha256"] : ["openingId"])
            let modularFit = attaching ? await api.admitsWorkspaceModularPortalFit : false
            let rectangularFit = attaching ? await api.admitsWorkspaceRectangularPortalFit : false
            let modelHash = fields["modelSha256"].string
            let validReusableHash = (modularFit || rectangularFit) && modelHash.map {
                $0.utf8.count == 64 && $0.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
            } == true
            guard let opening = fields["openingId"].string,
                  !attaching || (fields["furnishingId"].string != nil && fields["furnishingVersion"] == .integer(1)
                    && (modelHash == WorkspacePortalFinish.modelSHA256 || validReusableHash)) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            if let draft = try await api.prepareWorkspacePortalFinish(openingId: opening,
                furnishingId: fields["furnishingId"].string, expectedModelSHA256: fields["modelSha256"].string, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "workspace-material-floor-apply", "workspace-material-wall-apply":
            guard selectedOEXFormat == nil else { throw BridgeFailure("WORKSPACE_MATERIAL_UNAVAILABLE") }
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["surfaceId", "materialId", "materialVersion", "repeatMeters", "rotation"])
            guard let surfaceId = command["surfaceId"].string, let materialId = command["materialId"].string,
                  let version = command["materialVersion"].integer, let dimensions = command["repeatMeters"].items,
                  dimensions.count == 2, let width = dimensions[0].double, let length = dimensions[1].double,
                  let rotation = command["rotation"].double else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let surfaceKind: WorkspaceMaterialKind = operation == "workspace-material-wall-apply" ? .wall : .floor
            let binding = try WorkspaceSurfaceMaterialBinding(surfaceKind: surfaceKind, surfaceId: surfaceId, materialId: materialId,
                                                              materialVersion: Int(version), repeatMeters: [width, length], rotation: rotation)
            if let draft = try await api.prepareWorkspaceSurfaceMaterial(binding, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "load-copy":
            try checkedRevision(request, current)
            guard selectedOEXFormat == nil, current.draft == nil, current.pending == nil, let path = request["inputPath"].string, let hash = inputs[path] else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let url = URL(fileURLWithPath: path)
            guard url.resolvingSymlinksInPath() == url, let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size <= 1_048_576 else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let data = try Data(contentsOf: url); guard Self.digest(data) == hash else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
            let source = try JSONDecoder().decode(NativeExhibitionDraft.self, from: data)
            guard let sourceDraftID = source.document["id"].string, let sourceRevisionID = source.candidate["revisionId"].string,
                  Set([sourceDraftID, source.id, sourceRevisionID]).count == 3 else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            var next = current; next.draft = try source.copyForEditing()
            let saved = try await bridge.saveDocumentState(context, state: next)
            result = .object(["document": try value(saved), "idMap": .object([
                sourceDraftID: saved.draft!.document["id"], source.id: .string(saved.draft!.id),
                sourceRevisionID: saved.draft!.candidate["revisionId"]]), "nestedIdsPreserved": .bool(true)])
        case "import-oex9", "import-oex10", "import-oex12":
            try checkedRevision(request, current)
            let archiveFormat: String
            switch operation {
            case "import-oex12": archiveFormat = "1.0.0-draft.12"
            case "import-oex10": archiveFormat = "1.0.0-draft.10"
            default: archiveFormat = "1.0.0-draft.9"
            }
            guard selectedOEXFormat == archiveFormat, current.draft == nil, current.pending == nil,
                  request["archiveFormat"].string == archiveFormat, let path = request["inputPath"].string,
                  let expectedHash = inputs[path], let outputPath = request["outputPath"].string else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            _ = try output(outputPath)
            let status = try await bridge.nativeStatus(context)
            guard status.selectedOex == true, status.oex027Qualified == true,
                  archiveFormat != "1.0.0-draft.10" || status.oex028Qualified == true,
                  archiveFormat != "1.0.0-draft.12" || (workspaceV7ValidationRuntime == nil ? status.oexWorkspaceV6Qualified : status.oexWorkspaceV7Qualified) == true else {
                throw BridgeFailure("NATIVE_OEX_RELEASE_UNAVAILABLE")
            }
            let source = URL(fileURLWithPath: path)
            guard source.resolvingSymlinksInPath() == source else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let file = try await NativeOEXImportFile.loadForBoundedProfile(source)
            guard file.archive.sha256 == expectedHash else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
            if archiveFormat != "1.0.0-draft.9" {
                guard try await bridge.selectedOEXFormat(file.archive) == archiveFormat else { throw BridgeFailure("OEX_FORMAT_INVALID") }
            }
            let original = try await bridge.oexImportState(context)
            guard original.workspaceId == current.workspaceId, original.tenantId == current.tenantId,
                  original.principalId == current.principalId else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
            // This adapter exposes no installation-selection operation. The
            // immutable context stays owned until task drain; each journal and
            // owner-channel action independently checks its original token.
            let transfer = try await NativeOEXImportTransfer.runBoundedOnce(state: original, file: file, api: api,
                archiveFormat: archiveFormat, requestId: id,
                persist: { [bridge] state in try await bridge.saveOEXImportState(context, state: state) },
                select: { [bridge] in try await bridge.selectBoundedOEX(context) },
                isCurrent: { original.workspaceId == current.workspaceId && original.tenantId == current.tenantId && original.principalId == current.principalId })
            _ = try await bridge.runBoundedOEX(context)
            let stored = try await NativeSelectedOEXCompletion.finish(bridge: bridge, context: context, api: api, original: original, jobId: transfer.1.id)
            let receipt = try await api.oexImportReceipt(transfer.1.id)
            let storedValue = try value(stored)
            let receiptFields: [String: DocumentValue] = [
                "archiveSHA256": .string(expectedHash), "archiveFormat": .string(archiveFormat),
                "jobId": .string(transfer.1.id), "requestId": .string(id),
                "document": storedValue, "importReceipt": receipt
            ]
            let recorded: DocumentValue
            if archiveFormat == "1.0.0-draft.12" {
                selectedOEXFormat = nil
                recorded = try await Self.recordCompletedDraft12Import(receipt: receiptFields, reopen: {
                    try await reopenSaved(stored, context: context, api: api)
                }, record: { try write($0, to: outputPath) })
            } else {
                recorded = try write(.object(receiptFields), to: outputPath)
                _ = try await bridge.stop(context); _ = try await waitReady()
                _ = try await bridge.start(context); _ = try await waitReady()
                authoringSession = try await api.connect(bridge.authoringSession(context))
                selectedOEXFormat = nil
            }
            result = .object(["document": storedValue, "receipt": recorded])
        case "room-audio-apply":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["roomId", "etag", "settings"])
            guard selectedOEXFormat == nil, fields["inputPath"] == nil, fields["outputPath"] == nil,
                  let etag = current.etag, command["etag"] == .string(etag), let roomId = command["roomId"].string,
                  case .object(let settings) = command["settings"] else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            if let draft = try await api.prepareRoomAudio(RoomAudioSettings(fields: settings), roomId: roomId, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "placed-exit-door-apply":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["etag", "settings"])
            guard selectedOEXFormat == nil, fields["inputPath"] == nil, fields["outputPath"] == nil,
                  let etag = current.etag, command["etag"] == .string(etag) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let settings = try PlacedExitDoorSettings(value: command["settings"])
            if let draft = try await api.preparePlacedExitDoor(settings, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
            }
        case "floor-appearance-apply":
            try checkedRevision(request, current)
            let policy = try await checkSavedSurfacePolicy(current, api: api)
            let command = request["command"]
            guard selectedOEXFormat == nil, var draft = current.draft,
                  case .object(let fields) = command,
                  let room = command["roomId"].string, let surface = command["surfaceId"].string else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            if Set(fields.keys) == ["roomId", "surfaceId", "preset"],
               let name = command["preset"].string, let preset = FloorSettings.Preset(rawValue: name) {
                try draft.applyFloorAppearance(preset, roomId: room, surfaceId: surface)
                _ = try await api.floorPreview(preset)
            } else {
                guard Set(fields.keys) == ["roomId", "surfaceId", "catalog"],
                      command["catalog"] == NativePrivateFloorCatalog.reference else {
                    throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
                }
                let status = try await bridge.nativeStatus(context)
                guard let manifest = status.configuration?.manifestSha256, manifest == runtimeManifest,
                      NativePrivateFloorCatalog.supports(manifestSHA256: manifest, ready: status.runtime?.readiness.ready == true) else {
                    throw BridgeFailure("MATERIAL_CATALOG_UNAVAILABLE")
                }
                let catalog = try await api.privateFloorCatalog()
                _ = try await api.privateFloorPreview(catalog)
                try draft.applyFloorAppearance(.wood, roomId: room, surfaceId: surface,
                                               defaults: NativePrivateFloorCatalog.defaults)
            }
            var next = current
            next.draft = draft
            let saved = try await bridge.saveDocumentState(context, state: next)
            result = .object(["document": try value(saved),
                "policy": .object(["exhibitionId": .string(policy.exhibitionId), "revision": .integer(Int64(policy.revision)),
                    "etag": .string(policy.etag), "boundedProfile": .bool(policy.boundedProfile == true),
                    "boundedEditRequired": .bool(false), "boundedReadOnly": .bool(false)])])
        case "surface-texture-prepare":
            try checkedRevision(request, current)
            try await checkSavedSurfacePolicy(current, api: api)
            guard let draft = current.draft, let surfaceId = request["command"]["surfaceId"].string,
                  let path = request["outputPath"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let frame = ViewportFrame(epoch: capability, sequence: 1, tenantId: current.tenantId, principalId: current.principalId,
                draft: draft, selection: ViewportSelection(kind: "surface", id: surfaceId), mode: "translate", view: "isometric",
                disabled: false, workspaceId: current.workspaceId, baseETag: current.etag)
            let proposal = try SurfaceMaterialTextureRequest(frame: frame, input: request["command"])
            let catalog = try await api.surfaceMaterials()
            guard catalog.items.contains(where: { $0.id == proposal.command.texture?.catalogId && $0.version == proposal.command.texture?.catalogVersion }) else { throw BridgeFailure("MATERIAL_CATALOG_INVALID") }
            _ = try output(path)
            result = try write(.object(["version": .integer(1), "operation": .string("surface-texture-proposal"),
                "frame": try value(frame), "request": proposal.value]), to: path)
            surfaceProposals[id] = proposal
        case "surface-texture-apply":
            try checkedRevision(request, current)
            try await checkSavedSurfacePolicy(current, api: api)
            guard current.pending == nil, let draft = current.draft, let proposalId = request["proposalId"].string,
                  let proposal = surfaceProposals[proposalId], proposal.frame.draft == draft,
                  let path = request["inputPath"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let url = URL(fileURLWithPath: path)
            guard url.path.hasPrefix(root.path + "/"), url.resolvingSymlinksInPath() == url,
                  let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size <= 1_100_000 else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let reply = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: url))
            var next = current
            next.draft = try proposal.applyReply(reply, to: current)
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "catalog-read":
            let artists = try await api.artists(), artworks = try await api.artworks()
            guard artists.nextCursor == nil, artworks.nextCursor == nil else { throw BridgeFailure("AUTOMATION_CATALOG_PAGE_LIMIT") }
            result = .object(["artists": try value(artists.items), "artworks": try value(artworks.items),
                              "nextCursor": artworks.nextCursor.map(DocumentValue.string) ?? .null])
        case "artist-create":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["name", "bio"])
            guard let name = command["name"].string, let bio = command["bio"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            try beginCatalogCreation(request, id: id)
            let created = try value(await api.createArtist(name: name, bio: bio))
            _ = try write(created, to: root.appendingPathComponent(".automation-catalog-" + id + ".result.json").path)
            result = created
        case "artwork-create":
            try checkedRevision(request, current)
            let provisional = request["command"]["provisionalHeightMeters"] != .null
            var keys: Set<String> = ["artistId", "metadata"]
            if provisional { keys.insert("provisionalHeightMeters") }
            let command = try NativeAutomationArtworkRules.command(request, keys: keys)
            guard let artist = command["artistId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            var metadata = try JSONDecoder().decode(ArtworkMetadata.self, from: JSONEncoder().encode(command["metadata"]))
            guard try value(metadata) == command["metadata"] else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            if provisional {
                guard selectedOEXFormat == nil, current.pending == nil, !current.dirty,
                      request["outputPath"] == .null, metadata.displayReference == nil,
                      metadata.provenance.sourceUnits == "unknown", !metadata.provenance.scaleApplied,
                      let height = command["provisionalHeightMeters"].double,
                      let path = request["inputPath"].string, let hash = inputs[path] else {
                    throw BridgeFailure("AUTOMATION_INPUT_INVALID")
                }
                let url = URL(fileURLWithPath: path)
                guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
                let file = try await ImportFileSnapshot.load(url)
                guard file.sha256 == hash else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
                let inspection = try await api.inspectModelOriginal(file)
                metadata = try inspection.applying(to: metadata, height: height)
            }
            try beginCatalogCreation(request, id: id)
            let created = try value(await api.saveArtwork(id: nil, revision: nil, artistId: artist, metadata: metadata))
            _ = try write(created, to: root.appendingPathComponent(".automation-catalog-" + id + ".result.json").path)
            result = created
        case "hayes-original-queue":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "artworkRevision", "displayReferenceSha256"])
            guard selectedOEXFormat == nil, api.hayesOriginalRuntimeQualified, current.pending == nil, !current.dirty,
                  let draft = current.draft, let etag = current.etag,
                  let path = request["inputPath"].string, inputs[path] == NativeHayesOriginalIntent.sourceSHA256,
                  let artworkId = command["artworkId"].string, let revision = command["artworkRevision"].integer,
                  let reference = command["displayReferenceSha256"].string else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let artwork = try await api.artwork(artworkId)
            guard artwork.id == artworkId, Int64(artwork.revision) == revision, !artwork.archived,
                  let metadata = artwork.metadata, metadata.provenance.sourceUnits == "unknown",
                  metadata.displayReference?.status == "provisional-uncalibrated",
                  metadata.displayReference?.assetSha256 == reference else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
            let url = URL(fileURLWithPath: path)
            guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let original = try await ImportFileSnapshot.load(url, byteLimit: NativeHayesOriginalIntent.sourceBytes)
            let hayes = NativeHayesOriginalIntent(artworkRevision: artwork.revision, displayReferenceSha256: reference)
            let intent = NativeImportIntent(artworkId: artworkId, idempotencyKey: id, mime: original.mime,
                sha256: original.sha256, bytes: original.data.count, scaleMeters: 1, rights: metadata.rights, hayesOriginal: hayes)
            guard hayes.matches(intent) else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            let job = try await NativeAutomationArtworkTransfer.run(bridge: bridge, context: context, state: state,
                intent: intent, file: original, transport: api)
            result = .object(["job": NativeAutomationArtworkRules.jobValue(job), "transfer": try value(await bridge.uploadState(context))])
        case "hayes-original-prepare":
            try checkedRevision(request, current)
            var keys: Set<String> = ["jobId", "acknowledgeRights"]
            if request["command"]["workerRuntime"] != .null { keys.insert("workerRuntime") }
            let command = try NativeAutomationArtworkRules.command(request, keys: keys)
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            guard selectedOEXFormat == nil, api.hayesOriginalRuntimeQualified, current.pending == nil, !current.dirty,
                  let draft = current.draft, let etag = current.etag, command["acknowledgeRights"] == .bool(true),
                  let jobId = state.jobId, command["jobId"].string == jobId,
                  let intent = state.intent, let hayes = intent.hayesOriginal else {
                throw BridgeFailure("AUTOMATION_INPUT_INVALID")
            }
            var worker: URL?
            if keys.contains("workerRuntime") {
                guard let path = command["workerRuntime"].string, path.hasPrefix("/") else {
                    throw BridgeFailure("AUTOMATION_INPUT_INVALID")
                }
                let selected = URL(fileURLWithPath: path, isDirectory: true)
                guard selected.resolvingSymlinksInPath() == selected else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
                worker = selected
            }
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let queued = try await api.importJob(jobId)
            guard queued.id == jobId, queued.state == "queued" else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            let receipt = try await bridge.executeHayesAndWait(context, jobId: jobId, artworkId: intent.artworkId,
                artworkRevision: hayes.artworkRevision, workerRuntime: worker)
            let artwork = try await api.artwork(intent.artworkId), job = try await api.importJob(jobId)
            guard job.id == jobId, hayes.confirmsDisplay(artwork, job: job, intent: intent) else {
                throw BridgeFailure("IMPORT_RECEIPT_INVALID")
            }
            result = .object(["job": NativeAutomationArtworkRules.jobValue(job), "artwork": try value(artwork),
                              "receipt": try value(receipt)])
        case "hayes-original-release":
            try checkedRevision(request, current)
            var keys: Set<String> = ["jobId"]
            if request["command"]["currentArtworkRevision"] != .null { keys.insert("currentArtworkRevision") }
            let command = try NativeAutomationArtworkRules.command(request, keys: keys)
            let revision = command["currentArtworkRevision"].integer.flatMap(Int.init(exactly:))
            guard api.hayesOriginalRuntimeQualified, let jobId = command["jobId"].string else {
                throw BridgeFailure("AUTOMATION_INPUT_INVALID")
            }
            if keys.contains("currentArtworkRevision"), revision == nil { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            result = try value(await bridge.hayesRelease(context, jobId: jobId, currentArtworkRevision: revision))
        case "hayes-finish-intake":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "jobId"])
            var transfer = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(transfer, document: current)
            guard selectedOEXFormat == nil, api.hayesOriginalRuntimeQualified, !current.dirty,
                  let artworkId = command["artworkId"].string, let jobId = command["jobId"].string else {
                throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
            }
            if transfer.intent == nil, transfer.jobId == nil {
                let finished = try await finishedHayes(context, api: api, document: current,
                                                       artworkId: artworkId, jobId: jobId, revision: 1)
                guard finished.artwork.approvedRevision == 1, finished.artwork.approvedAssetId == finished.fullAssetId else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
                result = .object(["transfer": try value(transfer), "receipt": try value(finished.receipt)])
                break
            }
            guard
                  let intent = transfer.intent, intent.artworkId == artworkId, transfer.jobId == jobId,
                  let hayes = intent.hayesOriginal, hayes.artworkRevision == 1 else {
                throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
            }
            let artwork = try await api.artwork(artworkId), job = try await api.importJob(jobId)
            guard artwork.approvedRevision == 1, artwork.approvedAssetId == job.assetId,
                  job.id == jobId, hayes.confirmsDisplay(artwork, job: job, intent: intent) else {
                throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
            }
            let receipt = try await bridge.hayesRelease(context, jobId: jobId)
            transfer.intent = nil; transfer.jobId = nil; transfer.fileName = nil
            let cleared = try await bridge.saveUploadState(context, state: transfer)
            guard cleared.intent == nil, cleared.jobId == nil, cleared.fileName == nil else {
                throw BridgeFailure("IMPORT_CONTEXT_CHANGED")
            }
            result = .object(["transfer": try value(cleared), "receipt": try value(receipt)])
        case "hayes-finished-release", "hayes-approve-finished":
            try checkedRevision(request, current)
            let approve = operation == "hayes-approve-finished"
            let keys: Set<String> = approve
                ? ["artworkId", "jobId", "assetId", "acknowledgeRights"]
                : ["artworkId", "jobId", "currentArtworkRevision"]
            let command = try NativeAutomationArtworkRules.command(request, keys: keys)
            guard let artworkId = command["artworkId"].string, let jobId = command["jobId"].string,
                  let revision = approve ? 2 : command["currentArtworkRevision"].integer.flatMap(Int.init(exactly:)) else {
                throw BridgeFailure("AUTOMATION_INPUT_INVALID")
            }
            let finished = try await finishedHayes(context, api: api, document: current,
                                                   artworkId: artworkId, jobId: jobId, revision: revision)
            if approve {
                guard command["acknowledgeRights"] == .bool(true), let assetId = command["assetId"].string,
                      finished.fullAssetId == assetId else { throw BridgeFailure("APPROVAL_REVIEW_REQUIRED") }
                let approved = try await api.approveArtwork(artworkId, revision: revision, assetId: assetId)
                guard approved.approvedRevision == revision, approved.approvedAssetId == assetId,
                      approved.metadata == finished.artwork.metadata else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
                result = try value(approved)
            } else {
                result = try value(finished.receipt)
            }
        case "compact-model-prepare", "compact-model-status", "compact-model-retry", "compact-model-cancel", "compact-model-approve", "compact-model-refresh-placement":
            result = try await executeCompactModel(operation, request: request, id: id, context: context, current: current, api: api)
        case "import-glb", "import-image":
            let image = operation == "import-image"
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request,
                keys: image ? ["artworkId", "artworkRevision"] : ["artworkId", "artworkRevision", "scaleMeters"])
            guard selectedOEXFormat == nil, current.pending == nil, let path = request["inputPath"].string,
                  let artworkId = command["artworkId"].string, let revision = command["artworkRevision"].integer,
                  let scale = image ? 1.0 : command["scaleMeters"].double else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let expected = inputs[path]
            guard installedImageCLI ? image && path.hasPrefix("/") : expected != nil else {
                throw BridgeFailure("AUTOMATION_INPUT_INVALID")
            }
            let url = URL(fileURLWithPath: path)
            guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let allowLargeModel = image ? false : await api.admitsLargeModelPreparation
            let file = try await ImportFileSnapshot.load(url, allowJPEG: image, allowLargeModel: allowLargeModel)
            guard (image ? ["image/jpeg", "image/png"].contains(file.mime) : file.mime == "model/gltf-binary"),
                  installedImageCLI || file.sha256 == expected else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
            let artwork = try await api.artwork(artworkId)
            guard Int64(artwork.revision) == revision, !artwork.archived, let metadata = artwork.metadata else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            guard state.compactVariant == nil else { throw BridgeFailure("IMPORT_ACTIVE") }
            if image {
                guard await api.admitsFreshImagePreparation else { throw BridgeFailure("IMAGE_PREPARATION_UNAVAILABLE") }
                let preparation = NativeProductImagePreparationIntent(artworkRevision: artwork.revision, fresh: true)
                try preparation.checkFreshArtwork(artwork, rights: metadata.rights)
                let intent = NativeImportIntent(artworkId: artwork.id, idempotencyKey: id, mime: file.mime,
                    sha256: file.sha256, bytes: file.data.count, scaleMeters: 1, rights: metadata.rights,
                    productImagePreparation: preparation)
                let job = try await NativeAutomationArtworkTransfer.run(bridge: bridge, context: context, state: state,
                    intent: intent, file: file, transport: api)
                result = .object(["job": try productImageJobValue(job, intent: intent, current: current),
                                  "transfer": try value(await bridge.uploadState(context))])
                break
            }
            try NativeAutomationArtworkRules.checkDisplayAsset(metadata, sha256: file.sha256, scaleMeters: scale)
            if metadata.provenance.sourceUnits == "unknown" {
                guard await api.admitsModelOriginalInspection,
                      state.intent == nil || state.intent?.modelPreparation != nil else {
                    throw BridgeFailure("IMPORT_MODEL_UNAVAILABLE")
                }
            }
            let preparation: NativeModelPreparationIntent?
            if let retained = state.intent {
                preparation = retained.modelPreparation
            } else if await api.admitsRequiredModelPreparation {
                try NativeModelPreparationIntent.requireFreshIntake(for: artwork)
                preparation = .init(operationID: id, generation: UUID().uuidString.lowercased(), artworkRevision: artwork.revision,
                                    large: file.data.count > ImportFileSnapshot.limit)
            } else { preparation = nil }
            let intent = NativeImportIntent(artworkId: artwork.id, idempotencyKey: id, mime: file.mime,
                sha256: file.sha256, bytes: file.data.count, scaleMeters: scale, rights: metadata.rights,
                modelPreparation: preparation)
            let job = try await NativeAutomationArtworkTransfer.run(bridge: bridge, context: context, state: state,
                intent: intent, file: file, transport: api)
            result = .object(["job": NativeAutomationArtworkRules.jobValue(job), "transfer": try value(await bridge.uploadState(context))])
        case "artwork-status":
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId"])
            guard let artworkId = command["artworkId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let artwork = try await api.artwork(artworkId), state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            var fields: [String: DocumentValue] = ["artwork": try value(artwork), "transfer": try value(state)]
            if let intent = state.intent, intent.artworkId == artwork.id, let job = state.jobId {
                let observed = try await api.importJob(job)
                guard observed.id == job else { throw BridgeFailure("IMPORT_RECEIPT_INVALID") }
                if intent.productImagePreparation != nil {
                    fields["job"] = try productImageJobValue(observed, intent: intent, current: current)
                    result = .object(fields)
                    break
                }
                if intent.modelPreparation != nil {
                    guard observed.id == job, observed.modelPreparation?.matches(intent, job: observed) == true else {
                        throw BridgeFailure("IMPORT_RECEIPT_INVALID")
                    }
                } else { try NativeAutomationArtworkRules.checkJob(observed, id: job) }
                fields["job"] = NativeAutomationArtworkRules.jobValue(observed)
            }
            result = .object(fields)
        case "artwork-image-preparation-retry":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "jobId"])
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            guard selectedOEXFormat == nil, await api.admitsFreshImagePreparation,
                  let intent = state.intent, let preparation = intent.productImagePreparation, preparation.isFresh,
                  command["artworkId"] == .string(intent.artworkId), let jobId = state.jobId,
                  command["jobId"] == .string(jobId) else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            let artwork = try await api.artwork(intent.artworkId)
            try preparation.checkFreshArtwork(artwork, rights: intent.rights)
            let previous = try await api.importJob(jobId)
            guard previous.id == jobId, previous.canRetry, previous.confirmsIntakeDrain(for: intent) else {
                throw BridgeFailure("IMPORT_DRAIN_PENDING")
            }
            let retried = try await api.importAction(jobId, action: .retry)
            guard retried.id == jobId else { throw BridgeFailure("IMPORT_RECEIPT_INVALID") }
            result = .object(["job": try productImageJobValue(retried, intent: intent, current: current), "transfer": try value(state)])
        case "artwork-model-preparation-discard":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "jobId"])
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            guard !installedImageCLI, selectedOEXFormat == nil, await api.admitsRequiredModelPreparation,
                  let intent = state.intent, intent.modelPreparation != nil,
                  command["artworkId"] == .string(intent.artworkId), let jobId = state.jobId,
                  command["jobId"] == .string(jobId) else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            let previous = try await api.importJob(jobId)
            guard previous.id == jobId, ["failed", "cancelled"].contains(previous.state),
                  previous.modelPreparation?.matches(intent, job: previous) == true,
                  previous.confirmsIntakeDrain(for: intent) else { throw BridgeFailure("IMPORT_DRAIN_PENDING") }
            // Reuse the ordinary server cancellation and GUI clearing policy.
            // A failed source cannot be replaced by retrying its immutable job.
            if previous.state == "failed" { _ = try await api.importAction(jobId, action: .cancel) }
            let cancelled = try await api.importJob(jobId)
            guard cancelled.id == jobId, cancelled.state == "cancelled",
                  cancelled.confirmsIntakeDrain(for: intent),
                  cancelled.modelPreparation?.permitsClearingTransfer(artwork: nil, request: intent, job: cancelled) == true else {
                throw BridgeFailure("IMPORT_DRAIN_PENDING")
            }
            let (_, _, refreshed) = try await checked(request)
            guard try value(refreshed) == value(current),
                  try value(await bridge.uploadState(context)) == value(state) else {
                throw BridgeFailure("IMPORT_CONTEXT_CHANGED")
            }
            try Task.checkCancellation()
            var cleared = state; cleared.intent = nil; cleared.jobId = nil; cleared.fileName = nil
            let persisted = try await bridge.saveUploadState(context, state: cleared)
            result = .object(["job": NativeAutomationArtworkRules.jobValue(cancelled), "transfer": try value(persisted)])
        case "artwork-model-preparation-retry":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "jobId"])
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            guard await api.admitsRequiredModelPreparation, let intent = state.intent, let preparation = intent.modelPreparation,
                  command["artworkId"] == .string(intent.artworkId), let jobId = state.jobId,
                  command["jobId"] == .string(jobId) else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            let artwork = try await api.artwork(intent.artworkId)
            let previous = try await api.importJob(jobId)
            guard !artwork.archived, artwork.revision == preparation.artworkRevision, artwork.metadata?.rights == intent.rights,
                  previous.id == jobId, previous.canRetry, previous.confirmsIntakeDrain(for: intent) else {
                throw BridgeFailure("IMPORT_DRAIN_PENDING")
            }
            let retried = try await api.importAction(jobId, action: .retry)
            guard retried.id == jobId, retried.modelPreparation?.matches(intent, job: retried) == true else {
                throw BridgeFailure("IMPORT_RECEIPT_INVALID")
            }
            result = .object(["job": NativeAutomationArtworkRules.jobValue(retried), "transfer": try value(state)])
        case "artwork-icc-validation-status":
            guard selectedOEXFormat == nil, api.admitsICCJPEG,
                  NativeJPEGImportRuntime.supportsPortalICCValidation(manifestSHA256: runtimeManifest, ready: !runtimeStopped) else {
                throw BridgeFailure("IMPORT_ICC_UNAVAILABLE")
            }
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "artworkRevision", "jobId", "originalSha256"])
            let transfer = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(transfer, document: current)
            guard let intent = transfer.intent, intent.artworkId == command["artworkId"].string,
                  intent.sha256 == command["originalSha256"].string,
                  let revision = command["artworkRevision"].integer, revision > 0,
                  let jobId = transfer.jobId, jobId == command["jobId"].string else {
                throw BridgeFailure("IMPORT_INTENT_CHANGED")
            }
            let artwork = try await api.artwork(intent.artworkId)
            let job = try await api.importJob(jobId)
            let preparation = try NativeAutomationArtworkRules.checkICCReadback(
                artwork: artwork, job: job, jobId: jobId, intent: intent, revision: revision)
            var wire = NativeAutomationArtworkRules.jobValue(job)
            wire["imagePreparation"] = .object(["provenance": .string(preparation.provenance),
                "originalSha256": .string(preparation.originalSha256), "derivedSha256": .string(preparation.derivedSha256),
                "recipe": try value(preparation.recipe)])
            result = .object(["artwork": try value(artwork), "job": wire])
        case "artwork-approve":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "revision", "assetId", "acknowledgeRights"])
            guard command["acknowledgeRights"] == .bool(true), let artworkId = command["artworkId"].string,
                  let revision = command["revision"].integer, let assetId = command["assetId"].string else { throw BridgeFailure("APPROVAL_REVIEW_REQUIRED") }
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            guard let intent = state.intent, intent.artworkId == artworkId,
                  intent.mime == "model/gltf-binary" || intent.productImagePreparation?.isFresh == true,
                  !installedImageCLI || intent.productImagePreparation?.isFresh == true,
                  intent.derived == nil, intent.modelDerivation == nil, let jobId = state.jobId else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            let job = try await api.importJob(jobId), artwork = try await api.artwork(artworkId)
            if let preparation = intent.productImagePreparation, preparation.isFresh {
                guard job.id == jobId, job.assetId == assetId, Int64(preparation.artworkRevision) == revision else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
                result = try value(await api.approveFreshImage(job, request: intent, current: artwork, workspaceID: current.workspaceId))
                break
            }
            if let hayes = intent.hayesOriginal {
                guard job.id == jobId, job.assetId == assetId, Int64(artwork.revision) == revision,
                      hayes.matchesReleasedDisplay(artwork, job: job, intent: intent) else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
                _ = try await bridge.hayesRelease(context, jobId: jobId, currentArtworkRevision: artwork.revision)
            } else if let preparation = intent.modelPreparation {
                guard job.id == jobId, job.assetId == assetId, Int64(preparation.artworkRevision) == revision else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
                result = try value(await api.approvePreparedModel(job, request: intent, current: artwork))
                break
            } else {
                try NativeAutomationArtworkRules.checkApproval(artwork: artwork, job: job, jobId: jobId,
                    intent: intent, revision: revision, assetId: assetId)
            }
            _ = try await api.approveArtwork(artworkId, revision: Int(revision), assetId: assetId)
            let approved = try await api.artwork(artworkId)
            guard approved.approvedRevision.map(Int64.init) == revision, approved.approvedAssetId == assetId else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
            if artwork.metadata?.displayReference != nil {
                guard approved.metadata == artwork.metadata else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
            }
            result = try value(approved)
        case "artwork-register":
            try checkedRevision(request, current)
            try await checkSavedEditPolicy(current, api: api, unsupportedCode: "NATIVE_ARTWORK_CAPABILITY_UNAVAILABLE")
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "revision", "roomId", "placementId", "transform"])
            guard current.pending == nil, var draft = current.draft, let artworkId = command["artworkId"].string,
                  let revision = command["revision"].integer, let room = command["roomId"].string,
                  let placement = command["placementId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let approved = try await api.studioArtwork(artworkId)
            if installedImageCLI {
                let transfer = try await bridge.uploadState(context)
                try NativeAutomationArtworkRules.checkUploadScope(transfer, document: current)
                guard let intent = transfer.intent, intent.artworkId == artworkId,
                      let preparation = intent.productImagePreparation, preparation.isFresh, let jobId = transfer.jobId else {
                    throw BridgeFailure("IMPORT_INTENT_CHANGED")
                }
                let job = try await api.importJob(jobId)
                guard job.id == jobId, let status = job.productImagePreparation, let full = status.full,
                      status.validatesReady(intent: intent, preparation: preparation, job: job,
                                            workspaceID: current.workspaceId, principalID: current.principalId),
                      approved.artwork["primaryAssetId"] == .string(full.assetId) else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
            }
            try NativeAutomationArtworkRules.checkRegistration(approved.artwork, artworkId: artworkId, revision: revision,
                tenantId: current.tenantId, allowsImage: await api.admitsFreshImagePreparation)
            try draft.addApprovedArtwork(approved.artwork, roomId: room, placementId: placement, transform: command["transform"])
            var next = current; next.draft = draft
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "hayes-refresh-placed-artwork":
            try checkedRevision(request, current)
            let finished = request["command"]["historicalJobId"].string
            var keys: Set<String> = ["artworkId", "placementId"]
            if finished != nil { keys.insert("historicalJobId") }
            let command = try NativeAutomationArtworkRules.command(request, keys: keys)
            let transfer = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(transfer, document: current)
            guard selectedOEXFormat == nil, api.hayesOriginalRuntimeQualified, !current.dirty,
                  var draft = current.draft, let etag = current.etag,
                  let artworkId = command["artworkId"].string, let placementId = command["placementId"].string else {
                throw BridgeFailure("AUTOMATION_INPUT_INVALID")
            }
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            if let finished {
                let historical = try await finishedHayes(context, api: api, document: current,
                                                         artworkId: artworkId, jobId: finished, revision: 2)
                guard historical.artwork.approvedRevision == 2, historical.artwork.approvedAssetId == historical.fullAssetId else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
            } else {
                guard let intent = transfer.intent, intent.artworkId == artworkId,
                      let hayes = intent.hayesOriginal, let jobId = transfer.jobId else {
                    throw BridgeFailure("AUTOMATION_INPUT_INVALID")
                }
                let artwork = try await api.artwork(artworkId), job = try await api.importJob(jobId)
                guard artwork.revision == 2, artwork.approvedRevision == artwork.revision, artwork.approvedAssetId == job.assetId,
                      job.id == jobId, hayes.matchesReleasedDisplay(artwork, job: job, intent: intent) else {
                    throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
                }
                _ = try await bridge.hayesRelease(context, jobId: jobId, currentArtworkRevision: artwork.revision)
            }
            let snapshot = try await api.studioArtwork(artworkId)
            try NativeAutomationArtworkRules.checkRegistration(snapshot.artwork, artworkId: artworkId, revision: 2, tenantId: current.tenantId)
            try draft.refreshHayesArtwork(snapshot.artwork, placementId: placementId)
            var next = current; next.draft = draft
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "plaque-prepare", "wall-finish-prepare":
            try checkedRevision(request, current)
            guard !current.dirty, current.pending == nil, let draft = current.draft, let etag = current.etag else {
                throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED")
            }
            try await checkSavedEditPolicy(current, api: api, unsupportedCode: "NATIVE_VIEWPORT_CAPABILITY_UNAVAILABLE")
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let epoch = UUID().uuidString.lowercased()
            if operation == "plaque-prepare" {
                let command = try NativeAutomationPlaqueWall.plaque(request["command"])
                let frame = ViewportFrame(epoch: epoch, sequence: 1, tenantId: current.tenantId, principalId: current.principalId,
                    draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false, workspaceId: current.workspaceId, baseETag: etag)
                let proposal = try PlaqueViewportRequest(frame: frame, command: command, selectedPlaqueId: command.plaqueId)
                let reply = try await api.nativeAuthoringProposal(exhibitionId: draft.id, studioRevision: saved.revision, etag: etag, proposal: proposal.value)
                try Task.checkCancellation()
                let (_, _, refreshed) = try await checked(request)
                guard try value(refreshed) == value(current) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
                let now = try await api.exhibition(draft.id)
                let (_, _, finalState) = try await checked(request)
                try Task.checkCancellation()
                guard now.draft == draft, now.etag == etag, now.revision == saved.revision,
                      try value(finalState) == value(current) else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
                let preview = try proposal.checkedReply(reply, selectedPlaqueId: command.plaqueId)
                plaqueProposals[id] = (preview, etag)
                result = .object(["proposalId": .string(id), "candidate": preview.candidate])
            } else {
                let command = try NativeAutomationPlaqueWall.wall(request["command"])
                let frame = ViewportFrame(epoch: epoch, sequence: 1, tenantId: current.tenantId, principalId: current.principalId,
                    draft: draft, selection: ViewportSelection(kind: "surface", id: command.surfaceId), mode: "translate", view: "isometric", disabled: false, workspaceId: current.workspaceId, baseETag: etag)
                let proposal = try WallFinishViewportRequest(frame: frame, command: command)
                let reply = try await api.nativeAuthoringProposal(exhibitionId: draft.id, studioRevision: saved.revision, etag: etag, proposal: proposal.value)
                try Task.checkCancellation()
                let (_, _, refreshed) = try await checked(request)
                guard try value(refreshed) == value(current) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
                let now = try await api.exhibition(draft.id)
                let (_, _, finalState) = try await checked(request)
                try Task.checkCancellation()
                guard now.draft == draft, now.etag == etag, now.revision == saved.revision,
                      try value(finalState) == value(current) else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
                let preview = try proposal.checkedReply(reply)
                wallFinishProposals[id] = (preview, etag)
                result = .object(["proposalId": .string(id), "candidate": preview.candidate])
            }
        case "plaque-apply", "wall-finish-apply":
            try checkedRevision(request, current)
            guard !current.dirty, current.pending == nil, let draft = current.draft, let etag = current.etag,
                  let proposalId = request["proposalId"].string else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            try await checkSavedEditPolicy(current, api: api, unsupportedCode: "NATIVE_VIEWPORT_CAPABILITY_UNAVAILABLE")
            let saved = try await api.exhibition(draft.id)
            let (_, _, finalState) = try await checked(request)
            try Task.checkCancellation()
            guard saved.draft == draft, saved.etag == etag,
                  try value(finalState) == value(current) else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            var next = current
            if operation == "plaque-apply" {
                guard let (preview, expectedETag) = plaqueProposals[proposalId], expectedETag == etag else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                next.draft = try preview.apply(to: draft, selectedPlaqueId: preview.command.plaqueId, command: preview.command)
            } else {
                guard let (preview, expectedETag) = wallFinishProposals[proposalId], expectedETag == etag else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                next.draft = try preview.apply(to: draft, command: preview.command, selectedSurfaceId: preview.command.surfaceId)
            }
            try Task.checkCancellation()
            result = try value(await bridge.saveDocumentState(context, state: next))
            plaqueProposals.removeValue(forKey: proposalId); wallFinishProposals.removeValue(forKey: proposalId)
        case "plaque-cancel", "wall-finish-cancel":
            try checkedRevision(request, current)
            guard let proposalId = request["proposalId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let removed = operation == "plaque-cancel" ? plaqueProposals.removeValue(forKey: proposalId) != nil : wallFinishProposals.removeValue(forKey: proposalId) != nil
            guard removed else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            result = .object(["cancelled": .bool(true)])
        case "shared-bench-prepare":
            try checkedRevision(request, current)
            try await checkSavedEditPolicy(current, api: api)
            let fields = try NativeAutomationArtworkRules.command(request, keys: ["hostId", "pose"])
            guard current.pending == nil, let draft = current.draft, let etag = current.etag,
                  let hostId = fields["hostId"].string, let path = request["outputPath"].string,
                  request["inputPath"] == .null else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let pose = try FurnishingPose(fields["pose"])
            let command = try SharedFurnishingCommand(host: .bench, hostId: hostId, pose: pose)
            let catalog = try await api.sharedFurnishings()
            guard catalog.contains(where: { $0["catalogId"] == .string("backless-oak-bench") && $0["catalogVersion"] == .integer(1) }) else {
                throw BridgeFailure("FURNISHING_CATALOG_INVALID")
            }
            let frame = ViewportFrame(epoch: capability, sequence: 1, tenantId: current.tenantId, principalId: current.principalId,
                draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false,
                workspaceId: current.workspaceId, baseETag: etag)
            let proposal = try SharedFurnishingViewportRequest(frame: frame, command: command, selectedInspectorId: hostId)
            result = try write(.object(["version": .integer(1), "operation": .string("shared-furnishing-proposal"),
                "frame": try value(frame), "request": proposal.value]), to: path)
            sharedBenchProposals[id] = proposal
        case "shared-bench-apply":
            try checkedRevision(request, current)
            try await checkSavedEditPolicy(current, api: api)
            guard current.pending == nil, let draft = current.draft, let etag = current.etag,
                  let proposalId = request["proposalId"].string, let proposal = sharedBenchProposals[proposalId],
                  proposal.frame.draft == draft, proposal.frame.baseETag == etag,
                  let path = request["inputPath"].string, request["outputPath"] == .null else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            let reply = try Self.sharedBenchReply(root: root, path: path)
            let preview = try proposal.checkedReply(reply)
            var next = current; next.draft = try preview.apply(to: draft, command: proposal.command)
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "bench-prepare":
            try checkedRevision(request, current)
            try await checkSavedEditPolicy(current, api: api)
            guard current.pending == nil, let draft = current.draft, request["command"]["type"].string == "add-bench" else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let bench = try BenchSettings(value: request["command"]["bench"])
            guard case .object(let commandFields) = request["command"],
                  Set(commandFields.keys).isSubset(of: ["type", "bench", "viewpoint"]) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let viewpoint = request["command"]["viewpoint"] == .null ? nil : try BenchViewpoint(value: request["command"]["viewpoint"])
            let command = BenchCommand.add(bench, viewpoint: viewpoint)
            let frame = ViewportFrame(epoch: capability, sequence: 1, tenantId: current.tenantId, principalId: current.principalId, draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false, workspaceId: current.workspaceId, baseETag: current.etag)
            let proposal = try BenchViewportRequest(frame: frame, command: command, selectedBenchId: bench.id)
            proposals[id] = proposal
            let packet: DocumentValue = .object(["version": .integer(1), "operation": .string("bench-proposal"), "frame": try value(frame), "request": proposal.value])
            guard let path = request["outputPath"].string else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
            result = try write(packet, to: path)
        case "bench-apply":
            try checkedRevision(request, current)
            try await checkSavedEditPolicy(current, api: api)
            guard current.pending == nil, let draft = current.draft, let proposalId = request["proposalId"].string,
                  let proposal = proposals[proposalId], proposal.frame.draft == draft,
                  let path = request["inputPath"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let url = URL(fileURLWithPath: path)
            guard url.path.hasPrefix(root.path + "/"), url.resolvingSymlinksInPath() == url,
                  let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size <= 1_100_000 else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let reply = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: url))
            let preview = try proposal.checkedReply(reply, selectedBenchId: proposal.command.benchId)
            var next = current; next.draft = try preview.apply(to: draft, selectedBenchId: proposal.command.benchId, command: proposal.command)
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "artwork-placement-move", "artwork-interpretation-set":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: operation == "artwork-placement-move"
                ? ["placementId", "position", "etag"] : ["placementId", "expectedText", "text", "etag"])
            guard existingWorkspaceV5 != nil, selectedOEXFormat == nil, !current.dirty, current.pending == nil,
                  let etag = current.etag, command["etag"] == .string(etag), var draft = current.draft,
                  let placementId = command["placementId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let policy = try await api.nativeViewportPolicy(draft.id)
            let needsViewportCheck = try Self.operatorRequiresViewportValidation(policy, etag: etag)
            let saved = try await api.exhibition(draft.id)
            guard saved.etag == etag, saved.draft == draft, saved.revision == policy.revision else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            if operation == "artwork-placement-move" {
                guard let values = command["position"].items, values.count == 3,
                      values.allSatisfy({ $0.double != nil }) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                try draft.movePlacement(placementId, position: values.compactMap(\.double))
            } else {
                guard let previous = command["expectedText"].string, let text = command["text"].string else {
                    throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
                }
                try draft.setArtworkInterpretation(placementId, expectedText: previous, text: text)
            }
            if needsViewportCheck { try await api.validateNativeViewportEdit(draft, etag: etag) }
            try Task.checkCancellation()
            guard try value(await bridge.documentState(context)) == value(current) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
            let latest = try await api.exhibition(draft.id)
            guard latest.etag == etag, latest.draft == saved.draft else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let latestPolicy = try await api.nativeViewportPolicy(draft.id)
            guard latestPolicy.revision == saved.revision,
                  try Self.operatorRequiresViewportValidation(latestPolicy, etag: etag) == needsViewportCheck else {
                throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
            }
            guard try value(await bridge.documentState(context)) == value(current) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
            try Task.checkCancellation()
            var next = current; next.draft = draft
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "artwork-placement-rotate":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["placementId", "rotation", "etag"])
            guard existingWorkspaceV5 != nil, selectedOEXFormat == nil, !current.dirty, current.pending == nil,
                  fields["inputPath"] == nil, fields["outputPath"] == nil,
                  let etag = current.etag, command["etag"] == .string(etag), var draft = current.draft,
                  let placementId = command["placementId"].string, let items = command["rotation"].items,
                  items.count == 4 else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            try await checkSavedEditPolicy(current, api: api)
            try draft.rotatePlacement(placementId, rotation: items.compactMap(\.double))
            try await api.validateNativeViewportEdit(draft, etag: etag)
            var next = current
            next.draft = draft
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "save":
            guard let draft = current.draft else { throw BridgeFailure("EXHIBITION_REQUIRED") }
            var next = current
            if next.pending != nil {
                // A restarted ordinary operator must retain the original
                // request/draft/ETag and supply its refreshed current envelope.
                try Self.checkPendingSaveRequest(request, current, knownRevision: pendingRevisions[id],
                                                 ordinary: installedImageCLI || installedAuthoringCLI)
            } else {
                try checkedRevision(request, current)
                next.pending = .init(requestId: id, etag: next.etag, draft: draft)
                next = try await bridge.saveDocumentState(context, state: next)
                pendingRevisions[id] = next.revision
            }
            let saved = try await api.saveExhibition(draft, requestId: id, etag: next.etag)
            try saved.requireSameComparison(await api.exhibition(draft.id))
            next.draft = saved.draft; next.etag = saved.etag; next.savedEditVersion = saved.draft.document["editVersion"].integer; next.pending = nil
            result = try value(await bridge.saveDocumentState(context, state: next))
        case "reopen-saved":
            try checkedRevision(request, current)
            result = try await reopenSaved(current, context: context, api: api)
        case "export-static-status":
            guard let (device, inode) = installedExportRootIdentity else {
                throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED")
            }
            try checkedRevision(request, current)
            guard !current.dirty, let draft = current.draft, let etag = current.etag,
                  let path = request["outputPath"].string else {
                throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED")
            }
            _ = try installedExportDestination(path)
            let publication = try await bridge.publicationState(context)
            guard publication.pending == nil, let completed = publication.completed, completed.mode == nil,
                  completed.sourceExhibitionId == draft.id, completed.sourceEtag == etag,
                  let status = try await bridge.staticExportStatus(context), status.privateEditor == true,
                  status.workspaceId == current.workspaceId, status.tenantId == current.tenantId,
                  status.principalId == current.principalId, status.output == path,
                  status.publicationId == completed.publication.publicationId,
                  status.revisionSha256 == completed.publication.revisionSha256,
                  status.outputParentIdentity?.device == UInt64(bitPattern: Int64(device)),
                  status.outputParentIdentity?.inode == UInt64(inode),
                  try Self.exportRootIdentity(root, profile: profile) == (device, inode) else {
                throw BridgeFailure("AUTOMATION_EXPORT_CHANGED")
            }
            result = try value(status)
        case "export-oex":
            try checkedRevision(request, current)
            if installedExportRootIdentity != nil && authoringSession?.nativeOexExportProfile != "bounded-file-v1" {
                throw AuthoringFailure("NATIVE_OEX_EXPORT_UNAVAILABLE")
            }
            guard !current.dirty, let draft = current.draft, let etag = current.etag, let path = request["outputPath"].string else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            let url = try output(path)
            let reopened = try await api.exhibition(draft.id)
            guard reopened.draft == draft, reopened.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            let sha256: String
            let confirmed: Bool
            if authoringSession?.nativeOexExportProfile == "bounded-file-v1" {
                let archive = try await api.exportBoundedOEX(exhibitionId: draft.id, etag: etag)
                confirmed = try await archive.write(to: url, expectedParent: installedExportRootIdentity)
                sha256 = archive.sha256
            } else {
                let archive = try await api.exportOEX(exhibitionId: draft.id, etag: etag)
                confirmed = try await archive.writeBoundedVerified(to: url)
                sha256 = archive.sha256
            }
            var receipt: [String: DocumentValue] = ["path": confirmed ? .string(url.path) : .null,
                                                    "sha256": .string(sha256), "etag": .string(etag)]
            if !confirmed { receipt["commitState"] = .string("committed-path-unconfirmed") }
            result = .object(receipt)
        case "export-static-saved-head":
            result = try value(await exportSavedHead(request, id: id, context: context, current: current, api: api))
        case "export-static", "export-static-native-original":
            try checkedRevision(request, current)
            if installedExportRootIdentity != nil && authoringSession?.nativePublicationProfile != "bounded-local-v1" {
                throw AuthoringFailure("PUBLICATION_STAGE_UNAVAILABLE")
            }
            guard !current.dirty, let draft = current.draft, let etag = current.etag, let path = request["outputPath"].string else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            let nativeOriginal = operation == "export-static-native-original"
            if nativeOriginal {
                let command = try NativeAutomationArtworkRules.command(request, keys: ["etag"])
                guard existingWorkspaceV5 != nil, selectedOEXFormat == nil, current.pending == nil,
                      fields["inputPath"] == nil, command["etag"] == .string(etag),
                      authoringSession?.nativePublicationProfile == "bounded-local-v1" else {
                    throw BridgeFailure("NATIVE_DETAIL_UNAVAILABLE")
                }
            }
            let destination = URL(fileURLWithPath: path)
            guard destination.path.hasPrefix(root.path + "/"), destination.deletingLastPathComponent().resolvingSymlinksInPath() == root else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
            if !nativeOriginal && staticIntents[id] == nil { _ = try output(path) }
            let reopened = try await api.exhibition(draft.id)
            guard reopened.draft == draft, reopened.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            var publication = try await bridge.publicationState(context)
            let publicationProfile = nativeOriginal ? "native-original-v1"
                : authoringSession?.nativePublicationProfile == "bounded-local-v1" ? "bounded-local-v1" : nil
            // Session capabilities rotate on reconnect. The checked owner
            // scope and all durable request fields still bind exact replay.
            let originalRequestSHA256 = nativeOriginal
                ? Self.digest(try encoder.encode(DocumentValue.object(fields.filter { $0.key != "capability" }))) : nil
            let planBinding = try runtimeManifest.map { try nativePublicationPlanBinding(sourceExhibitionId: draft.id, etag: etag,
                runtimeManifestSha256: $0, watermark: draft.publicationWatermark(), publicationProfile: publicationProfile,
                nativeOriginalRequestSHA256: originalRequestSHA256) }
            if nativeOriginal {
                guard let planBinding else { throw BridgeFailure("NATIVE_DETAIL_UNAVAILABLE") }
                let intent = try NativePublicationPending(nativeOriginalSource: draft, etag: etag, requestId: id, planBindingSha256: planBinding)
                if let completed = publication.completed, completed.publishRequestId == id {
                    try completed.refuseNativeOriginalExportReplay(intent)
                }
                let before = publication
                try publication.stageNativeOriginal(intent)
                _ = try output(path)
                if publication != before { publication = try await bridge.savePublicationState(context, state: publication) }
                staticIntents[id] = intent
            } else if staticIntents[id] == nil {
                guard publication.pending == nil else { throw BridgeFailure("NATIVE_PUBLICATION_PENDING") }
                let intent = try NativePublicationPending(source: draft, etag: etag, planBindingSha256: planBinding)
                // Retain the exact operation-to-snapshot/request binding before
                // any native/API write. No retry creates replacement IDs.
                _ = try write(.object(["version": .integer(1), "requestId": .string(id),
                    "workspaceId": .string(current.workspaceId), "tenantId": .string(current.tenantId), "principalId": .string(current.principalId),
                    "expectedRevision": .integer(Int64(current.revision)), "expectedEditVersion": draft.document["editVersion"],
                    "requestSHA256": .string(Self.digest(bytes)), "outputPath": .string(path), "publicationIntent": try value(intent)]),
                    to: root.appendingPathComponent(".automation-static-" + id + ".json").path)
                staticIntents[id] = intent
                publication.pending = intent
                publication = try await bridge.savePublicationState(context, state: publication)
            }
            guard let intent = staticIntents[id], intent.sourceExhibitionId == draft.id, intent.sourceEtag == etag else { throw BridgeFailure("NATIVE_PUBLICATION_PENDING") }
            let frozen: NativePublication
            if let completed = publication.completed, completed.publishRequestId == intent.publishRequestId,
               completed.snapshotExhibitionId == intent.snapshot.id {
                frozen = completed.publication
            } else {
                if publication.pending == nil {
                    publication.pending = intent
                    publication = try await bridge.savePublicationState(context, state: publication)
                }
                guard var pending = publication.pending, pending.publishRequestId == intent.publishRequestId else { throw BridgeFailure("NATIVE_PUBLICATION_PENDING") }
                if pending.snapshotEtag == nil {
                    let saved = try await api.saveExhibition(pending.snapshot, requestId: pending.saveRequestId, etag: nil)
                    pending = try pending.confirmingSavedSnapshot(saved)
                    publication.pending = pending
                    publication = try await bridge.savePublicationState(context, state: publication)
                }
                guard let snapshotETag = pending.snapshotEtag else { throw BridgeFailure("PUBLICATION_PENDING") }
                try Task.checkCancellation()
                try pending.requirePlanBinding(planBinding)
                frozen = try await api.preparePublication(exhibitionId: pending.snapshot.id, etag: snapshotETag, requestId: pending.publishRequestId, publicationProfile: publicationProfile)
                publication.completed = NativePublicationCompleted(pending: pending, publication: frozen); publication.pending = nil
                publication = try await bridge.savePublicationState(context, state: publication)
            }
            if nativeOriginal {
                let matches = try await api.publications(exhibitionId: draft.id).filter { $0.publicationId == frozen.publicationId }
                guard matches == [frozen], frozen.status == "published" else { throw BridgeFailure("PUBLICATION_SCOPE_CHANGED") }
                try reopened.requireSameComparison(await api.exhibition(draft.id))
            }
            let existing = try await bridge.staticExportStatus(context)
            let reuse = try existing?.canReuse(publicationId: frozen.publicationId, output: path) == true
            if nativeOriginal && reuse { throw BridgeFailure("NATIVE_DETAIL_EXPORT_REPLAY_UNAVAILABLE") }
            let detailJobId: String?
            if !reuse {
                guard !FileManager.default.fileExists(atPath: path) else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
                if nativeOriginal {
                    try publication.claimNativeOriginalExport(intent)
                    publication = try await bridge.savePublicationState(context, state: publication)
                }
                let started = try await bridge.startStaticExport(context, publicationId: frozen.publicationId, output: destination,
                                                                 privateEditor: true, nativeOriginalDetail: nativeOriginal,
                                                                 outputParentIdentity: installedExportRootIdentity)
                detailJobId = nativeOriginal ? started.jobId : nil
            } else { detailJobId = nil }
            result = try value(await waitForStaticExport(context, publication: frozen, path: path,
                                                         nativeOriginal: nativeOriginal, detailJobId: detailJobId))
        default: throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED")
        }
        if !Self.freshResponseOperations.contains(operation) { replay[id] = (bytes, result) }
        return result
    }
    private func waitForStaticExport(_ context: Context, publication frozen: NativePublication, path: String,
                                     nativeOriginal: Bool, detailJobId: String?) async throws -> NativeStaticExportReceipt {
        do {
            var receipt: NativeStaticExportReceipt?
            for _ in 0..<1200 {
                try Task.checkCancellation()
                if let status = try await bridge.staticExportStatus(context), !status.running {
                    if nativeOriginal && !status.confirmsDetailTerminal(jobId: detailJobId, publicationId: frozen.publicationId, output: path) {
                        throw BridgeFailure("AUTOMATION_DRAIN_UNCERTAIN")
                    }
                    guard let accepted = status.receipt, status.errorCode == nil else { throw BridgeFailure(status.errorCode ?? "NATIVE_STATIC_EXPORT_FAILED") }
                    receipt = accepted; break
                }
                try await Task.sleep(for: .milliseconds(100))
            }
            guard let receipt else { throw BridgeFailure("NATIVE_TIMEOUT") }
            if nativeOriginal {
                guard receipt.publicationId == frozen.publicationId, receipt.revisionSha256 == frozen.revisionSha256,
                      receipt.output == path, FileManager.default.fileExists(atPath: path) else {
                    throw BridgeFailure("PUBLICATION_SCOPE_CHANGED")
                }
            }
            return receipt
        } catch {
            let ownedBridge = bridge
            // Both the cancellation request and termination observations
            // must run outside the already-cancelled operation task.
            let drained = (try? await Task.detached {
                _ = try await ownedBridge.cancelStaticExport(context)
                for _ in 0..<1200 {
                    if let status = try await ownedBridge.staticExportStatus(context), !status.running {
                        return !nativeOriginal || status.confirmsDetailTerminal(jobId: detailJobId, publicationId: frozen.publicationId, output: path)
                    }
                    try await Task.sleep(for: .milliseconds(100))
                }
                return false
            }.value) ?? false
            if drained { throw error }
            throw BridgeFailure("AUTOMATION_DRAIN_UNCERTAIN")
        }
    }

    public func close() async throws {
        guard !busy, !closing else { throw BridgeFailure("AUTOMATION_BUSY") }
        guard !closed else { return }
        closing = true
        defer { closing = false }
        try await closeBridge()
        await api?.endLocalAuthoringSession()
        authoringSession = nil
        closed = true
    }
    private func closeBridge() async throws {
        if let closeFailure { throw closeFailure }
        guard bridgeOpen else { return }
        do {
            try await bridge.close()
            bridgeOpen = false
        } catch {
            closeFailure = error
            throw error
        }
    }
    /// Control authorization precedes cancellation or shutdown side effects.
    public func authorizeControl(_ request: DocumentValue) async throws {
        try Self.checkPreservationOperation(request["operation"].string, manifest: runtimeManifest, allowed: "close")
        guard !closed, !closing, context != nil, let identity,
              request["capability"].string == capability,
              request["workspaceId"].string == identity.workspaceId,
              request["tenantId"].string == identity.tenantId,
              request["principalId"].string == identity.principalId else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
    }
    public func nativeStatusValue() async throws -> DocumentValue {
        guard let context, !closed, bridgeOpen else { throw BridgeFailure("AUTOMATION_NOT_READY") }
        let host = try await bridge.nativeStatus(context)
        let transfer = try? await bridge.oexImportState(context)
        return .object(["operation": .string(host.operation), "busy": .bool(host.busy),
            "errorCode": host.errorCode.map(DocumentValue.string) ?? .null,
            "runtimeState": host.runtime.map { .string($0.state) } ?? .null,
            "runtimeHasOwnedProcesses": host.runtime.map { .bool($0.hasOwnedProcesses) } ?? .null,
            "retainedImportAvailable": .bool(transfer != nil),
            "retainedImport": try transfer.map { try value($0) } ?? .null])
    }
    public func cancelOwnedRuntime() async throws -> Bool {
        guard !busy, !closing, let context, !closed else { throw BridgeFailure("AUTOMATION_BUSY") }
        if let closeFailure { throw closeFailure }
        if !bridgeOpen { return true }
        _ = try await bridge.stop(context)
        for _ in 0..<1200 {
            let status = try await bridge.nativeStatus(context)
            if !status.busy, let runtime = status.runtime, !runtime.hasOwnedProcesses,
               ["stopped", "failed"].contains(runtime.state) {
                runtimeStopped = true
                return true
            }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw BridgeFailure("AUTOMATION_DRAIN_UNCERTAIN")
    }
}
