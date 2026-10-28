// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import UniformTypeIdentifiers
import ManagerCore

@MainActor
final class AuthoringModel: ObservableObject {
    @Published var subject = ""
    @Published var password = ""
    @Published var query = ""
    @Published var artistName = ""
    @Published var artistBio = ""
    @Published var creationYearText = ""
    @Published var draft = ArtworkMetadata()
    @Published var artistId = ""
    @Published private(set) var session: AuthoringSession?
    @Published private(set) var artworks: [ArtworkSummary] = []
    @Published private(set) var artists: [NativeArtist] = []
    @Published private(set) var selected: NativeArtwork?
    @Published private(set) var editing = false
    @Published private(set) var busy = false
    @Published private(set) var failure: AuthoringFailure?
    @Published private(set) var notice = ""
    @Published private(set) var cursor: String?
    @Published private(set) var importFile: ImportFileSnapshot?
    @Published private(set) var transfer: NativeUploadState?
    @Published private(set) var importJob: NativeImportJob?
    @Published var importScale = 1.0
    @Published var approvalAcknowledged = false
    @Published var assetId = ""
    private var readTransfer: (() async throws -> NativeUploadState)?
    private var writeTransfer: ((NativeUploadState) async throws -> NativeUploadState)?
    private let importTransfer = NativeImportTransfer()
    private var importRead: Task<ImportFileSnapshot, Error>?
    private var imagePreparation: Task<NativePreparedJPEG, Error>?
    private var imagePreparationScope: NativeImagePreparationScope?
    // The native schema-21 packet has real original/derived custody evidence.
    // Other runtime manifests keep the existing PNG/GLB chooser and contract.
    @Published private(set) var jpegImportsEnabled = false
    @Published private(set) var modelDerivationEnabled = false
    var modelWorkerPending: Bool { importTransfer.hasPendingModelWorker }
    var modelOutputsCurrent: Bool {
        guard let intent = transfer?.intent, let job = importJob, let selected else { return false }
        return job.modelDerivation?.currentOutputs(in: selected, intent: intent, job: job) == true
    }
    var importFormats: String { jpegImportsEnabled ? "GLB·PNG·JPEG" : "GLB·PNG" }
    var jpegOriginalStored: Bool {
        guard transfer?.intent?.mime == "image/jpeg", transfer?.intent?.derived?.valid == true,
              let job = importJob, job.state == "approved", transfer?.jobId?.lowercased() == job.id.lowercased(),
              let original = job.originalAssetId, let output = job.assetId,
              UUID(uuidString: original) != nil, UUID(uuidString: output) != nil else { return false }
        return original.lowercased() != output.lowercased()
    }
    private var importPoll: Task<Void, Never>?
    private var importPollID: UUID?
    private var importStatusGeneration = UUID()
    private var pollingImportStatus = false
    private var original = ArtworkMetadata()
    private var api: AuthoringAPI?
    var exhibitionAPI: AuthoringAPI? { session == nil ? nil : api }
    private var scope: String?
    private var runtimeManifestSHA256: String?
    private var editingUser: String?
    private var available = false
    var metadataDirty: Bool { editing && (draft != original || selected == nil || creationYearText != (original.creationYear.map(String.init) ?? "")) }

    var dirty: Bool { metadataDirty || (importFile != nil && transfer?.intent == nil) }

