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
    struct IntakeRow: Identifiable {
        struct Result {
            let artworkID: String
            let artworkRevision: Int
            let jobID: String
            let state: String
        }
        let id = UUID()
        let url: URL
        var name: String { url.lastPathComponent }
        var error: String?
        var result: Result?
    }
    @Published private(set) var intakeRows: [IntakeRow] = []
    @Published private(set) var activeIntakeID: UUID?
    @Published private(set) var intakeWritesAvailable = false
    private var intakeTarget: (artworkID: String, revision: Int)?
    private var intakeWorkspaceCheck: (() async throws -> Bool)?
    var hasWaitingIntake: Bool { intakeRows.contains { $0.result == nil && $0.id != activeIntakeID } }
    private var requiredModelPreparationEnabled: Bool {
        session?.modelPreparationProfile == NativeModelPreparationIntent.profile
    }
    private var largeModelPreparationEnabled: Bool {
        api?.largeModelRuntimeQualified == true && requiredModelPreparationEnabled
            && session?.nativeLargeModelPreparationProfile == NativeModelPreparationIntent.largeProfile
    }
    private var modelIntakeEnabled: Bool { modelDerivationEnabled || requiredModelPreparationEnabled }
    var canSelectIntake: Bool {
        intakeWritesAvailable && (modelIntakeEnabled || iccImportAdmitted) && available && session != nil && !busy
            && compactTransfer == nil
    }
    var canOpenIntakeRow: Bool {
        canSelectIntake && !hasUnknownSourceUnits && activeIntakeID == nil && selected?.archived == false
            && !metadataDirty && importFile == nil && transfer?.intent == nil
    }
    var hasUnknownSourceUnits: Bool { draft.provenance.sourceUnits == "unknown" }
    var intakeDrainPending: Bool {
        guard let intent = transfer?.intent, let job = importJob else { return false }
        return (job.terminal || job.state == "failed") && requiresIntakeDrain(intent, job: job)
            && !job.confirmsIntakeDrain(for: intent)
    }
    private func requiresIntakeDrain(_ intent: NativeImportIntent, job: NativeImportJob) -> Bool {
        // A restarted queue loses its in-memory row, but keeps this intent and
        // the runtime's drain contract. Unknown older single-file runtimes
        // retain their existing behavior.
        activeIntakeID != nil || intent.modelDerivation != nil || intent.modelPreparation != nil || intent.coarsePreparation != nil
            || intent.productImagePreparation != nil
            || (intent.mime == "image/png" && (iccImportAdmitted || (job.workerDrainVersion ?? 0) > 0))
    }
    @Published private(set) var compactJob: NativeCompactModelVariantJob?
    var compactTransfer: NativeCompactModelVariantTransfer? { transfer?.compactVariant }
    var compactAvailable: Bool {
        session?.compactModelVariantProfile == NativeCompactModelVariantRequest.profile
    }
    private var approvedFullModel: NativeArtwork.Asset? {
        guard let selected, !selected.archived, selected.approvedRevision == selected.revision else { return nil }
        return selected.assets.first { $0.id == selected.approvedAssetId && $0.state == "approved" && $0.mime == "model/gltf-binary" }
    }
    var canPrepareCompactVariant: Bool {
        compactAvailable && transfer != nil && transfer?.intent == nil && compactTransfer == nil
            && importFile == nil && activeIntakeID == nil && !metadataDirty && approvedFullModel != nil
    }
    var compactApprovalVisible: Bool {
        guard let compactTransfer, let compactJob, let selected, selected.id == compactTransfer.artworkId else { return false }
        return compactJob.modelPreparation.status == .ready && compactJob.modelPreparation.workerDrained
            && selected.approvedRevision == compactTransfer.request.revision
    }

    @Published var importScale = 1.0
    @Published var approvalAcknowledged = false
    @Published var assetId = ""
    private var readTransfer: (() async throws -> NativeUploadState)?
    private var writeTransfer: ((NativeUploadState) async throws -> NativeUploadState)?
    @Published private var hayesSessionReady = false
    @Published private(set) var hayesReceipt: NativeHayesReceipt?
    private var executeHayes: ((String, String, Int) async throws -> NativeHayesReceipt)?
    private var recoverHayes: ((String, Int) async throws -> NativeHayesReceipt)?
    var hayesOriginalAvailable: Bool {
        guard hayesSessionReady, api?.hayesOriginalRuntimeQualified == true, let selected, hasUnknownSourceUnits else {
            return false
        }
        return hasHayesIntent || NativeHayesOriginalIntent.matchesSourceArtwork(selected)
    }
    var hasHayesIntent: Bool {
        guard let selected, let intent = transfer?.intent else { return false }
        return intent.artworkId == selected.id && intent.hayesOriginal != nil
    }
    private var usesHayesOriginal: Bool { hasHayesIntent || hayesOriginalAvailable }
    var modelOriginalInspectionAvailable: Bool {
        requiredModelPreparationEnabled && session?.modelOriginalInspectionProfile == NativeModelOriginalInspection.profile
    }
    var ordinaryUnknownModel: Bool {
        hasUnknownSourceUnits && modelOriginalInspectionAvailable && !usesHayesOriginal
    }
    var canInspectUnknownModel: Bool {
        modelOriginalInspectionAvailable && !usesHayesOriginal && transfer != nil && transfer?.intent == nil
            && compactTransfer == nil && activeIntakeID == nil && selected?.archived != true
            && (selected == nil || selected?.approvedRevision != selected?.revision)
    }
    @Published private(set) var modelOriginalInspection: NativeModelOriginalInspection?
    @Published var provisionalHeight: Double?
    var inspectedReferencePending: Bool {
        modelOriginalInspection != nil && (!hasUnknownSourceUnits || draft.displayReference?.assetSha256 != importFile?.sha256)
    }

    func applyInspectedDisplayReference() {
        guard canInspectUnknownModel, let modelOriginalInspection, let provisionalHeight else { return }
        do {
            draft = try modelOriginalInspection.applying(to: draft, height: provisionalHeight)
            importScale = 1
            failure = nil
            notice = "임시 표시 높이를 설정했습니다. 작품 정보와 권리를 저장하세요."
        } catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
        }
    }
    private let importTransfer = NativeImportTransfer()
    private var importRead: Task<ImportFileSnapshot, Error>?
    private var imagePreparation: Task<NativePreparedJPEG, Error>?
    private var imagePreparationScope: NativeImagePreparationScope?
    // Only explicitly qualified runtime packets expose original/derived JPEG
    // custody. Other manifests retain the existing PNG/GLB chooser and contract.
    @Published private(set) var jpegImportsEnabled = false
    @Published private(set) var modelDerivationEnabled = false
    var modelWorkerPending: Bool { importTransfer.hasPendingModelWorker }
    var modelOutputsCurrent: Bool {
        guard let intent = transfer?.intent, let job = importJob, let selected else { return false }
        return job.modelDerivation?.currentOutputs(in: selected, intent: intent, job: job) == true
    }
    var importFormats: String { (jpegImportsEnabled || freshImagePreparationAvailable) ? "GLB·PNG·JPEG" : "GLB·PNG" }
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
    private var iccImportAdmitted: Bool {
        available && session != nil && api?.admitsICCJPEG == true
            && NativeJPEGImportRuntime.supportsICC(manifestSHA256: runtimeManifestSHA256, ready: available)
    }
    var exhibitionAPI: AuthoringAPI? { session == nil ? nil : api }
    private var scope: String?
    private var runtimeManifestSHA256: String?
    private var editingUser: String?
    private var runtimeJobID: String?
    private var available = false
    var metadataDirty: Bool { editing && (draft != original || selected == nil || creationYearText != (original.creationYear.map(String.init) ?? "")) }
    var intakeMetadataHeld: Bool {
        transfer?.holdsRequiredModelMetadata(for: selected?.id) == true
            || (hasFreshImageTransfer && transfer?.intent?.artworkId == selected?.id)
    }

    var dirty: Bool { metadataDirty || (importFile != nil && transfer?.intent == nil) }

    @discardableResult
    func configure(_ configuration: NativeConfiguration?, ready: Bool, runtimeJobID: String?) -> Bool {
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
            return false
        }
        available = ready
        if self.runtimeJobID != runtimeJobID {
            // Starting a new local runtime revokes the previous session even
            // when the tenant, port and runtime manifest stay the same.
            self.runtimeJobID = runtimeJobID
            hayesSessionReady = false; hayesReceipt = nil; executeHayes = nil; recoverHayes = nil
            session = nil
            importStatusGeneration = UUID()
        }
        let previousManifest = runtimeManifestSHA256
        runtimeManifestSHA256 = configuration?.manifestSha256
        modelDerivationEnabled = configuration?.kind == "personal" && NativeModelDerivationProfile.supports(manifestSHA256: runtimeManifestSHA256, ready: ready)
        jpegImportsEnabled = NativeJPEGImportRuntime.supports(manifestSHA256: runtimeManifestSHA256, ready: ready)
        if !ready || previousManifest != runtimeManifestSHA256 {
            importStatusGeneration = UUID()
            imagePreparationScope?.invalidate(); imagePreparation?.cancel()
        }
        guard let configuration, let tenant = configuration.tenantId else { return false }
        let next = "\(tenant):\(configuration.ports.web):\(configuration.manifestSha256)"
        guard next != scope else { return available && api != nil }
        imagePreparationScope?.invalidate(); imagePreparation?.cancel()
        importStatusGeneration = UUID()
        guard !busy, !dirty, !hasWaitingIntake else { api = nil; session = nil; failure = AuthoringFailure("LOCAL_SCOPE_CHANGED"); return false }
        importPoll?.cancel(); importPoll = nil; importPollID = nil
        transfer = nil; importJob = nil; importFile = nil; compactJob = nil; modelOriginalInspection = nil; provisionalHeight = nil; assetId = ""
        intakeRows = []; activeIntakeID = nil; intakeTarget = nil; intakeWritesAvailable = false; intakeWorkspaceCheck = nil
        readTransfer = nil; writeTransfer = nil
        scope = next; session = nil; artworks = []; artists = []; selected = nil; editing = false
        subject = configuration.subject ?? ""
        do { api = try AuthoringAPI(port: configuration.ports.web, tenantId: tenant, nativeRuntimeManifestSHA256: configuration.manifestSha256) }
        catch { failure = AuthoringFailure("LOCAL_SCOPE_INVALID"); return false }
        return available
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
            let jobID = runtimeJobID
            let authenticated = try await api.connect(credentials)
            guard available, self.api === api, runtimeJobID == jobID else { throw AuthoringFailure("LOCAL_SCOPE_CHANGED") }
            if let editingUser, dirty && editingUser != authenticated.userId { throw AuthoringFailure("DRAFT_ACCOUNT_MISMATCH", status: 403) }
            session = authenticated
            try await restoreImport()
            artists = try await api.artists().items
            let page = try await api.artworks(query: query); artworks = page.items; cursor = page.nextCursor
            notice = "이 Mac의 작업 공간에 연결했습니다. 작품은 이 Mac에 저장됩니다."
        }
    }
    func waitForConnectionTurn(runtimeJobID: String?) async -> Bool {
        while busy && available && self.runtimeJobID == runtimeJobID {
            do { try await Task.sleep(for: .milliseconds(50)) }
            catch { return false }
        }
        return available && self.runtimeJobID == runtimeJobID && !Task.isCancelled
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
            modelOriginalInspection = nil; provisionalHeight = nil
            approvalAcknowledged = false
            selected = value; draft = value.metadata ?? ArtworkMetadata(); original = draft; creationYearText = draft.creationYear.map(String.init) ?? ""
            assetId = value.approvedAssetId
                ?? (transfer?.intent?.artworkId == value.id && hasHayesIntent ? importJob?.assetId : nil)
                ?? value.assets.first(where: { $0.state == "approved" })?.id ?? ""
            artistId = value.artistId; editing = true; editingUser = session?.userId
            if !artists.contains(where: { $0.id == value.artistId }) { artists.append(try await api.artist(value.artistId)) }
        }
    }
    func newArtwork() { assetId = ""; if transfer?.intent == nil { importFile = nil }; modelOriginalInspection = nil; provisionalHeight = nil; approvalAcknowledged = false; editingUser = session?.userId; creationYearText = ""; selected = nil; draft = ArtworkMetadata(); original = draft; artistId = artists.first(where: { !$0.archived })?.id ?? ""; editing = true; failure = nil; notice = "" }
    func save() async {
        await action {
            guard let api else { return }
            guard !intakeMetadataHeld else { throw AuthoringFailure(hasFreshImageTransfer ? "IMPORT_ACTIVE" : "IMPORT_MODEL_METADATA_HELD") }
            guard !inspectedReferencePending else { throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID") }
            if let selected, let intent = transfer?.intent, intent.derived?.recipe.colourConversion != nil,
               selected.id.lowercased() == intent.artworkId.lowercased() {
                throw AuthoringFailure(iccImportAdmitted ? "IMPORT_ACTIVE" : "IMPORT_ICC_UNAVAILABLE")
            }
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
    func bindIntakeWorkspace(available: Bool, recheck: @escaping () async throws -> Bool) {
        intakeWritesAvailable = available; intakeWorkspaceCheck = recheck
    }
    func chooseIntakeFiles() async {
        guard canSelectIntake, intakeRows.count < 8, let window = NSApp.keyWindow else { return }
        let generation = importStatusGeneration, workspace = transfer?.workspaceId
        let panel = NSOpenPanel()
        panel.title = "작품 파일 대기 목록에 추가"
        panel.message = "최대 8개까지 추가합니다. 선택만으로 작품에 연결하거나 파일 검사를 시작하지 않습니다."
        panel.canChooseDirectories = false; panel.allowsMultipleSelection = true
        panel.allowedContentTypes = (iccImportAdmitted ? [.png] : []) + (modelIntakeEnabled ? [UTType(filenameExtension: "glb") ?? .data] : [])
        let response = await withCheckedContinuation { continuation in
            panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
        }
        guard response == .OK, canSelectIntake, generation == importStatusGeneration,
              workspace == transfer?.workspaceId else { return }
        let urls = panel.urls.filter { url in !intakeRows.contains { $0.url == url } }
        guard intakeRows.count + urls.count <= 8 else { failure = AuthoringFailure("INTAKE_QUEUE_LIMIT"); return }
        intakeRows += urls.map { IntakeRow(url: $0) }
    }
    func openIntakeRow(_ id: UUID) async {
        guard canOpenIntakeRow, let artwork = selected,
              let row = intakeRows.first(where: { $0.id == id && $0.result == nil }) else { return }
        await action {
            let generation = importStatusGeneration, workspace = transfer?.workspaceId
            guard try await intakeWorkspaceCheck?() == true else { throw AuthoringFailure("INTAKE_WORKSPACE_UNAVAILABLE") }
            do {
                let read = Task { try await ImportFileSnapshot.load(row.url, allowLargeModel: largeModelPreparationEnabled) }
                importRead = read
                defer { importRead = nil }
                let file = try await withTaskCancellationHandler { try await read.value } onCancel: { read.cancel() }
                try Task.checkCancellation()
                guard generation == importStatusGeneration, workspace == transfer?.workspaceId, available,
                      selected?.id == artwork.id, selected?.revision == artwork.revision, !metadataDirty,
                      transfer?.intent == nil, importFile == nil else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
                guard file.mime == "image/png" && iccImportAdmitted || file.mime == "model/gltf-binary" && modelIntakeEnabled else {
                    throw AuthoringFailure("INTAKE_WORKSPACE_UNAVAILABLE")
                }
                guard !artwork.assets.contains(where: { $0.sha256 == file.sha256 && $0.state == "approved" }) else {
                    throw AuthoringFailure("INTAKE_FILE_ALREADY_APPROVED")
                }
                activeIntakeID = id; intakeTarget = (artwork.id, artwork.revision)
                importFile = file; importScale = 1
                if let i = intakeRows.firstIndex(where: { $0.id == id }) { intakeRows[i].error = nil }
            } catch let error as AuthoringFailure {
                if generation == importStatusGeneration, let i = intakeRows.firstIndex(where: { $0.id == id }) { intakeRows[i].error = error.code }
                throw error
            }
        }
    }
    func removeIntakeRow(_ id: UUID) {
        guard !busy, id != activeIntakeID else { return }
        intakeRows.removeAll { $0.id == id }
    }
    func putBackIntakeRow() {
        guard !busy, activeIntakeID != nil, transfer?.intent == nil else { return }
        activeIntakeID = nil; intakeTarget = nil; importFile = nil
    }
    func bindImportStore(read: @escaping () async throws -> NativeUploadState, save: @escaping (NativeUploadState) async throws -> NativeUploadState) {
        readTransfer = read; writeTransfer = save
    }
    func bindHayesOriginal(ready: Bool, execute: @escaping (String, String, Int) async throws -> NativeHayesReceipt,
                           recover: @escaping (String, Int) async throws -> NativeHayesReceipt) {
        hayesSessionReady = ready
        executeHayes = execute
        recoverHayes = recover
    }

    private func recoverHayesIfApproved(_ job: NativeImportJob) async {
        guard job.state == "approved", let intent = transfer?.intent, let hayes = intent.hayesOriginal,
              let recoverHayes, let api else { return }
        let generation = importStatusGeneration
        do {
            let artwork = try await api.artwork(intent.artworkId)
            guard hayes.matchesReleasedDisplay(artwork, job: job, intent: intent),
                  generation == importStatusGeneration, transfer?.intent == intent, transfer?.jobId == job.id else {
                throw AuthoringFailure("HAYES_RELEASE_UNCONFIRMED")
            }
            let receipt = try await recoverHayes(job.id, artwork.revision)
            guard generation == importStatusGeneration, transfer?.intent == intent, transfer?.jobId == job.id else { return }
            hayesReceipt = receipt
        } catch {
            guard generation == importStatusGeneration, transfer?.intent == intent else { return }
            hayesReceipt = nil
            failure = AuthoringFailure("HAYES_RELEASE_UNCONFIRMED")
        }
    }

    private func restoreImport() async throws {
        guard let readTransfer, let session else { return }
        let state = try await readTransfer()
        guard state.tenantId.lowercased() == session.tenantId.lowercased(), state.principalId.lowercased() == session.userId.lowercased() else { throw AuthoringFailure("IMPORT_SCOPE_CHANGED") }
        if transfer == nil, let intent = state.intent { importScale = intent.scaleMeters }
        transfer = state
        importJob = nil; compactJob = nil
        if let compact = state.compactVariant {
            if let id = state.jobId, let api {
                compactJob = try await api.compactModelVariantJob(id, artworkId: compact.artworkId, input: compact.request)
            }
            scheduleImportPoll()
            return
        }
        if let intent = state.intent, intent.modelDerivation != nil, let api {
            try importTransfer.restoreModel(intent: intent, jobID: state.jobId, transport: api)
        }
        if let id = state.jobId, let api {
            let observed = try await api.importJob(id)
            if let intent = state.intent { try importTransfer.observe(observed, intent: intent) }
            importJob = observed
            await recoverHayesIfApproved(observed)
        }
        scheduleImportPoll()
    }
    private func persistImport(_ state: NativeUploadState) async throws {
        guard let writeTransfer else { throw AuthoringFailure("IMPORT_STORE_REQUIRED") }
        transfer = try await writeTransfer(state)
    }
    var hasProductImageTransfer: Bool { transfer?.intent?.productImagePreparation?.isFresh == false }
    var hasFreshImageTransfer: Bool { transfer?.intent?.productImagePreparation?.isFresh == true }
    var freshImagePreparationAvailable: Bool {
        available && intakeWritesAvailable && api != nil
            && session?.nativeFreshImagePreparationProfile == NativeProductImagePreparationIntent.freshProfileID
    }
    var freshImageSourceSelected: Bool {
        hasFreshImageTransfer || (freshImagePreparationAvailable && transfer?.intent == nil
            && selected?.approvedRevision == nil && selected?.approvedAssetId == nil
            && ["image/jpeg", "image/png"].contains(importFile?.mime ?? ""))
    }
    var freshImageReady: NativeProductImagePreparationStatus? {
        guard let transfer, let intent = transfer.intent, let preparation = intent.productImagePreparation, preparation.isFresh,
              let job = importJob, transfer.jobId == job.id, let status = job.productImagePreparation,
              status.validatesReady(intent: intent, preparation: preparation, job: job,
                                    workspaceID: transfer.workspaceId, principalID: transfer.principalId) else { return nil }
        return status
    }

    var productImagePreparationAvailable: Bool {
        available && intakeWritesAvailable && api?.productImageRuntimeQualified == true
            && session?.nativeProductImagePreparationProfile == NativeProductImagePreparationIntent.profileID
    }

    func prepareProductImage(_ url: URL, artworkId: String, artworkRevision: Int,
                             expectedAPI: AuthoringAPI, isCurrent: @escaping @MainActor () -> Bool) async throws {
        guard productImagePreparationAvailable, !busy, !dirty, activeIntakeID == nil,
              compactTransfer == nil, api === expectedAPI, let transfer,
              transfer.intent == nil || transfer.intent?.productImagePreparation != nil,
              await expectedAPI.admitsProductImagePreparation, isCurrent() else {
            throw AuthoringFailure("ARTWORK_REPLACEMENT_UNAVAILABLE")
        }
        let generation = UUID()
        importStatusGeneration = generation
        busy = true
        defer { busy = false; scheduleImportPoll() }
        let file = try await ImportFileSnapshot.load(url, allowJPEG: true)
        let preparation = NativeProductImagePreparationIntent(artworkRevision: artworkRevision)
        let intent: NativeImportIntent
        if let retained = transfer.intent {
            guard retained.artworkId == artworkId, retained.productImagePreparation == preparation,
                  file.matches(retained) else { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            intent = retained
        } else {
            let artwork = try await expectedAPI.artwork(artworkId)
            try preparation.checkSource(file, artwork: artwork)
            guard artwork.id == artworkId, let rights = artwork.metadata?.rights else {
                throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
            }
            intent = NativeImportIntent(artworkId: artworkId, mime: file.mime, sha256: file.sha256,
                bytes: file.data.count, scaleMeters: 1, rights: rights, productImagePreparation: preparation)
        }
        guard importStatusGeneration == generation, self.transfer?.revision == transfer.revision, isCurrent() else {
            throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
        }
        do { try await continueImport(intent: intent, file: file, api: expectedAPI, isScopeCurrent: isCurrent) }
        catch let error as AuthoringFailure where NativeProductImagePreparationIntent.definitivelyRejectedCreate(error) {
            if importStatusGeneration == generation, isCurrent(), var rejected = self.transfer,
               rejected.intent == intent, rejected.jobId == nil {
                rejected.intent = nil; rejected.fileName = nil
                try await persistImport(rejected)
            }
            throw error
        }
    }

    func finishProductImageTransfer(intent: NativeImportIntent, jobId: String, api: AuthoringAPI) async throws {
        guard !busy, self.api === api, var transfer, transfer.intent == intent, transfer.jobId == jobId,
              let preparation = intent.productImagePreparation else { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
        let generation = UUID()
        importStatusGeneration = generation
        busy = true
        defer { busy = false }
        let artwork = try await api.artwork(intent.artworkId)
        let job = try await api.importJob(jobId)
        guard importStatusGeneration == generation, self.transfer?.revision == transfer.revision, job.id == jobId,
              job.productImagePreparation?.validatesReady(intent: intent, preparation: preparation, job: job,
                  workspaceID: transfer.workspaceId, principalID: transfer.principalId) == true,
              artwork.id == intent.artworkId, artwork.revision == preparation.artworkRevision + 1,
              artwork.approvedRevision == artwork.revision, artwork.approvedAssetId == job.assetId else {
            throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
        }
        transfer.intent = nil; transfer.jobId = nil; transfer.fileName = nil
        try await persistImport(transfer)
        importJob = nil
    }

    func changeProductImagePreparation(_ change: NativeImportAction, artworkId: String) async {
        await action {
            guard productImagePreparationAvailable, let api, await api.admitsProductImagePreparation,
                  var transfer, let intent = transfer.intent, intent.artworkId == artworkId,
                  let preparation = intent.productImagePreparation, let jobId = transfer.jobId else {
                throw AuthoringFailure("ARTWORK_REPLACEMENT_UNAVAILABLE")
            }
            let generation = importStatusGeneration
            var observed = try await api.importJob(jobId)
            guard observed.id == jobId, observed.productImagePreparation?.matches(preparation, job: observed) == true,
                  importStatusGeneration == generation, self.transfer?.revision == transfer.revision else {
                throw AuthoringFailure("IMPORT_RECEIPT_INVALID")
            }
            switch change {
            case .cancel:
                guard observed.state != "approved" else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
                if observed.state != "cancelled" { observed = try await api.importAction(jobId, action: change) }
            case .retry:
                guard observed.state == "failed", observed.canRetry, observed.retryAt != nil,
                      observed.confirmsIntakeDrain(for: intent) else { throw AuthoringFailure("IMPORT_DRAIN_PENDING") }
                observed = try await api.importAction(jobId, action: change)
            default:
                throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
            }
            guard observed.id == jobId, observed.productImagePreparation?.matches(preparation, job: observed) == true,
                  importStatusGeneration == generation, self.transfer?.revision == transfer.revision else {
                throw AuthoringFailure("IMPORT_RECEIPT_INVALID")
            }
            importJob = observed
            if observed.state == "cancelled", observed.confirmsIntakeDrain(for: intent) {
                transfer.intent = nil; transfer.jobId = nil; transfer.fileName = nil
                try await persistImport(transfer)
                importJob = nil
            }
            scheduleImportPoll()
        }
    }
    func chooseImportFile(inspectUnknown: Bool = false) async {
        guard !hasProductImageTransfer, compactTransfer == nil, !inspectUnknown || canInspectUnknownModel,
              !hasUnknownSourceUnits || usesHayesOriginal || ordinaryUnknownModel else { return }
        let hayes = usesHayesOriginal
        let unknown = inspectUnknown || ordinaryUnknownModel
        guard !busy, available, session != nil, activeIntakeID == nil || transfer?.intent != nil else { return }
        let panel = NSOpenPanel()
        panel.title = "작품 파일 가져오기"
        panel.message = (jpegImportsEnabled || freshImagePreparationAvailable) ? "GLB·PNG·JPEG 파일을 선택하세요. 최대 32 MiB이며 JPEG 원본을 유지하고 감상용 PNG를 준비합니다." : "GLB 또는 PNG 파일을 선택하세요. 최대 32 MiB를 지원합니다."
        if largeModelPreparationEnabled {
            panel.message = (jpegImportsEnabled || freshImagePreparationAvailable)
                ? "GLB는 최대 64 MiB, PNG·JPEG는 최대 32 MiB를 지원합니다."
                : "GLB는 최대 64 MiB, PNG는 최대 32 MiB를 지원합니다."
        }
        panel.canChooseDirectories = false; panel.canChooseFiles = true; panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [UTType(filenameExtension: "glb") ?? .data]
        if hayes {
            panel.message = "등록할 Hayes GLB 원본을 선택하세요."
        } else if unknown {
            panel.message = "원본 단위를 확인할 수 없는 GLB를 선택하세요. 구조와 좌표 범위를 확인합니다."
        } else {
            panel.allowedContentTypes += [.png] + ((jpegImportsEnabled || freshImagePreparationAvailable) ? [.jpeg] : [])
        }
        guard let window = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.identifier?.rawValue == "manager" }) else { return }
        let response = await withCheckedContinuation { continuation in
            panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
        }
        guard response == .OK, let url = panel.url else { return }
        await action {
            let read = Task {
                try await ImportFileSnapshot.load(url, allowJPEG: !hayes && (jpegImportsEnabled || freshImagePreparationAvailable),
                    byteLimit: hayes ? NativeHayesOriginalIntent.sourceBytes : ImportFileSnapshot.limit,
                    allowLargeModel: !hayes && largeModelPreparationEnabled)
            }
            importRead = read
            defer { importRead = nil }
            let snapshot = try await withTaskCancellationHandler { try await read.value } onCancel: { read.cancel() }
            try Task.checkCancellation()
            if hayes {
                guard snapshot.mime == "model/gltf-binary", snapshot.sha256 == NativeHayesOriginalIntent.sourceSHA256,
                      snapshot.data.count == NativeHayesOriginalIntent.sourceBytes else {
                    throw AuthoringFailure("IMPORT_INTENT_CHANGED")
                }
            }
            if let intent = transfer?.intent, !snapshot.matches(intent) { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            if inspectUnknown {
                guard let api, canInspectUnknownModel else { throw AuthoringFailure("IMPORT_MODEL_UNAVAILABLE") }
                let scope = importStatusGeneration
                let inspection = try await api.inspectModelOriginal(snapshot)
                guard self.api === api, scope == importStatusGeneration, canInspectUnknownModel else {
                    throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
                }
                modelOriginalInspection = inspection
                provisionalHeight = draft.displayReference?.referenceHeightMeters
            } else {
                if ordinaryUnknownModel, draft.displayReference?.assetSha256 != snapshot.sha256 {
                    throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
                }
                modelOriginalInspection = nil
                provisionalHeight = nil
            }
            importFile = snapshot
            if draft.title.isEmpty && selected == nil { draft.title = url.deletingPathExtension().lastPathComponent }
            notice = inspectUnknown ? "구조와 좌표 범위를 확인했습니다. 임시 표시 높이를 입력하세요."
                : "파일을 선택했습니다. 작품 정보와 권리를 저장한 뒤 파일 검사를 시작하세요."
        }
    }
    func uploadImportFile() async {
        await action {
            guard !hasProductImageTransfer, let api, let selected, !metadataDirty, let file = importFile, let state = transfer,
                  !inspectedReferencePending, state.compactVariant == nil else { throw AuthoringFailure("IMPORT_FILE_REQUIRED") }
            if usesHayesOriginal {
                guard hayesOriginalAvailable, let reference = selected.metadata?.displayReference,
                      file.sha256 == NativeHayesOriginalIntent.sourceSHA256,
                      file.data.count == NativeHayesOriginalIntent.sourceBytes else {
                    throw AuthoringFailure("HAYES_OPERATION_UNAVAILABLE")
                }
                let intent = NativeImportIntent(artworkId: selected.id, idempotencyKey: state.intent?.idempotencyKey ?? UUID().uuidString.lowercased(),
                    mime: file.mime, sha256: file.sha256, bytes: file.data.count, scaleMeters: 1, rights: draft.rights,
                    hayesOriginal: .init(artworkRevision: selected.revision, displayReferenceSha256: reference.assetSha256))
                if let saved = state.intent, saved != intent { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
                try await continueImport(intent: intent, file: file, api: api)
                approvalAcknowledged = false
                notice = "원본을 등록했습니다. 저장한 작품 정보와 권리를 확인한 뒤 감상용 파일을 준비할 수 있습니다."
                return
            }
            if hasUnknownSourceUnits {
                guard ordinaryUnknownModel, file.mime == "model/gltf-binary", !draft.provenance.scaleApplied,
                      selected.metadata?.displayReference?.assetSha256 == file.sha256,
                      state.intent == nil || state.intent?.modelPreparation != nil else {
                    throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
                }
            }
            let generation = importStatusGeneration, currentScope = scope, manifest = runtimeManifestSHA256
            let target = intakeTarget
            let startsRequiredModel = state.intent == nil && file.mime == "model/gltf-binary" && requiredModelPreparationEnabled
            if let target {
                guard target.artworkID == selected.id, target.revision == selected.revision,
                      try await intakeWorkspaceCheck?() == true else { throw AuthoringFailure("INTAKE_WORKSPACE_UNAVAILABLE") }
            }
            if target != nil || startsRequiredModel {
                let fresh = try await api.artwork(selected.id)
                guard available, generation == importStatusGeneration, currentScope == scope,
                      fresh.id == selected.id, !fresh.archived, fresh.revision == selected.revision, fresh.metadata == selected.metadata,
                      self.selected?.id == selected.id, self.selected?.revision == selected.revision,
                      !metadataDirty, self.transfer?.revision == state.revision else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
                if startsRequiredModel { try NativeModelPreparationIntent.requireFreshIntake(for: fresh) }
                if target != nil {
                    guard file.mime == "image/png" && iccImportAdmitted || file.mime == "model/gltf-binary" && modelIntakeEnabled else {
                        throw AuthoringFailure("INTAKE_WORKSPACE_UNAVAILABLE")
                    }
                    if state.intent == nil, fresh.assets.contains(where: { $0.sha256 == file.sha256 && $0.state == "approved" }) {
                        throw AuthoringFailure("INTAKE_FILE_ALREADY_APPROVED")
                    }
                }
            }
            if freshImageSourceSelected {
                guard freshImagePreparationAvailable, await api.admitsFreshImagePreparation,
                      try await intakeWorkspaceCheck?() == true else {
                    throw AuthoringFailure("IMAGE_PREPARATION_UNAVAILABLE")
                }
                let preparation = state.intent?.productImagePreparation
                    ?? .init(artworkRevision: selected.revision, fresh: true)
                try preparation.checkFreshArtwork(selected, rights: draft.rights)
                let intent = state.intent ?? NativeImportIntent(artworkId: selected.id, mime: file.mime, sha256: file.sha256,
                    bytes: file.data.count, scaleMeters: 1, rights: draft.rights, productImagePreparation: preparation)
                guard generation == importStatusGeneration, currentScope == scope, self.api === api,
                      self.selected?.id == selected.id, self.selected?.revision == selected.revision,
                      self.transfer?.revision == state.revision, !metadataDirty,
                      intent.artworkId == selected.id, intent.rights == draft.rights, file.matches(intent) else {
                    throw AuthoringFailure("IMPORT_INTENT_CHANGED")
                }
                try await continueImport(intent: intent, file: file, api: api)
                approvalAcknowledged = false
                notice = "원본 파일을 보냈습니다. 감상용 파일이 준비되면 작품 정보와 권리를 검토하세요."
                scheduleImportPoll()
                return
            }
            let rights = draft.rights, scale = ordinaryUnknownModel ? 1 : importScale
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
                  !metadataDirty, draft.rights == rights, ordinaryUnknownModel || importScale == scale,
                  transfer?.revision == state.revision, transfer?.workspaceId == state.workspaceId,
                  importFile?.sha256 == file.sha256 else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
            let modelPreparation: NativeModelPreparationIntent?
            if let previous = state.intent {
                modelPreparation = previous.modelPreparation
            } else if startsRequiredModel {
                modelPreparation = NativeModelPreparationIntent(operationID: requestKey,
                    generation: UUID().uuidString.lowercased(), artworkRevision: selected.revision,
                    large: file.data.count > ImportFileSnapshot.limit)
            } else { modelPreparation = nil }
            let modelDerivation: NativeModelDerivationIntent?
            if let previous = state.intent {
                modelDerivation = previous.modelDerivation // same server generation on recovery
            } else if modelPreparation == nil, file.mime == "model/gltf-binary", modelDerivationEnabled {
                modelDerivation = NativeModelDerivationIntent(operationID: requestKey, generation: generation.uuidString.lowercased(), artworkRevision: selected.revision)
            } else { modelDerivation = nil }
            let intent = NativeImportIntent(artworkId: selected.id, idempotencyKey: requestKey, mime: file.mime,
                sha256: file.sha256, bytes: file.data.count, scaleMeters: scale, rights: rights,
                derived: derived?.derived, modelDerivation: modelDerivation, modelPreparation: modelPreparation)
            if let previous = state.intent, previous != intent { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            try await continueImport(intent: intent, file: file, derived: derived, api: api)
            approvalAcknowledged = false
            notice = "파일 검사 요청을 확인했습니다. 현재 상태와 작품 승인은 아래에서 확인하세요."
            scheduleImportPoll()
        }
    }
    func recoverImport() async {
        await action {
            guard !hasProductImageTransfer, let api, let intent = transfer?.intent else { return }
            try await checkGUIImportContinuation(intent, api: api)
            try await continueImport(intent: intent, file: nil, api: api)
            notice = "같은 파일 검사 요청의 현재 상태를 확인했습니다."
            scheduleImportPoll()
        }
    }
    private func checkGUIImportContinuation(_ intent: NativeImportIntent, api: AuthoringAPI) async throws {
        let generation = importStatusGeneration
        let artwork = try await api.artwork(intent.artworkId)
        guard generation == importStatusGeneration, available, self.api === api,
              transfer?.intent == intent else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
        if let hayes = intent.hayesOriginal {
            guard api.hayesOriginalRuntimeQualified, artwork.id == intent.artworkId,
                  artwork.revision == hayes.artworkRevision,
                  artwork.metadata?.displayReference?.assetSha256 == hayes.displayReferenceSha256,
                  artwork.metadata?.displayReference?.status == "provisional-uncalibrated",
                  artwork.metadata?.provenance.sourceUnits == "unknown", artwork.metadata?.rights == intent.rights else {
                throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
            }
            return
        }
        guard artwork.id == intent.artworkId, let metadata = artwork.metadata else {
            throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
        }
        if let preparation = intent.productImagePreparation, preparation.isFresh {
            guard freshImagePreparationAvailable, await api.admitsFreshImagePreparation else {
                throw AuthoringFailure("IMAGE_PREPARATION_UNAVAILABLE")
            }
            try preparation.checkFreshArtwork(artwork, rights: intent.rights, approvedAssetId: importJob?.assetId)
            return
        }
        if metadata.provenance.sourceUnits == "unknown" {
            guard modelOriginalInspectionAvailable, intent.modelPreparation != nil, intent.scaleMeters == 1,
                  !metadata.provenance.scaleApplied, metadata.displayReference?.assetSha256 == intent.sha256 else {
                throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
            }
        } else if !["m", "cm", "mm"].contains(metadata.provenance.sourceUnits) {
            throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
        }
        if let preparation = intent.modelPreparation {
            guard requiredModelPreparationEnabled, artwork.revision == preparation.artworkRevision,
                  !artwork.archived, metadata.rights == intent.rights else {
                throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
            }
        }
    }
    private func continueImport(intent: NativeImportIntent, file: ImportFileSnapshot?, derived: NativePreparedJPEG? = nil,
                                api: AuthoringAPI, isScopeCurrent: @escaping @MainActor () -> Bool = { true }) async throws {
        guard intent.modelDerivation == nil || modelDerivationEnabled else { throw AuthoringFailure("IMPORT_MODEL_UNAVAILABLE") }
        guard intent.modelPreparation == nil || requiredModelPreparationEnabled else { throw AuthoringFailure("IMPORT_MODEL_UNAVAILABLE") }
        let generation = importStatusGeneration, currentScope = scope, manifest = runtimeManifestSHA256
        let tenant = session?.tenantId, principal = session?.userId
        @MainActor func current() -> Bool {
            generation == importStatusGeneration && currentScope == scope && available && isScopeCurrent()
                && (intent.productImagePreparation?.isFresh == true
                    ? (freshImagePreparationAvailable && manifest == runtimeManifestSHA256)
                    : (intent.mime != "image/jpeg" || ((intent.productImagePreparation == nil
                        ? jpegImportsEnabled : productImagePreparationAvailable) && manifest == runtimeManifestSHA256)))
                && (intent.modelDerivation == nil || (modelDerivationEnabled && manifest == runtimeManifestSHA256))
                && (intent.modelPreparation == nil || (requiredModelPreparationEnabled && manifest == runtimeManifestSHA256))
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
        let allowICC = NativeJPEGImportRuntime.supportsICC(manifestSHA256: manifest, ready: available)
        let owned = Task {
            try await NativePreparedJPEG.prepare(original: original, operationID: ticket.operationID, generation: ticket.generation,
                isCurrent: { ticket.isCurrent(operationID: operationID, generation: generation.uuidString.lowercased(), originalSHA256: original.sha256) },
                allowICC: allowICC)
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
            if current() { notice = "" }
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
            guard let api, let job = importJob,
                  transfer?.permitsOrdinaryJobAction(job.id, selectedArtworkId: selected?.id) == true else {
                throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
            }
            if change == .retry {
                guard let intent = transfer?.intent, transfer?.jobId == job.id else {
                    throw AuthoringFailure("IMPORT_RECEIPT_INVALID")
                }
                try await checkGUIImportContinuation(intent, api: api)
                guard importJob?.id == job.id, transfer?.jobId == job.id else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
                if intent.coarsePreparation != nil {
                    let observed = try await api.importJob(job.id)
                    guard observed.id == job.id, transfer?.jobId == job.id, transfer?.intent == intent,
                          observed.canRetryReviewedImport(intent) else {
                        throw AuthoringFailure("IMPORT_DRAIN_PENDING")
                    }
                    importJob = observed
                }
                if intent.modelPreparation != nil || intent.productImagePreparation?.isFresh == true {
                    let observed = try await api.importJob(job.id)
                    guard observed.id == job.id, transfer?.jobId == job.id, transfer?.intent == intent,
                          observed.canRetry, observed.confirmsIntakeDrain(for: intent) else {
                        throw AuthoringFailure("IMPORT_DRAIN_PENDING")
                    }
                    importJob = observed
                }
            }
            if let intent = transfer?.intent, intent.derived?.recipe.colourConversion != nil {
                guard iccImportAdmitted else { throw AuthoringFailure("IMPORT_ICC_UNAVAILABLE") }
                guard transfer?.jobId?.lowercased() == job.id.lowercased() else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
                try importTransfer.observe(job, intent: intent)
            }
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
            await recoverHayesIfApproved(observed)
            if !hasProductImageTransfer, importJob?.state == "approved", let selected, selected.id == transfer?.intent?.artworkId {
                let fresh = try await api.artwork(selected.id)
                if fresh.revision == selected.revision { self.selected = fresh; if let asset = importJob?.assetId { assetId = asset } }
            }
            scheduleImportPoll()
        }
    }
    func clearImport() async {
        await action {
            guard !hasProductImageTransfer else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
            guard transfer?.intent?.derived?.recipe.colourConversion == nil || iccImportAdmitted else { throw AuthoringFailure("IMPORT_ICC_UNAVAILABLE") }
            guard importJob?.terminal == true, !modelWorkerPending,
                  importJob?.modelDerivation?.workerDrained != false, var state = transfer else { throw AuthoringFailure("IMPORT_ACTIVE") }
            let generation = importStatusGeneration, currentScope = scope
            let tenant = session?.tenantId, principal = session?.userId
            if let intent = state.intent, let job = importJob, requiresIntakeDrain(intent, job: job) {
                guard let api, job.id == state.jobId else { throw AuthoringFailure("IMPORT_DRAIN_PENDING") }
                let observed = try await api.importJob(job.id)
                try importTransfer.observe(observed, intent: intent)
                importJob = observed
                guard observed.id == job.id, observed.terminal, observed.confirmsIntakeDrain(for: intent),
                      transfer?.revision == state.revision else { throw AuthoringFailure("IMPORT_DRAIN_PENDING") }
            }
            if let intent = state.intent, intent.derived?.recipe.colourConversion != nil {
                guard let api, let job = importJob, state.jobId?.lowercased() == job.id.lowercased() else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
                try importTransfer.observe(job, intent: intent)
                if job.state == "approved" {
                    let artwork = try await api.artwork(intent.artworkId)
                    guard artwork.approvedRevision == artwork.revision, artwork.approvedAssetId == job.assetId else {
                        throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                    }
                }
            }
            let completed = importJob
            if let intent = state.intent, intent.modelPreparation != nil {
                guard let api, let job = completed, let preparation = job.modelPreparation else {
                    throw AuthoringFailure("IMPORT_RECEIPT_INVALID")
                }
                let artwork = job.state == "approved" ? try await api.artwork(intent.artworkId) : nil
                guard preparation.permitsClearingTransfer(artwork: artwork, request: intent, job: job) else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
            }
            if let intent = state.intent, intent.productImagePreparation?.isFresh == true,
               let job = completed, job.state == "approved" {
                guard let api else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
                let artwork = try await api.artwork(intent.artworkId)
                guard artwork.approvedRevision == intent.productImagePreparation?.artworkRevision,
                      artwork.approvedAssetId == job.assetId else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
            }
            if state.intent?.hayesOriginal != nil, let job = completed, job.state == "approved" {
                guard let api, let intent = state.intent, let hayes = intent.hayesOriginal else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
                let artwork = try await api.artwork(intent.artworkId)
                guard artwork.approvedRevision == artwork.revision, artwork.approvedAssetId == job.assetId,
                      hayes.matchesReleasedDisplay(artwork, job: job, intent: intent) else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
            }
            guard available, generation == importStatusGeneration, currentScope == scope,
                  tenant == session?.tenantId, principal == session?.userId,
                  transfer?.workspaceId == state.workspaceId, transfer?.revision == state.revision,
                  transfer?.intent == state.intent, transfer?.jobId == state.jobId,
                  completed?.id == state.jobId else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
            state.intent = nil; state.jobId = nil; state.fileName = nil
            try await persistImport(state)
            if let activeIntakeID, let i = intakeRows.firstIndex(where: { $0.id == activeIntakeID }), let completed, let target = intakeTarget {
                intakeRows[i].result = IntakeRow.Result(artworkID: target.artworkID, artworkRevision: target.revision,
                                                        jobID: completed.id, state: completed.state)
                self.activeIntakeID = nil; intakeTarget = nil
            }
            importJob = nil; importFile = nil; approvalAcknowledged = false
            modelOriginalInspection = nil; provisionalHeight = nil
        }
    }
    func prepareHayesOriginal() async {
        await action {
            guard hayesOriginalAvailable, approvalAcknowledged, !metadataDirty, let api, let executeHayes,
                  let selected, let job = importJob, let intent = transfer?.intent,
                  let hayes = intent.hayesOriginal, selected.id == intent.artworkId,
                  transfer?.jobId == job.id, job.state == "queued" else {
                throw AuthoringFailure("HAYES_OPERATION_UNAVAILABLE")
            }
            let generation = importStatusGeneration
            try await checkGUIImportContinuation(intent, api: api)
            let receipt = try await executeHayes(job.id, intent.artworkId, hayes.artworkRevision)
            guard generation == importStatusGeneration, available, transfer?.intent == intent,
                  self.selected?.id == intent.artworkId else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
            hayesReceipt = receipt
            hayesSessionReady = false
            let observed = try await api.importJob(job.id)
            let artwork = try await api.artwork(intent.artworkId)
            guard generation == importStatusGeneration, available, transfer?.intent == intent,
                  observed.id == job.id, hayes.confirmsDisplay(artwork, job: observed, intent: intent) else {
                throw AuthoringFailure("IMPORT_RECEIPT_INVALID")
            }
            importJob = observed; self.selected = artwork; assetId = observed.assetId ?? ""
            approvalAcknowledged = false
            notice = "감상용 파일 준비와 원본 보관을 확인했습니다. 작품 정보와 권리를 승인할 수 있습니다."
        }
    }
    func approveImportedArtwork() async {
        await action {
            guard !hasProductImageTransfer, let api, let selected, !metadataDirty, approvalAcknowledged,
                  hasFreshImageTransfer ? freshImageReady?.full != nil
                    : selected.assets.contains(where: { $0.id == assetId && $0.state == "approved" }) else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
            if usesHayesOriginal {
                guard let recoverHayes, let intent = transfer?.intent, let job = importJob,
                      job.assetId == assetId,
                      intent.hayesOriginal?.matchesReleasedDisplay(selected, job: job, intent: intent) == true else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
                hayesReceipt = try await recoverHayes(job.id, selected.revision)
                guard transfer?.intent == intent, importJob?.id == job.id else {
                    throw AuthoringFailure("IMPORT_CONTEXT_CHANGED")
                }
            }
            if let intent = transfer?.intent, intent.artworkId.lowercased() == selected.id.lowercased(),
               intent.derived?.recipe.colourConversion != nil {
                guard iccImportAdmitted else { throw AuthoringFailure("IMPORT_ICC_UNAVAILABLE") }
                guard let job = importJob, jpegOriginalStored, job.assetId == assetId else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
                try importTransfer.observe(job, intent: intent)
            }
            if transfer?.intent?.derived != nil, transfer?.intent?.artworkId == selected.id, importJob?.assetId == assetId {
                guard jpegOriginalStored else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            }
            let approved: NativeArtwork
            if let transfer, let intent = transfer.intent, intent.productImagePreparation?.isFresh == true {
                guard freshImagePreparationAvailable, intent.artworkId == selected.id, let jobId = transfer.jobId else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
                let job = try await api.importJob(jobId)
                let current = try await api.artwork(selected.id)
                guard self.transfer?.revision == transfer.revision, self.selected?.id == selected.id,
                      job.id == jobId, job.productImagePreparation?.serverReceipt?.sha256 == freshImageReady?.serverReceipt?.sha256,
                      current.metadata == selected.metadata else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
                approved = try await api.approveFreshImage(job, request: intent, current: current, workspaceID: transfer.workspaceId)
            } else if let intent = transfer?.intent, intent.modelPreparation != nil, intent.artworkId == selected.id {
                guard requiredModelPreparationEnabled, let jobId = transfer?.jobId else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
                let job = try await api.importJob(jobId)
                let current = try await api.artwork(selected.id)
                guard transfer?.intent == intent, transfer?.jobId == jobId, self.selected?.id == selected.id,
                      job.assetId == assetId, job.modelPreparation?.permitsReview(of: current, request: intent, job: job) == true else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
                approved = try await api.approvePreparedModel(job, request: intent, current: current)
            } else {
                guard !hasUnknownSourceUnits || usesHayesOriginal else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
                approved = try await api.approveArtwork(selected.id, revision: selected.revision, assetId: assetId)
            }
            self.selected = approved
            if let metadata = approved.metadata { draft = metadata; original = metadata }
            approvalAcknowledged = false
            notice = "파일과 저장한 정보·권리를 승인했습니다. 전시에서 사용할 준비가 되었습니다."
        }
    }
    func prepareCompactVariant() async {
        let allowed = canPrepareCompactVariant
        await action {
            guard let api, let selected, var state = transfer, !metadataDirty,
                  await api.admitsCompactModelVariant else { throw AuthoringFailure("IMPORT_MODEL_UNAVAILABLE") }
            let compact: NativeCompactModelVariantTransfer
            if let retained = state.compactVariant {
                guard retained.artworkId == selected.id, state.jobId == nil else {
                    throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
                }
                compact = retained
            } else {
                guard allowed, let full = approvedFullModel, let bytes = Int(exactly: full.bytes) else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
                let request = NativeCompactModelVariantRequest(revision: selected.revision, assetId: full.id,
                                                               sha256: full.sha256, bytes: bytes)
                let approved = try await api.studioArtwork(selected.id).artwork
                guard approved["id"].string == selected.id, approved["revision"].integer == Int64(selected.revision),
                      approved["primaryAssetId"].string == full.id,
                      approved["extensions"]["org.exhibitos.viewer/lod"] == .null else {
                    throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED")
                }
                guard self.api === api, self.selected?.id == selected.id, transfer?.revision == state.revision else {
                    throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
                }
                compact = .init(artworkId: selected.id, request: request)
                state.compactVariant = compact
                try await persistImport(state)
            }
            guard self.api === api, self.selected?.id == selected.id,
                  transfer?.compactVariant == compact, transfer?.jobId == nil else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            let job = try await api.createCompactModelVariant(artworkId: compact.artworkId, input: compact.request)
            guard self.api === api, var retained = transfer, retained.compactVariant == compact, retained.jobId == nil else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            retained.jobId = job.id
            try await persistImport(retained)
            compactJob = job; approvalAcknowledged = false
            scheduleImportPoll()
        }
    }

    func refreshCompactVariant() async {
        await action {
            guard let api, let compact = compactTransfer, let id = transfer?.jobId else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            let observed = try await api.compactModelVariantJob(id, artworkId: compact.artworkId, input: compact.request)
            guard self.api === api, compactTransfer == compact, transfer?.jobId == id else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            compactJob = observed
            scheduleImportPoll()
        }
    }

    func changeCompactVariant(_ change: NativeImportAction) async {
        await action {
            guard let api, let compact = compactTransfer, let id = transfer?.jobId else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            let job = try await api.changeCompactModelVariant(change, jobId: id, artworkId: compact.artworkId,
                                                             input: compact.request)
            guard self.api === api, compactTransfer == compact, transfer?.jobId == id else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            compactJob = job; approvalAcknowledged = false
            scheduleImportPoll()
        }
    }

    func approveCompactVariant() async {
        await action {
            guard approvalAcknowledged, !metadataDirty, let api, let compact = compactTransfer, let id = transfer?.jobId,
                  selected?.id == compact.artworkId else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
            _ = try await api.approveCompactModelVariant(artworkId: compact.artworkId, jobId: id, input: compact.request)
            let artwork = try await api.artwork(compact.artworkId)
            guard self.api === api, compactTransfer == compact, transfer?.jobId == id,
                  selected?.id == artwork.id, artwork.approvedRevision == compact.request.revision + 1,
                  artwork.approvedAssetId == compact.request.assetId else { throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED") }
            selected = artwork; assetId = compact.request.assetId
            approvalAcknowledged = false
            notice = "전체 모델을 유지하고 축소본을 다음 작품 버전에 승인했습니다."
        }
    }

    func clearCompactVariant() async {
        await action {
            guard let api, let compact = compactTransfer, var state = transfer, let id = state.jobId else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            let job = try await api.compactModelVariantJob(id, artworkId: compact.artworkId, input: compact.request)
            if job.state != "cancelled" || !job.modelPreparation.workerDrained {
                _ = try await api.verifyApprovedCompactModelVariant(artworkId: compact.artworkId, jobId: id,
                                                                   input: compact.request)
            }
            guard self.api === api, transfer?.revision == state.revision, compactTransfer == compact else {
                throw AuthoringFailure("COMPACT_VARIANT_CONTEXT_CHANGED")
            }
            state.compactVariant = nil; state.jobId = nil
            try await persistImport(state)
            compactJob = nil; approvalAcknowledged = false
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
            if let compact = compactTransfer {
                let job = try await api.compactModelVariantJob(id, artworkId: compact.artworkId, input: compact.request)
                guard current(), compactTransfer == compact else { return }
                compactJob = job
                return
            }
            let observed = try await api.importJob(id)
            guard current() else { return }
            if let intent = transfer?.intent { try importTransfer.observe(observed, intent: intent) }
            importJob = observed
            if !modelWorkerPending, failure?.code == "IMPORT_DRAIN_PENDING" { failure = nil }
            if !hasProductImageTransfer, observed.state == "approved", let selected, selected.id == transfer?.intent?.artworkId {
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
        if let compactJob {
            return compactJob.modelPreparation.status == .pending || compactJob.modelPreparation.status == .processing
                || (["failed", "cancelled"].contains(compactJob.state) && !compactJob.modelPreparation.workerDrained)
        }
        guard let job = importJob else { return false }
        return ["queued", "processing"].contains(job.state) || (job.state == "failed" && job.attempts < 5 && !job.canRetry)
            || intakeDrainPending || job.modelDerivation?.workerDrained == false || (job.state == "approved" && job.modelDerivation?.status == "pending")
            || job.modelPreparation?.status == .processing
    }
}

struct NativeAuthoringView: View {
    @ObservedObject var model: AuthoringModel
    let configuration: NativeConfiguration?
    let ready: Bool
    let runtimeJobID: String?
    let mode: Mode
    let connect: () async -> Void
    @State private var discardPrompt = false
    @State private var pendingId: String?
    @State private var pendingNew = false
    private var scope: String { "\(configuration?.tenantId ?? ""):\(configuration?.ports.web ?? 0):\(configuration?.manifestSha256 ?? ""):\(runtimeJobID ?? ""):\(ready)" }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if !ready { Text("작업 환경에서 로컬 제작을 시작하세요.").foregroundStyle(.secondary) }
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
                if model.editing {
                    NativeImportControls(model: model, ready: ready, mode: mode, openArtwork: { select($0) })
                    if model.intakeMetadataHeld {
                        Text(model.hasFreshImageTransfer
                             ? "준비된 파일의 검토를 마친 뒤 작품 정보를 편집할 수 있습니다."
                             : AuthoringFailure("IMPORT_MODEL_METADATA_HELD").localizedDescription).foregroundStyle(.secondary)
                    }
                    editor.disabled(model.intakeMetadataHeld)
                }
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
        .task(id: scope) {
            guard model.configure(configuration, ready: ready, runtimeJobID: runtimeJobID),
                  configuration?.kind == "personal", model.session == nil else { return }
            await connect()
        }
        .confirmationDialog("저장하지 않은 입력을 버릴까요?", isPresented: $discardPrompt, titleVisibility: .visible) {
            Button("입력 버리기", role: .destructive) { performSelection() }
            Button("계속 편집", role: .cancel) {}
        }
    }
    private func select(_ id: String?) {
        guard model.activeIntakeID == nil else { return }
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
                if model.draft.provenance.sourceUnits == "unknown" {
                    LabeledContent("등록된 높이 (m)", value: model.draft.dimensions.height.formatted())
                    LabeledContent("등록된 너비 (m)", value: model.draft.dimensions.width.formatted())
                    if let depth = model.draft.dimensions.depth {
                        LabeledContent("등록된 깊이 (m)", value: depth.formatted())
                    }
                    Text("GLB 원본 단위와 실물 치수의 좌표축 대응은 미확인입니다.").foregroundStyle(.secondary)
                } else {
                    TextField("가로 (m)", value: $model.draft.dimensions.width, format: .number)
                    TextField("세로 (m)", value: $model.draft.dimensions.height, format: .number)
                    TextField("깊이 (m, 선택)", value: $model.draft.dimensions.depth, format: .number)
                }
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
                if model.draft.provenance.sourceUnits == "unknown" {
                    LabeledContent("GLB 원본 단위", value: "미확인")
                    if let reference = model.draft.displayReference {
                        LabeledContent("임시 표시 높이 · 미보정", value: reference.referenceHeightMeters.formatted() + " m")
                    }
                } else {
                    Picker("원래 단위", selection: $model.draft.provenance.sourceUnits) { Text("m").tag("m"); Text("cm").tag("cm"); Text("mm").tag("mm") }
                    Toggle("크기를 조정했음", isOn: $model.draft.provenance.scaleApplied)
                }
                TextField("출처 설명", text: $model.draft.provenance.notes, axis: .vertical)
                if model.canInspectUnknownModel {
                    Button("단위 미확인 GLB 검사…") { Task { await model.chooseImportFile(inspectUnknown: true) } }
                    if let inspection = model.modelOriginalInspection {
                        LabeledContent("확인한 삼각형", value: inspection.triangles.formatted())
                        Text("GLB 구조와 좌표 범위만 확인했습니다. 실물 단위·정면·바닥 접점은 미확인입니다.")
                            .foregroundStyle(.secondary)
                        TextField("임시 표시 높이 (m)", value: $model.provisionalHeight, format: .number)
                        Button("임시 표시 높이 설정") { model.applyInspectedDisplayReference() }
                            .disabled(model.provisionalHeight == nil)
                    }
                }
            }
            if let selected = model.selected {
                Text("저장된 revision \(selected.revision) · 파일 \(selected.assets.count)개")
                Text("변경한 작품 버전은 정보·파일·권리를 다시 확인해 승인해야 합니다.").foregroundStyle(.secondary)
            }
            Button("작품 정보 저장") { Task { await model.save() } }.buttonStyle(.borderedProminent)
                .disabled(model.draft.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.artistId.isEmpty
                    || model.draft.rights.holder.isEmpty || model.inspectedReferencePending)
        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true).disabled(model.busy || !ready)
    }
}
