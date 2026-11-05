// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import Darwin

/// Private agent adapter to the same native document journal and authenticated
/// API used by ManagerApp. No credentials or arbitrary executable are exposed.
public actor NativeAutomationSession {
    private var bridge: NativeBridge
    private var bridgeOpen = true
    private var closeFailure: Error?
    private var closing = false
    private let helper: URL
    private let profile: URL
    private let temporaryRoot: URL
    private let ports: [String: UInt16]
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

    public init(helper: URL, profile: URL, ports: [String: UInt16], outputRoot: URL, temporaryRoot: URL, inputs: [String: String], selectedOEX9: Bool = false, selectedOEX10: Bool = false) throws {
        guard !selectedOEX9 || !selectedOEX10 else { throw BridgeFailure("AUTOMATION_CONFIGURATION_INVALID") }
        guard outputRoot.isFileURL, outputRoot.path.hasPrefix("/"),
              FileManager.default.fileExists(atPath: outputRoot.path),
              outputRoot.resolvingSymlinksInPath().path == outputRoot.path,
              !profile.path.hasPrefix(outputRoot.path + "/") else { throw BridgeFailure("AUTOMATION_PATH_INVALID") }
        self.root = outputRoot; self.inputs = inputs
        self.helper = helper; self.profile = profile; self.temporaryRoot = temporaryRoot; self.ports = ports
        selectedOEXFormat = selectedOEX10 ? "1.0.0-draft.10" : selectedOEX9 ? "1.0.0-draft.9" : nil
        for url in [outputRoot, temporaryRoot] {
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard attributes[.type] as? FileAttributeType == .typeDirectory,
                  (attributes[.posixPermissions] as? NSNumber)?.intValue == 0o700,
                  (attributes[.ownerAccountID] as? NSNumber)?.uint32Value == getuid(),
                  url.resolvingSymlinksInPath() == url else { throw BridgeFailure("AUTOMATION_PATH_INVALID") }
        }
        bridge = try NativeBridge(helper: helper, profile: profile, freshSelectedOEX: selectedOEX9 || selectedOEX10,
                                  freshWorkspacePorts: ports, temporaryDirectory: temporaryRoot)
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
        guard let runtime = stopped.runtime, runtime.state == "stopped", !runtime.hasOwnedProcesses else {
            throw BridgeFailure("AUTOMATION_DRAIN_UNCERTAIN")
        }
        try await closeBridge()
        try Task.checkCancellation()
        guard try profileIdentity() == directory else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        bridge = try NativeBridge(helper: helper, profile: profile, temporaryDirectory: temporaryRoot)
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
    // These existing CMS create endpoints have no idempotency key. Preserve
    // the exact attempt before POST and never automatically repeat an uncertain
    // creation. The consumer inspects catalog-read and this durable receipt.
    private func beginCatalogCreation(_ request: DocumentValue, id: String) throws {
        let path = root.appendingPathComponent(".automation-catalog-" + id + ".json").path
        guard !FileManager.default.fileExists(atPath: path) else { throw BridgeFailure("AUTOMATION_CREATE_UNCERTAIN") }
        guard case .object(let fields) = request else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        _ = try write(.object(fields.filter { $0.key != "capability" }), to: path)
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
        case "import-oex9", "import-oex10":
            try checkedRevision(request, current)
            let archiveFormat = operation == "import-oex10" ? "1.0.0-draft.10" : "1.0.0-draft.9"
            guard selectedOEXFormat == archiveFormat, current.draft == nil, current.pending == nil,
                  request["archiveFormat"].string == archiveFormat, let path = request["inputPath"].string,
                  let expectedHash = inputs[path], let outputPath = request["outputPath"].string else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            _ = try output(outputPath)
            let status = try await bridge.nativeStatus(context)
            guard status.selectedOex == true, status.oex027Qualified == true,
                  archiveFormat != "1.0.0-draft.10" || status.oex028Qualified == true else {
                throw BridgeFailure("NATIVE_OEX_RELEASE_UNAVAILABLE")
            }
            let source = URL(fileURLWithPath: path)
            guard source.resolvingSymlinksInPath() == source else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
            let file = try await NativeOEXImportFile.loadForBoundedProfile(source)
            guard file.archive.sha256 == expectedHash else { throw BridgeFailure("AUTOMATION_INPUT_CHANGED") }
            if archiveFormat == "1.0.0-draft.10" {
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
            let recorded = try write(.object(["archiveSHA256": .string(expectedHash), "archiveFormat": .string(archiveFormat),
                "jobId": .string(transfer.1.id), "requestId": .string(id), "document": try value(stored), "importReceipt": receipt]), to: outputPath)
            // The supported selected-owner transition must drain normally
            // before ordinary editing can start in the imported workspace.
            _ = try await bridge.stop(context); _ = try await waitReady()
            _ = try await bridge.start(context); _ = try await waitReady()
            authoringSession = try await api.connect(bridge.authoringSession(context))
            selectedOEXFormat = nil
            result = .object(["document": try value(stored), "receipt": recorded])
        case "floor-appearance-apply":
            try checkedRevision(request, current)
            let policy = try await checkSavedSurfacePolicy(current, api: api)
            let command = try NativeAutomationArtworkRules.command(request, keys: ["roomId", "surfaceId", "preset"])
            guard selectedOEXFormat == nil, var draft = current.draft,
                  let room = command["roomId"].string, let surface = command["surfaceId"].string,
                  let name = command["preset"].string, let preset = FloorSettings.Preset(rawValue: name) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            try draft.applyFloorAppearance(preset, roomId: room, surfaceId: surface)
            _ = try await api.floorPreview(preset)
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
            if let existing = try await bridge.staticExportStatus(context) {
                guard existing.publicationId == frozen.publicationId, existing.output == path else { throw BridgeFailure("AUTOMATION_EXPORT_CHANGED") }
            } else {
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