    func configure(_ configuration: NativeConfiguration?, ready: Bool) {
        let requestedScope = configuration.flatMap { configuration in
            configuration.tenantId.map { "\($0):\(configuration.ports.web):\(configuration.manifestSha256)" }
        }
        if modelWorkerPending, requestedScope != scope {
            // Keep the old durable intent, transport, manifest and polling
            // context until that worker drains or its owning runtime stops.
            available = false; modelDerivationEnabled = false; jpegImportsEnabled = false
            importStatusGeneration = UUID()
            imagePreparationScope?.invalidate(); imagePreparation?.cancel()
            failure = AuthoringFailure("IMPORT_DRAIN_PENDING")
            return
        }
        available = ready
        let previousManifest = runtimeManifestSHA256
        runtimeManifestSHA256 = configuration?.manifestSha256
        modelDerivationEnabled = configuration?.kind == "personal" && NativeModelDerivationProfile.supports(manifestSHA256: runtimeManifestSHA256, ready: ready)
        jpegImportsEnabled = ready && (runtimeManifestSHA256 == "74b08c7899d74ad989bbb61e47fc86fe9fa25c8853ab27ce57e9135c4932e96a"
            || PublicationWatermarkRuntime.supports(manifestSHA256: runtimeManifestSHA256, ready: ready))
        if !ready || previousManifest != runtimeManifestSHA256 {
            importStatusGeneration = UUID()
            imagePreparationScope?.invalidate(); imagePreparation?.cancel()
        }
        guard let configuration, let tenant = configuration.tenantId else { return }
        let next = "\(tenant):\(configuration.ports.web):\(configuration.manifestSha256)"
        guard next != scope else { return }
        imagePreparationScope?.invalidate(); imagePreparation?.cancel()
        importStatusGeneration = UUID()
        guard !busy, !dirty else { api = nil; session = nil; failure = AuthoringFailure("LOCAL_SCOPE_CHANGED"); return }
        importPoll?.cancel(); importPoll = nil; importPollID = nil
        transfer = nil; importJob = nil; importFile = nil; assetId = ""
        readTransfer = nil; writeTransfer = nil
        scope = next; session = nil; artworks = []; artists = []; selected = nil; editing = false
        subject = configuration.subject ?? ""
        do { api = try AuthoringAPI(port: configuration.ports.web, tenantId: tenant) }
        catch { failure = AuthoringFailure("LOCAL_SCOPE_INVALID") }
    }
    private func action(_ work: () async throws -> Void) async {
        guard !busy, available, api != nil else { return }
        importStatusGeneration = UUID()
        busy = true; failure = nil; notice = ""
        defer { busy = false }
        do { try await work() }
        catch is CancellationError {
            notice = "파일 준비·전송을 정지했습니다. 저장한 요청과 이미 받은 원본은 유지됩니다."
        }
        catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure((error as? BridgeFailure)?.code ?? "CONNECTION_FAILED")
            if failure?.status == 401 { session = nil }
        }
    }
    func connect(_ credentials: LocalAuthoringSession) async {
        await action {
            guard let api else { return }
            let authenticated = try await api.connect(credentials)
            if let editingUser, dirty && editingUser != authenticated.userId { throw AuthoringFailure("DRAFT_ACCOUNT_MISMATCH", status: 403) }
            session = authenticated
            try await restoreImport()
            artists = try await api.artists().items
            let page = try await api.artworks(query: query); artworks = page.items; cursor = page.nextCursor
            notice = "이 Mac의 작업 공간에 연결했습니다. 작품은 이 Mac에 저장됩니다."
        }
    }
    func login() async {
        await action {
            guard let api else { return }
            defer { password = "" }
            let authenticated = try await api.login(subject: subject, password: password)
            if let editingUser, editingUser != authenticated.userId {
                if dirty {
                    try await api.logout()
                    throw AuthoringFailure("DRAFT_ACCOUNT_MISMATCH", status: 403)
                }
                editing = false; selected = nil; draft = ArtworkMetadata(); original = draft; creationYearText = ""
            }
            session = authenticated
            artists = try await api.artists().items
            let page = try await api.artworks(query: query)
            artworks = page.items; cursor = page.nextCursor
            notice = "로그인했습니다. 같은 계정으로 작품과 전시를 제작합니다."
        }
    }
    func refresh(more: Bool = false) async {
        await action {
            guard let api else { return }
            let page = try await api.artworks(query: query, cursor: more ? cursor : nil)
            if more { artworks += page.items.filter { item in !artworks.contains(where: { $0.id == item.id }) } }
            else { artworks = page.items }
            cursor = page.nextCursor
            artists = try await api.artists().items
        }
    }
    func open(_ id: String) async {
        await action {
            guard let api else { return }
            let value = try await api.artwork(id)
            if transfer?.intent == nil { importFile = nil }
            approvalAcknowledged = false
            selected = value; draft = value.metadata ?? ArtworkMetadata(); original = draft; creationYearText = draft.creationYear.map(String.init) ?? ""
            assetId = value.approvedAssetId ?? value.assets.first(where: { $0.state == "approved" })?.id ?? ""
            artistId = value.artistId; editing = true; editingUser = session?.userId
            if !artists.contains(where: { $0.id == value.artistId }) { artists.append(try await api.artist(value.artistId)) }
        }
    }
    func newArtwork() { assetId = ""; if transfer?.intent == nil { importFile = nil }; approvalAcknowledged = false; editingUser = session?.userId; creationYearText = ""; selected = nil; draft = ArtworkMetadata(); original = draft; artistId = artists.first(where: { !$0.archived })?.id ?? ""; editing = true; failure = nil; notice = "" }
    func save() async {
        await action {
            guard let api else { return }
            if creationYearText.isEmpty { draft.creationYear = nil }
            else if let year = Int(creationYearText), (1...9999).contains(year) { draft.creationYear = year }
            else { throw AuthoringFailure("CREATION_YEAR_INVALID", status: 400) }
            let value = try await api.saveArtwork(id: selected?.id, revision: selected?.revision, artistId: artistId, metadata: draft)
            selected = value; draft = value.metadata ?? draft; original = draft
            notice = "작품 정보를 저장했습니다. 파일과 권리 검토·승인은 별도로 진행합니다."
            let page = try await api.artworks(query: query); artworks = page.items; cursor = page.nextCursor
        }
    }
    func createArtist() async {
        await action {
            guard let api else { return }
            let artist = try await api.createArtist(name: artistName, bio: artistBio)
            if !artists.contains(where: { $0.id == artist.id }) { artists.append(artist) }
            artistId = artist.id; artistName = ""; artistBio = ""
            notice = "작가 정보를 저장했습니다."
            // Keep the acknowledged creation even if a subsequent list refresh
            // fails; retrying the form must not duplicate a known saved artist.
            artists = try await api.artists().items
        }
    }
}

