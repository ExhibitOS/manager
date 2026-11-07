// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import Darwin

/// Private agent adapter to the same native document journal and authenticated
/// API used by ManagerApp. No credentials or arbitrary executable are exposed.
public actor NativeAutomationSession {
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
    private let helper: URL
    private let profile: URL
    private let temporaryRoot: URL
    private let ports: [String: UInt16]
    private let workspaceV5ValidationRuntime: URL?
    private let workspaceV6ValidationRuntime: URL?
    private let workspaceV7ValidationRuntime: URL?
    private let existingWorkspaceV5: ExistingWorkspaceV5?
    private let root: URL
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
    private var surfaceProposals: [String: SurfaceMaterialTextureRequest] = [:]
    private var pendingRevisions: [String: UInt64] = [:]
    private var staticIntents: [String: NativePublicationPending] = [:]
    private var selectedOEXFormat: String?
    private var runtimeStopped = false

    public init(helper: URL, profile: URL, ports: [String: UInt16], outputRoot: URL, temporaryRoot: URL, inputs: [String: String], selectedOEX9: Bool = false, selectedOEX10: Bool = false, workspaceV5ValidationRuntime: URL? = nil, existingWorkspaceV5: ExistingWorkspaceV5? = nil, workspaceV6ValidationRuntime: URL? = nil, workspaceV7ValidationRuntime: URL? = nil) throws {
        let draft12 = workspaceV6ValidationRuntime != nil || workspaceV7ValidationRuntime != nil
        guard !(selectedOEX9 && selectedOEX10),
              workspaceV6ValidationRuntime == nil || workspaceV7ValidationRuntime == nil,
              !draft12 || (!selectedOEX9 && !selectedOEX10 && workspaceV5ValidationRuntime == nil),
              existingWorkspaceV5 == nil || (!selectedOEX9 && !selectedOEX10
                  && (workspaceV5ValidationRuntime != nil || workspaceV7ValidationRuntime != nil)),
              workspaceV5ValidationRuntime == nil || selectedOEX10 || existingWorkspaceV5 != nil else {
            throw BridgeFailure("AUTOMATION_CONFIGURATION_INVALID")
        }
        try existingWorkspaceV5?.check()
        if existingWorkspaceV5 != nil { _ = try Self.profileIdentity(profile) }
        guard outputRoot.isFileURL, outputRoot.path.hasPrefix("/"),
              FileManager.default.fileExists(atPath: outputRoot.path),
              outputRoot.resolvingSymlinksInPath().path == outputRoot.path,
              !profile.path.hasPrefix(outputRoot.path + "/") else { throw BridgeFailure("AUTOMATION_PATH_INVALID") }
        self.root = outputRoot; self.inputs = inputs
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
    public func open() async throws -> DocumentValue {
        guard context == nil, !closed else { throw BridgeFailure("AUTOMATION_ALREADY_OPEN") }
        let context = try await bridge.context(); self.context = context
        if let existingWorkspaceV5 { return try await openExistingWorkspace(existingWorkspaceV5, context: context) }
        if selectedOEXFormat == nil { _ = try await bridge.prepare(context); _ = try await waitReady() }
        _ = try await bridge.start(context)
        let status = try await waitReady()
        guard status.runtime?.readiness.ready == true, let config = status.configuration else { throw BridgeFailure("NATIVE_NOT_READY") }
        let credentials = try await bridge.authoringSession(context)
        let api = try AuthoringAPI(port: config.ports.web, tenantId: credentials.tenantId, nativeRuntimeManifestSHA256: config.manifestSha256)
        authoringSession = try await api.connect(credentials); self.api = api; runtimeManifest = config.manifestSha256
        identity = try await bridge.documentState(context)
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
        let ready = try await waitReady()
        guard ready.runtime?.readiness.ready == true, ready.selectedOex != true,
              ready.configuration?.manifestSha256 == expected.manifestSHA256,
              try profileIdentity() == directory else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        let credentials = try await bridge.authoringSession(context)
        guard credentials.tenantId == expected.tenantId, credentials.principalId == expected.principalId else {
            throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
        }
        let api = try AuthoringAPI(port: config.ports.web, tenantId: expected.tenantId, nativeRuntimeManifestSHA256: config.manifestSha256)
        let session = try await api.connect(credentials)
        guard await api.admitsWorkspaceMaterials else { throw AuthoringFailure("WORKSPACE_MATERIAL_UNAVAILABLE") }
        let stored = try await api.exhibition(draft.id)
        guard stored.draft == draft, stored.etag == etag,
              try value(await bridge.documentState(context)) == value(saved) else {
            throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
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
        guard previous.runtime?.readiness.ready == true, let previousJob = previous.runtime?.jobId else { throw BridgeFailure("NATIVE_NOT_READY") }
        let directory = try profileIdentity()

        runtimeStopped = true
        proposals.removeAll(); surfaceProposals.removeAll()
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
                                  workspaceV7ValidationRuntime: workspaceV7ValidationRuntime)
        bridgeOpen = true
        let reopened = try await bridge.context()
        self.context = reopened
        guard try profileIdentity() == directory, reopened.activeId == context.activeId,
              try value(await bridge.documentState(reopened)) == value(current) else {
            throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED")
        }
        let persisted = try await bridge.nativeStatus(reopened)
        guard let config = persisted.configuration, config.manifestSha256 == manifest,
              config.ports.web == ports["web"], config.ports.api == ports["api"], config.ports.database == ports["database"],
              !persisted.busy, persisted.runtime?.hasOwnedProcesses != true else {
            throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
        }
        try Task.checkCancellation()
        _ = try await bridge.start(reopened)
        let ready = try await waitReady()
        guard ready.runtime?.readiness.ready == true, let job = ready.runtime?.jobId, job != previousJob,
              ready.configuration?.manifestSha256 == manifest else { throw BridgeFailure("NATIVE_NOT_READY") }
        let credentials = try await bridge.authoringSession(reopened)
        let reopenedAPI = try AuthoringAPI(port: config.ports.web, tenantId: current.tenantId, nativeRuntimeManifestSHA256: manifest)
        let session = try await reopenedAPI.connect(credentials)
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
        return (context, api, try await bridge.documentState(context))
    }
    private func checkedRevision(_ request: DocumentValue, _ state: NativeDocumentState) throws {
        guard case .object(let fields) = request, fields["expectedRevision"] != nil, fields["expectedEditVersion"] != nil,
              request["expectedRevision"].integer == Int64(state.revision),
              request["expectedEditVersion"] == (state.draft?.document["editVersion"] ?? .null) else { throw BridgeFailure("NATIVE_DOCUMENT_STATE_CHANGED") }
    }
    private func output(_ path: String) throws -> URL {
        let url = URL(fileURLWithPath: path)
        guard url.path.hasPrefix(root.path + "/"), url.deletingLastPathComponent().resolvingSymlinksInPath() == root,
              !FileManager.default.fileExists(atPath: url.path) else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
        return url
    }
    private func checkSavedEditPolicy(_ state: NativeDocumentState, api: AuthoringAPI, unsupportedCode: String = "NATIVE_BENCH_CAPABILITY_UNAVAILABLE") async throws {
        guard let draft = state.draft, let etag = state.etag else { return }
        let policy = try await api.nativeViewportPolicy(draft.id)
        guard policy.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        guard !policy.boundedReadOnly, policy.boundedEditRequired != true else { throw BridgeFailure(unsupportedCode) }
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
        try bytes.write(to: url, options: .withoutOverwriting)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
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
            let policy: WorkspaceArtworkFramePreparationPolicy?
            if let input = fields.removeValue(forKey: "preparationPolicy") {
                guard let name = input.string, let selected = WorkspaceArtworkFramePreparationPolicy(rawValue: name) else {
                    throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
                }
                policy = selected
            } else {
                policy = nil
            }
            guard Set(fields.keys) == commandKeys else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let command = DocumentValue.object(fields)
            let details = try JSONDecoder().decode(WorkspaceFurnishingDetails.self, from: JSONEncoder().encode(command))
            guard let id = request["requestId"].string, try value(details) == command else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            return try value(await api.createWorkspaceFurnishing(requestId: id, details: details, preparationPolicy: policy))
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
            let command = try NativeAutomationArtworkRules.command(request, keys: ["intakeId"])
            guard let id = command["intakeId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let intake = try await api.workspaceFurnishingIntake(id)
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

    public func execute(_ request: DocumentValue) async throws -> DocumentValue {
        guard request["version"] == .integer(1), let id = request["requestId"].string, UUID(uuidString: id) != nil,
              let operation = request["operation"].string, case .object(let fields) = request,
              Set(fields.keys).isSubset(of: ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "expectedRevision", "expectedEditVersion", "inputPath", "outputPath", "command", "proposalId", "archiveFormat"]) else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
        let bytes = try encoder.encode(request)
        // Scope is checked even for replay: correlation is not authorization.
        let (context, api, current) = try await checked(request)
        if let seen = seenRequests[id], seen != bytes { throw BridgeFailure("AUTOMATION_REQUEST_CHANGED") }
        if let prior = replay[id] {
            guard prior.0 == bytes else { throw BridgeFailure("AUTOMATION_REQUEST_CHANGED") }
            return prior.1
        }
        guard !busy, seenRequests.count < 128 else { throw BridgeFailure("AUTOMATION_BUSY") }
        guard !runtimeStopped || operation == "read" else { throw BridgeFailure("NATIVE_NOT_READY") }
        seenRequests[id] = bytes
        busy = true; defer { busy = false }
        let result: DocumentValue
        switch operation {
        case "read": result = try await statusValue()
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
        case "workspace-furnishing-place":
            guard selectedOEXFormat == nil, await api.admitsWorkspaceFurnishings else {
                throw BridgeFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["id", "roomId", "furnishingId", "furnishingVersion", "position", "yaw", "scale"])
            let placement = try JSONDecoder().decode(WorkspaceFurnishingPlacement.self, from: JSONEncoder().encode(command))
            guard try value(placement) == command else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            if let draft = try await api.prepareWorkspaceFurnishingPlacement(placement, in: current) {
                var next = current
                next.draft = draft
                result = .object(["changed": .bool(true), "document": try value(await bridge.saveDocumentState(context, state: next))])
            } else {
                result = .object(["changed": .bool(false), "document": try value(current)])
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
            guard let opening = fields["openingId"].string,
                  !attaching || (fields["furnishingId"].string != nil && fields["furnishingVersion"] == .integer(1)
                    && fields["modelSha256"].string == WorkspacePortalFinish.modelSHA256) else {
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
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artistId", "metadata"])
            guard let artist = command["artistId"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let metadata = try JSONDecoder().decode(ArtworkMetadata.self, from: JSONEncoder().encode(command["metadata"]))
            guard try value(metadata) == command["metadata"] else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            try beginCatalogCreation(request, id: id)
            let created = try value(await api.saveArtwork(id: nil, revision: nil, artistId: artist, metadata: metadata))
            _ = try write(created, to: root.appendingPathComponent(".automation-catalog-" + id + ".result.json").path)
            result = created
        case "import-glb":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "artworkRevision", "scaleMeters"])
            guard selectedOEXFormat == nil, current.pending == nil, let path = request["inputPath"].string,
                  let expected = inputs[path], let artworkId = command["artworkId"].string,
                  let revision = command["artworkRevision"].integer, let scale = command["scaleMeters"].double else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let url = URL(fileURLWithPath: path)
            guard url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let file = try await ImportFileSnapshot.load(url)
            guard file.mime == "model/gltf-binary", file.sha256 == expected else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
            let artwork = try await api.artwork(artworkId)
            guard Int64(artwork.revision) == revision, !artwork.archived, let metadata = artwork.metadata else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
            try NativeAutomationArtworkRules.checkDisplayAsset(metadata, sha256: file.sha256, scaleMeters: scale)
            let intent = NativeImportIntent(artworkId: artwork.id, idempotencyKey: id, mime: file.mime,
                sha256: file.sha256, bytes: file.data.count, scaleMeters: scale, rights: metadata.rights)
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            let job = try await NativeAutomationGLBTransfer.run(bridge: bridge, context: context, state: state,
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
                try NativeAutomationArtworkRules.checkJob(observed, id: job)
                fields["job"] = NativeAutomationArtworkRules.jobValue(observed)
            }
            result = .object(fields)
        case "artwork-approve":
            try checkedRevision(request, current)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["artworkId", "revision", "assetId", "acknowledgeRights"])
            guard command["acknowledgeRights"] == .bool(true), let artworkId = command["artworkId"].string,
                  let revision = command["revision"].integer, let assetId = command["assetId"].string else { throw BridgeFailure("APPROVAL_REVIEW_REQUIRED") }
            let state = try await bridge.uploadState(context)
            try NativeAutomationArtworkRules.checkUploadScope(state, document: current)
            guard let intent = state.intent, intent.artworkId == artworkId, intent.mime == "model/gltf-binary",
                  intent.derived == nil, intent.modelDerivation == nil, let jobId = state.jobId else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            let job = try await api.importJob(jobId), artwork = try await api.artwork(artworkId)
            try NativeAutomationArtworkRules.checkApproval(artwork: artwork, job: job, jobId: jobId,
                intent: intent, revision: revision, assetId: assetId)
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
            try NativeAutomationArtworkRules.checkRegistration(approved.artwork, artworkId: artworkId, revision: revision, tenantId: current.tenantId)
            try draft.addApprovedArtwork(approved.artwork, roomId: room, placementId: placement, transform: command["transform"])
            var next = current; next.draft = draft
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
        case "save":
            guard let draft = current.draft else { throw BridgeFailure("EXHIBITION_REQUIRED") }
            var next = current
            if let pending = next.pending {
                // An exact explicitly repeated request may recover only the
                // journal revision this session wrote before the uncertain API
                // call. It never rewrites expectedRevision or issues a new ID.
                guard pending.requestId == id, pending.draft == draft, pending.etag == next.etag,
                      pendingRevisions[id] == current.revision,
                      request["expectedEditVersion"] == draft.document["editVersion"] else { throw BridgeFailure("EXHIBITION_PENDING") }
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
        case "export-oex":
            try checkedRevision(request, current)
            guard !current.dirty, let draft = current.draft, let etag = current.etag, let path = request["outputPath"].string else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            let url = try output(path)
            let reopened = try await api.exhibition(draft.id)
            guard reopened.draft == draft, reopened.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            if authoringSession?.nativeOexExportProfile == "bounded-file-v1" {
                let archive = try await api.exportBoundedOEX(exhibitionId: draft.id, etag: etag)
                try await archive.write(to: url)
            } else {
                let archive = try await api.exportOEX(exhibitionId: draft.id, etag: etag)
                try await archive.writeBoundedVerified(to: url)
            }
            result = .object(["path": .string(url.path), "sha256": .string(Self.digest(try Data(contentsOf: url))), "etag": .string(etag)])
        case "export-static":
            try checkedRevision(request, current)
            guard !current.dirty, let draft = current.draft, let etag = current.etag, let path = request["outputPath"].string else { throw BridgeFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            let destination = URL(fileURLWithPath: path)
            guard destination.path.hasPrefix(root.path + "/"), destination.deletingLastPathComponent().resolvingSymlinksInPath() == root else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
            if staticIntents[id] == nil { _ = try output(path) }
            let reopened = try await api.exhibition(draft.id)
            guard reopened.draft == draft, reopened.etag == etag else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
            var publication = try await bridge.publicationState(context)
            let publicationProfile = authoringSession?.nativePublicationProfile == "bounded-local-v1" ? "bounded-local-v1" : nil
            let planBinding = try runtimeManifest.map { try nativePublicationPlanBinding(sourceExhibitionId: draft.id, etag: etag,
                runtimeManifestSha256: $0, watermark: draft.publicationWatermark(), publicationProfile: publicationProfile) }
            if staticIntents[id] == nil {
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
            let existing = try await bridge.staticExportStatus(context)
            if try existing?.canReuse(publicationId: frozen.publicationId, output: path) != true {
                guard !FileManager.default.fileExists(atPath: path) else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
                _ = try await bridge.startStaticExport(context, publicationId: frozen.publicationId, output: destination, privateEditor: true)
            }
            do {
                var receipt: NativeStaticExportReceipt?
                for _ in 0..<1200 {
                    try Task.checkCancellation()
                    if let status = try await bridge.staticExportStatus(context), !status.running {
                        guard let accepted = status.receipt, status.errorCode == nil else { throw BridgeFailure(status.errorCode ?? "NATIVE_STATIC_EXPORT_FAILED") }
                        receipt = accepted; break
                    }
                    try await Task.sleep(for: .milliseconds(100))
                }
                guard let receipt else { throw BridgeFailure("NATIVE_TIMEOUT") }
                result = try value(receipt)
            } catch {
                let ownedBridge = bridge
                // Both the cancellation request and termination observations
                // must run outside the already-cancelled operation task.
                let drained = (try? await Task.detached {
                    _ = try await ownedBridge.cancelStaticExport(context)
                    for _ in 0..<1200 {
                        if let status = try await ownedBridge.staticExportStatus(context), !status.running { return true }
                        try await Task.sleep(for: .milliseconds(100))
                    }
                    return false
                }.value) ?? false
                if drained { throw error }
                throw BridgeFailure("AUTOMATION_DRAIN_UNCERTAIN")
            }
        default: throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED")
        }
        replay[id] = (bytes, result)
        return result
    }
    public func close() async throws {
        guard !busy, !closing else { throw BridgeFailure("AUTOMATION_BUSY") }
        guard !closed else { return }
        closing = true
        defer { closing = false }
        try await closeBridge()
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