extension AuthoringModel {
    func bindImportStore(read: @escaping () async throws -> NativeUploadState, save: @escaping (NativeUploadState) async throws -> NativeUploadState) {
        readTransfer = read; writeTransfer = save
    }
    private func restoreImport() async throws {
        guard let readTransfer, let session else { return }
        let state = try await readTransfer()
        guard state.tenantId.lowercased() == session.tenantId.lowercased(), state.principalId.lowercased() == session.userId.lowercased() else { throw AuthoringFailure("IMPORT_SCOPE_CHANGED") }
        if transfer == nil, let intent = state.intent { importScale = intent.scaleMeters }
        transfer = state
        importJob = nil
        if let intent = state.intent, intent.modelDerivation != nil, let api {
            try importTransfer.restoreModel(intent: intent, jobID: state.jobId, transport: api)
        }
        if let id = state.jobId, let api {
            let observed = try await api.importJob(id)
            if let intent = state.intent { try importTransfer.observe(observed, intent: intent) }
            importJob = observed
        }
        scheduleImportPoll()
    }
    private func persistImport(_ state: NativeUploadState) async throws {
        guard let writeTransfer else { throw AuthoringFailure("IMPORT_STORE_REQUIRED") }
        transfer = try await writeTransfer(state)
    }
    func chooseImportFile() async {
        guard !busy, available, session != nil else { return }
        let panel = NSOpenPanel()
        panel.title = "작품 파일 가져오기"
        panel.message = jpegImportsEnabled ? "GLB·PNG·JPEG 파일을 선택하세요. 최대 32 MiB이며 JPEG 원본을 유지하고 전시용 PNG를 준비합니다." : "GLB 또는 PNG 파일을 선택하세요. 최대 32 MiB를 지원합니다."
        panel.canChooseDirectories = false; panel.canChooseFiles = true; panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [UTType(filenameExtension: "glb") ?? .data, .png] + (jpegImportsEnabled ? [.jpeg] : [])
        guard let window = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.identifier?.rawValue == "manager" }) else { return }
        let response = await withCheckedContinuation { continuation in
            panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
        }
        guard response == .OK, let url = panel.url else { return }
        await action {
            let read = Task { try await ImportFileSnapshot.load(url, allowJPEG: jpegImportsEnabled) }
            importRead = read
            defer { importRead = nil }
            let snapshot = try await withTaskCancellationHandler { try await read.value } onCancel: { read.cancel() }
            try Task.checkCancellation()
            if let intent = transfer?.intent, !snapshot.matches(intent) { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            importFile = snapshot
            if draft.title.isEmpty && selected == nil { draft.title = url.deletingPathExtension().lastPathComponent }
            notice = "파일을 선택했습니다. 작품 정보와 권리를 저장한 뒤 파일 검사를 시작하세요."
        }
    }
    func uploadImportFile() async {
        await action {
            guard let api, let selected, !metadataDirty, let file = importFile, let state = transfer else { throw AuthoringFailure("IMPORT_FILE_REQUIRED") }
            let generation = importStatusGeneration, currentScope = scope, manifest = runtimeManifestSHA256
            let rights = draft.rights, scale = importScale
            let requestKey = state.intent?.idempotencyKey ?? UUID().uuidString.lowercased()
            let derived: NativePreparedJPEG?
            if file.mime == "image/jpeg" {
                guard jpegImportsEnabled else { throw AuthoringFailure("JPEG_IMPORT_UNAVAILABLE") }
                notice = "JPEG 파일을 준비하고 있습니다. 선택한 원본은 변경하지 않습니다."
                derived = try await prepareJPEG(file, operationID: requestKey)
            } else { derived = nil }
            guard generation == importStatusGeneration, currentScope == scope, available,
                  file.mime != "image/jpeg" || (jpegImportsEnabled && manifest == runtimeManifestSHA256),
                  self.selected?.id == selected.id, self.selected?.revision == selected.revision,
                  !metadataDirty, draft.rights == rights, importScale == scale,
                  transfer?.revision == state.revision, transfer?.workspaceId == state.workspaceId,
                  importFile?.sha256 == file.sha256 else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
            let modelDerivation: NativeModelDerivationIntent?
            if let previous = state.intent {
                modelDerivation = previous.modelDerivation // same server generation on recovery
            } else if file.mime == "model/gltf-binary", modelDerivationEnabled {
                modelDerivation = NativeModelDerivationIntent(operationID: requestKey, generation: generation.uuidString.lowercased(), artworkRevision: selected.revision)
            } else { modelDerivation = nil }
            let intent = NativeImportIntent(artworkId: selected.id, idempotencyKey: requestKey, mime: file.mime,
                sha256: file.sha256, bytes: file.data.count, scaleMeters: scale, rights: rights, derived: derived?.derived, modelDerivation: modelDerivation)
            if let previous = state.intent, previous != intent { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            try await continueImport(intent: intent, file: file, derived: derived, api: api)
            approvalAcknowledged = false
            notice = "파일 검사 요청을 확인했습니다. 현재 상태와 작품 승인은 아래에서 확인하세요."
            scheduleImportPoll()
        }
    }
    func recoverImport() async {
        await action {
            guard let api, let intent = transfer?.intent else { return }
            try await continueImport(intent: intent, file: nil, api: api)
            notice = "같은 파일 검사 요청의 현재 상태를 확인했습니다."
            scheduleImportPoll()
        }
    }
    private func continueImport(intent: NativeImportIntent, file: ImportFileSnapshot?, derived: NativePreparedJPEG? = nil, api: AuthoringAPI) async throws {
        guard intent.modelDerivation == nil || modelDerivationEnabled else { throw AuthoringFailure("IMPORT_MODEL_UNAVAILABLE") }
        let generation = importStatusGeneration, currentScope = scope, manifest = runtimeManifestSHA256
        let tenant = session?.tenantId, principal = session?.userId
        @MainActor func current() -> Bool {
            generation == importStatusGeneration && currentScope == scope && available
                && (intent.mime != "image/jpeg" || (jpegImportsEnabled && manifest == runtimeManifestSHA256))
                && (intent.modelDerivation == nil || (modelDerivationEnabled && manifest == runtimeManifestSHA256))
                && session?.tenantId == tenant && session?.userId == principal
                && (transfer?.intent == nil || transfer?.intent == intent)
        }
        let result = try await importTransfer.run(intent: intent, jobID: transfer?.jobId, file: file, derived: derived, transport: api,
            persistIntent: { [self] in
                guard current(), var state = transfer else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
                if state.intent == nil {
                    guard let file else { throw AuthoringFailure("IMPORT_FILE_REQUIRED") }
                    state.intent = intent; state.fileName = file.name; state.jobId = nil
                    try await persistImport(state) // before POST, including uncertain responses
                }
            }, persistJob: { [self] job in
                guard current(), var state = transfer, state.intent == intent else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
                state.jobId = job.id
                try await persistImport(state)
                guard current() else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
                importJob = job
            }, isCurrent: current)
        try Task.checkCancellation()
        guard current(), transfer?.intent == intent else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
        importJob = result
    }
    private func prepareJPEG(_ original: ImportFileSnapshot, operationID: String) async throws -> NativePreparedJPEG {
        guard jpegImportsEnabled else { throw AuthoringFailure("JPEG_IMPORT_UNAVAILABLE") }
        let generation = importStatusGeneration, currentScope = scope, manifest = runtimeManifestSHA256
        let tenant = session?.tenantId, principal = session?.userId, workspace = transfer?.workspaceId
        let ticket = NativeImagePreparationScope(operationID: operationID, generation: generation.uuidString.lowercased(), originalSHA256: original.sha256)
        let owned = Task {
            try await NativePreparedJPEG.prepare(original: original, operationID: ticket.operationID, generation: ticket.generation,
                isCurrent: { ticket.isCurrent(operationID: operationID, generation: generation.uuidString.lowercased(), originalSHA256: original.sha256) })
        }
        imagePreparationScope = ticket; imagePreparation = owned
        defer {
            ticket.invalidate()
            if imagePreparationScope === ticket { imagePreparationScope = nil; imagePreparation = nil }
        }
        @MainActor func current() -> Bool {
            ticket.isCurrent(operationID: operationID, generation: generation.uuidString.lowercased(), originalSHA256: original.sha256)
                && generation == importStatusGeneration && currentScope == scope && available
                && jpegImportsEnabled && manifest == runtimeManifestSHA256
                && tenant == session?.tenantId && principal == session?.userId && workspace == transfer?.workspaceId
        }
        do {
            let result = try await withTaskCancellationHandler { try await owned.value } onCancel: { ticket.invalidate(); owned.cancel() }
            try Task.checkCancellation()
            guard current(), result.originalSHA256 == original.sha256 else { throw CancellationError() }
            return result
        } catch {
            if Task.isCancelled || owned.isCancelled || !current() { throw CancellationError() }
            if let failure = error as? NativeImagePreparation.Failure {
                switch failure {
                case .cancelledOrStale: throw CancellationError()
                case .unsupportedJPEG, .unsupportedPixels: throw AuthoringFailure("JPEG_PROFILE_UNSUPPORTED")
                case .limits: throw AuthoringFailure("JPEG_LIMIT")
                default: throw AuthoringFailure("JPEG_PREPARATION_FAILED")
                }
            }
            throw error
        }
    }
    func cancelImportAndWait() async {
        importStatusGeneration = UUID()
        imagePreparationScope?.invalidate()
        if let owned = imagePreparation { owned.cancel(); _ = await owned.result }
        if let read = importRead { read.cancel(); _ = await read.result }
        await importTransfer.cancelAndWait()
        if modelWorkerPending { failure = AuthoringFailure(importTransfer.modelDrainFailure ?? "IMPORT_DRAIN_PENDING") }
    }
    /// Only the owning native helper's real stopped/no-children boundary calls
    /// this fallback. Native local-runtime close awaits its import worker.
    func ownedImportRuntimeDidStop(manifestSHA256: String) async {
        guard runtimeManifestSHA256 == manifestSHA256 else { return }
        await importTransfer.ownedRuntimeDidStop()
        if failure?.code == "IMPORT_DRAIN_PENDING" { failure = nil }
    }
    func changeImport(_ change: NativeImportAction) async {
        await action {
            guard let api, let job = importJob else { return }
            if change == .cancel, transfer?.intent?.modelDerivation != nil {
                await importTransfer.cancelAndWait()
                guard !modelWorkerPending else { throw AuthoringFailure("IMPORT_DRAIN_PENDING") }
                let observed = try await api.importJob(job.id)
                if let intent = transfer?.intent { try importTransfer.observe(observed, intent: intent) }
                importJob = observed
            } else {
                if change == .retry, let intent = transfer?.intent, intent.modelDerivation != nil {
                    guard modelDerivationEnabled else { throw AuthoringFailure("IMPORT_MODEL_UNAVAILABLE") }
                    guard job.canRetry, job.modelDerivation?.workerDrained == true else { throw AuthoringFailure("IMPORT_DRAIN_PENDING") }
                    try importTransfer.restoreModel(intent: intent, jobID: job.id, transport: api)
                }
                let observed = try await api.importAction(job.id, action: change)
                if let intent = transfer?.intent { try importTransfer.observe(observed, intent: intent) }
                importJob = observed
            }
            scheduleImportPoll()
        }
    }
    func refreshImport() async {
        await action {
            guard let api, let id = transfer?.jobId else { return }
            let observed = try await api.importJob(id)
            if let intent = transfer?.intent { try importTransfer.observe(observed, intent: intent) }
            importJob = observed
            if importJob?.state == "approved", let selected, selected.id == transfer?.intent?.artworkId {
                let fresh = try await api.artwork(selected.id)
                if fresh.revision == selected.revision { self.selected = fresh; if let asset = importJob?.assetId { assetId = asset } }
            }
            scheduleImportPoll()
        }
    }
    func clearImport() async {
        await action {
            guard importJob?.terminal == true, !modelWorkerPending,
                  importJob?.modelDerivation?.workerDrained != false, var state = transfer else { throw AuthoringFailure("IMPORT_ACTIVE") }
            state.intent = nil; state.jobId = nil; state.fileName = nil
            try await persistImport(state)
            importJob = nil; importFile = nil; approvalAcknowledged = false
        }
    }
    func approveImportedArtwork() async {
        await action {
            guard let api, let selected, !metadataDirty, approvalAcknowledged,
                  selected.assets.contains(where: { $0.id == assetId && $0.state == "approved" }) else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
            if transfer?.intent?.derived != nil, transfer?.intent?.artworkId == selected.id, importJob?.assetId == assetId {
                guard jpegOriginalStored else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            }
            let approved = try await api.approveArtwork(selected.id, revision: selected.revision, assetId: assetId)
            self.selected = approved
            approvalAcknowledged = false
            notice = "파일과 저장한 정보·권리를 승인했습니다. 전시에서 사용할 준비가 되었습니다."
        }
    }
    private func scheduleImportPoll() {
        guard importPoll == nil, shouldPollImport else { return }
        let owner = UUID()
        importPollID = owner
        importPoll = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(1)) } catch { break }
                guard let self, self.available, self.session != nil else { break }
                if self.busy { continue }
                await self.pollImportStatus()
                if !self.shouldPollImport || (self.failure != nil && self.failure?.code != "IMPORT_DRAIN_PENDING") { break }
            }
            if self?.importPollID == owner { self?.importPoll = nil; self?.importPollID = nil }
        }
    }
    private func pollImportStatus() async {
        guard !busy, !pollingImportStatus, available, let api, let session,
              let id = transfer?.jobId else { return }
        pollingImportStatus = true
        defer { pollingImportStatus = false }
        let generation = importStatusGeneration
        let currentScope = scope
        @MainActor func current() -> Bool {
            generation == importStatusGeneration && currentScope == scope && !busy && available
                && transfer?.jobId == id && self.session?.tenantId == session.tenantId
                && self.session?.userId == session.userId && !Task.isCancelled
        }
        do {
            let observed = try await api.importJob(id)
            guard current() else { return }
            if let intent = transfer?.intent { try importTransfer.observe(observed, intent: intent) }
            importJob = observed
            if !modelWorkerPending, failure?.code == "IMPORT_DRAIN_PENDING" { failure = nil }
            if observed.state == "approved", let selected, selected.id == transfer?.intent?.artworkId {
                let fresh = try await api.artwork(selected.id)
                guard current(), self.selected?.id == selected.id,
                      self.selected?.revision == selected.revision, fresh.revision == selected.revision else { return }
                self.selected = fresh
                if let asset = observed.assetId { assetId = asset }
            }
        } catch {
            guard current() else { return }
            failure = error as? AuthoringFailure ?? AuthoringFailure((error as? BridgeFailure)?.code ?? "CONNECTION_FAILED")
            if failure?.status == 401 { self.session = nil }
        }
    }
    private var shouldPollImport: Bool {
        guard let job = importJob else { return false }
        return ["queued", "processing"].contains(job.state) || (job.state == "failed" && job.attempts < 5 && !job.canRetry)
            || job.modelDerivation?.workerDrained == false || (job.state == "approved" && job.modelDerivation?.status == "pending")
    }
}

struct NativeAuthoringView: View {
    @ObservedObject var model: AuthoringModel
    let configuration: NativeConfiguration?
    let ready: Bool
    let mode: Mode
    let connect: () async -> Void
    @State private var discardPrompt = false
    @State private var pendingId: String?
    @State private var pendingNew = false
    private var scope: String { "\(configuration?.tenantId ?? ""):\(configuration?.ports.web ?? 0):\(configuration?.manifestSha256 ?? ""):\(ready)" }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if !ready { Text("준비 화면에서 로컬 제작을 시작하세요.").foregroundStyle(.secondary) }
            if model.session == nil {
                if configuration?.kind == "personal" {
                    Text("이 Mac에 저장된 작품과 전시를 제작합니다.").foregroundStyle(.secondary)
                    Button("작업 공간 다시 연결") { Task { await connect() } }.disabled(!ready || model.busy)
                } else {
                    DisclosureGroup("기존 계정 연결") {
                        Form {
                            TextField("기존 계정", text: $model.subject).textContentType(.username)
                            SecureField("비밀번호", text: $model.password).textContentType(.password)
                            Button("기존 계정에 연결") { Task { await model.login() } }
                                .disabled(!ready || model.busy || model.subject.isEmpty || model.password.isEmpty)
                        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true)
                    }
                }
            } else {
                HStack {
                    TextField("작품 검색", text: $model.query).onSubmit { Task { await model.refresh() } }
                    Button("검색") { Task { await model.refresh() } }
                    Button("새 작품", systemImage: "plus") { select(nil) }
                }.disabled(model.busy || !ready)
                if model.artworks.isEmpty { Text("등록한 작품이 없습니다. 작가 정보를 추가하고 새 작품을 만드세요.").foregroundStyle(.secondary) }
                List(selection: Binding<String?>(get: { model.selected?.id }, set: { if let id = $0 { select(id) } })) {
                    ForEach(model.artworks) { artwork in
                        HStack { Text(artwork.title); Spacer(); Text("revision \(artwork.revision)").foregroundStyle(.secondary) }.tag(artwork.id)
                    }
                }.frame(minHeight: 140, maxHeight: 240).disabled(model.busy || !ready)
                if model.cursor != nil { Button("작품 더 보기") { Task { await model.refresh(more: true) } }.disabled(model.busy || !ready) }
                DisclosureGroup("작가 추가") {
                    TextField("작가 이름", text: $model.artistName)
                    TextField("작가 소개", text: $model.artistBio, axis: .vertical)
                    Button("작가 추가") { Task { await model.createArtist() } }.disabled(model.artistName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy || !ready)
                }
                if model.editing { NativeImportControls(model: model, ready: ready, mode: mode, openArtwork: { select($0) }); editor }
            }
            if model.busy { ProgressView("제작 내용을 확인하고 있습니다…") }
            if let failure = model.failure {
                Text(NativeJPEGImportStatus.message(for: failure.code) ?? failure.localizedDescription).foregroundStyle(.primary)
                DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) }
                if configuration?.kind == "personal", model.session != nil {
                    Button("작업 공간 다시 연결") { Task { await connect() } }.disabled(!ready || model.busy)
                }
            }
            if !model.notice.isEmpty { Text(model.notice).foregroundStyle(.secondary) }
        }
        .onChange(of: model.draft) { _, _ in model.approvalAcknowledged = false }
        .onChange(of: model.creationYearText) { _, _ in model.approvalAcknowledged = false }
        .task(id: scope) { model.configure(configuration, ready: ready); if ready && configuration?.kind == "personal" && model.session == nil { await connect() } }
        .confirmationDialog("저장하지 않은 입력을 버릴까요?", isPresented: $discardPrompt, titleVisibility: .visible) {
            Button("입력 버리기", role: .destructive) { performSelection() }
            Button("계속 편집", role: .cancel) {}
        }
    }
    private func select(_ id: String?) {
        pendingId = id; pendingNew = id == nil
        if model.dirty { discardPrompt = true } else { performSelection() }
    }
    private func performSelection() {
        if pendingNew { model.newArtwork() }
        else if let pendingId { Task { await model.open(pendingId) } }
    }
    private var editor: some View {
        Form {
            Section("작품 정보") {
                Picker("작가", selection: $model.artistId) {
                    Text("작가 선택").tag("")
                    ForEach(model.artists) { Text($0.name).tag($0.id) }
                }.disabled(model.selected != nil)
                TextField("작품 제목", text: $model.draft.title)
                TextField("설명", text: $model.draft.description, axis: .vertical).lineLimit(3...8)
                TextField("재료·매체", text: Binding(get: { model.draft.medium ?? "" }, set: { model.draft.medium = $0.isEmpty ? nil : $0 }))
                TextField("제작 연도 (선택)", text: $model.creationYearText)
                TextField("가로 (m)", value: $model.draft.dimensions.width, format: .number)
                TextField("세로 (m)", value: $model.draft.dimensions.height, format: .number)
                TextField("깊이 (m, 선택)", value: $model.draft.dimensions.depth, format: .number)
            }
            Section("권리와 표시") {
                TextField("권리자", text: $model.draft.rights.holder)
                Picker("권리 근거", selection: $model.draft.rights.ownership) {
                    Text("내가 권리자").tag("owner"); Text("허가를 받음").tag("authorized-licensee")
                }
                TextField("표시할 크레딧 (선택)", text: $model.draft.rights.creditLine)
                TextField("라이선스 ID", text: Binding(get: { model.draft.rights.licenseId ?? "" }, set: { model.draft.rights.licenseId = $0.isEmpty ? nil : $0 }))
                TextField("허용 조건", text: Binding(get: { model.draft.rights.licenseText ?? "" }, set: { model.draft.rights.licenseText = $0.isEmpty ? nil : $0 }), axis: .vertical)
                Toggle("전시 허용", isOn: $model.draft.rights.permissions.display)
                Toggle("다운로드 허용", isOn: $model.draft.rights.permissions.download)
                Toggle("OEX 내보내기 허용", isOn: $model.draft.rights.permissions.export)
                Toggle("상업적 사용 허용", isOn: $model.draft.rights.permissions.commercial)
                TextField("권리 시작일 (ISO 8601, 선택)", text: Binding(get: { model.draft.rights.validFrom ?? "" }, set: { model.draft.rights.validFrom = $0.isEmpty ? nil : $0 }))
                TextField("권리 만료일 (ISO 8601, 선택)", text: Binding(get: { model.draft.rights.expiresAt ?? "" }, set: { model.draft.rights.expiresAt = $0.isEmpty ? nil : $0 }))
            }
            Section("제작 출처") {
                Picker("출처", selection: $model.draft.provenance.source) {
                    Text("직접 제작").tag("human-authored"); Text("AI 도움").tag("ai-assisted"); Text("AI 생성").tag("ai-generated"); Text("합성 자료").tag("synthetic")
                }
                Picker("원래 단위", selection: $model.draft.provenance.sourceUnits) { Text("m").tag("m"); Text("cm").tag("cm"); Text("mm").tag("mm") }
                Toggle("크기를 조정했음", isOn: $model.draft.provenance.scaleApplied)
                TextField("출처 설명", text: $model.draft.provenance.notes, axis: .vertical)
            }
            if let selected = model.selected {
                Text("저장된 revision \(selected.revision) · 파일 \(selected.assets.count)개")
                Text("변경하면 공개 승인이 해제되며 새 검토가 필요합니다.").foregroundStyle(.secondary)
            }
            Button("작품 정보 저장") { Task { await model.save() } }.buttonStyle(.borderedProminent)
                .disabled(model.draft.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.artistId.isEmpty || model.draft.rights.holder.isEmpty)
        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true).disabled(model.busy || !ready)
    }
}
