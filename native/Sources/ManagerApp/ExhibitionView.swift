// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ImageIO
import CryptoKit
import UniformTypeIdentifiers
import ManagerCore

@MainActor
final class ExhibitionModel: ObservableObject {
    enum OpeningStage {
        case creating, loadingSaved, refreshingArtwork

        var title: String {
            switch self {
            case .creating: "새 전시 입력을 만들고 있습니다…"
            case .loadingSaved: "저장한 전시 입력과 편집 권한을 확인하고 있습니다…"
            case .refreshingArtwork: "작품 목록을 새로 확인하고 있습니다…"
            }
        }
    }

    @Published private(set) var openingStage: OpeningStage?

    let oexImport = OEXImportModel()
    @Published private(set) var state: NativeDocumentState?
    @Published private(set) var items: [NativeExhibitionSummary] = []
    @Published private(set) var library: [ArtworkSummary] = []
    @Published private(set) var catalogEntries: [String: ArtworkCatalogEntry] = [:]
    @Published private(set) var catalogFailures: Set<String> = []
    @Published var catalogQuery = ""
    @Published var catalogFilter = ArtworkCatalogFilter.all
    @Published private(set) var catalogArtworkId = ""
    @Published private(set) var catalogRevealID: String?
    @Published private(set) var catalogStatus = ""
    @Published private(set) var artworkPlacement: ArtworkPlacementIntent?
    @Published private(set) var artworkPlacementTarget: ViewportPlacementTarget?
    @Published private(set) var placementApplying = false
    private var placementAttempt: UUID?
    var placingArtwork: Bool { artworkPlacement != nil || placementApplying }
    var canPlaceVisually: Bool {
        guard let entry = catalogEntries[catalogArtworkId] else { return false }
        return canPlaceVisually(entry)
    }
    func canPlaceVisually(_ entry: ArtworkCatalogEntry) -> Bool {
        guard !ArtworkSizePresentation(entry.artwork).sourceUnitsUnknown else { return false }
        return canSelectCatalog && !immutable && viewportPresentation == .threeD && viewportController.connected && viewportController.placementSupported
    }
    @Published private(set) var libraryCursor: String?
    private var catalogTask: Task<Void, Never>?
    @Published private(set) var catalogImages: [String: NSImage] = [:]
    @Published private(set) var catalogPreviewFailures: Set<String> = []
    struct RegisteredFurnishing: Equatable {
        let access: UUID
        let item: WorkspaceFurnishing
    }

    private struct FurnishingThumbnailKey: Hashable {
        let access: UUID, tenant: String, principal: String, furnishingID: String
        let version: Int, modelBytes: Int
        let modelSHA256: String, recipe: String, tool: String
    }

    enum FurnishingThumbnail {
        case rendering(UUID), ready(NSImage, expires: Date?), failed
    }

    @Published private var furnishingThumbnails: [FurnishingThumbnailKey: FurnishingThumbnail] = [:]
    private var furnishingThumbnailOrder: [FurnishingThumbnailKey] = []
    private struct FurnishingThumbnailLookupKey: Hashable {
        let access: UUID, tenant: String, principal: String, furnishingID: String
        let version: Int
    }
    @Published private var furnishingThumbnailLookupFailures: Set<FurnishingThumbnailLookupKey> = []
    private var furnishingThumbnailTail: Task<Void, Never>?
    private var furnishingThumbnailTailID: UUID?
    private func furnishingThumbnailLookupKey(_ item: WorkspaceFurnishingSummary) -> FurnishingThumbnailLookupKey? {
        guard workspaceFurnishingIntakeAvailable, item.rightsAvailability.display, let state else { return nil }
        return .init(access: viewportAccessID, tenant: state.tenantId, principal: state.principalId, furnishingID: item.id, version: item.version)
    }

    private func furnishingThumbnailKey(_ registration: RegisteredFurnishing) -> FurnishingThumbnailKey? {
        guard registration.access == viewportAccessID, let state else { return nil }
        let item = registration.item, model = item.details.model
        return .init(access: registration.access, tenant: state.tenantId, principal: state.principalId,
                     furnishingID: item.id, version: item.version, modelBytes: model.bytes, modelSHA256: model.sha256,
                     recipe: ViewportFurnishingThumbnailRequest.recipe, tool: ViewportFurnishingThumbnailRequest.tool)
    }

    func furnishingThumbnail(for item: WorkspaceFurnishingSummary) -> FurnishingThumbnail? {
        guard let lookup = furnishingThumbnailLookupKey(item) else { return nil }
        guard let key = furnishingThumbnailOrder.last(where: {
                  $0.access == lookup.access && $0.tenant == lookup.tenant && $0.principal == lookup.principal
                      && $0.furnishingID == item.id && $0.version == item.version
              }) else { return furnishingThumbnailLookupFailures.contains(lookup) ? .failed : nil }
        return furnishingThumbnails[key]
    }

    private let furnishingThumbnailReviews = FurnishingThumbnailReviewSessions()
    var durableFurnishingThumbnailsAvailable: Bool {
        workspaceFurnishingIntakeAvailable && session?.workspaceFurnishingThumbnailProfile == WorkspaceFurnishingThumbnailSubmission.kind
            && !infoStandPreviewing
    }
    func makeFurnishingThumbnailReview(_ summary: WorkspaceFurnishingSummary) -> FurnishingThumbnailReviewModel? {
        guard durableFurnishingThumbnailsAvailable, !busy, summary.rightsAvailability.display,
              let api, let owner = state else { return nil }
        let access = viewportAccessID
        let document: () -> NativeDocumentState? = { [weak self] in
            guard let self, self.api === api, self.viewportAccessID == access, !self.infoStandPreviewing,
                  let current = self.state, current.workspaceId == owner.workspaceId,
                  current.tenantId == owner.tenantId, current.principalId == owner.principalId else { return nil }
            return current
        }
        let capture: () async throws -> WorkspaceFurnishingThumbnailSubmission = { [weak self] in
            guard let self, let base = document(), self.furnishingThumbnailFrameReady,
                  let draft = base.draft, let etag = base.etag else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
            let item = try await api.workspaceFurnishing(summary.id, version: summary.version)
            let (request, bytes) = try await self.viewportController.furnishingThumbnailCapture(item, draft: draft)
            let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
            guard let latest = document(), try encoder.encode(latest) == encoder.encode(base) else {
                throw AuthoringFailure("NATIVE_DOCUMENT_STATE_CHANGED")
            }
            let raw: DocumentValue = .object(["workspaceId": .string(base.workspaceId), "nativeRevision": .integer(Int64(base.revision)),
                "editVersion": draft.document["editVersion"], "baseETag": .string(etag),
                "request": try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)),
                "png": .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
                    "bytes": .integer(Int64(bytes.count)), "sha256": .string(SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined())])])
            return try WorkspaceFurnishingThumbnailSubmission(capture: raw, pngData: bytes, document: base, furnishing: item)
        }
        let refreshed: () async -> Void = { [weak self] in
            guard let self, document() != nil else { return }
            await self.loadFurnishingThumbnail(summary, retry: true)
        }
        let key = FurnishingThumbnailReviewSessions.Key(owner: owner, furnishingId: summary.id)
        if let retained = furnishingThumbnailReviews.existing(key) {
            guard retained.rebind(name: summary.name, api: api, document: document, capture: capture, refreshed: refreshed) else { return nil }
            return retained
        }
        let review = FurnishingThumbnailReviewModel(name: summary.name, furnishingId: summary.id, owner: owner, api: api,
            document: document, capture: capture, refreshed: refreshed)
        guard furnishingThumbnailReviews.retain(review, for: key) else {
            notice = "확인 중인 이미지 요청을 먼저 마친 뒤 다른 이미지를 준비하세요."; return nil
        }
        return review
    }
    private func loadSelectedFurnishingThumbnail(_ summary: WorkspaceFurnishingSummary) async throws -> Bool {
        guard let api, let owner = state, let lookup = furnishingThumbnailLookupKey(summary) else { return false }
        guard await api.admitsWorkspaceFurnishingThumbnails else { return false }
        let item = try await api.workspaceFurnishing(summary.id, version: summary.version)
        guard let selected = try await api.selectedWorkspaceFurnishingThumbnail(summary.id, version: summary.version, in: owner) else { return false }
        let latest = try await api.workspaceFurnishing(summary.id, version: summary.version)
        guard self.api === api, furnishingThumbnailLookupKey(summary) == lookup,
              state?.workspaceId == owner.workspaceId, item == latest, item.details.rights.permissions.display,
              let key = furnishingThumbnailKey(.init(access: lookup.access, item: item)),
              let image = NSImage(data: selected.data) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        var expires: Date?
        if let text = item.details.rights.expiresAt {
            let formatter = ISO8601DateFormatter(); expires = formatter.date(from: text)
            if expires == nil { formatter.formatOptions.insert(.withFractionalSeconds); expires = formatter.date(from: text) }
            guard let expires, expires > .now else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        }
        furnishingThumbnailOrder.removeAll { $0 == key }; furnishingThumbnailOrder.append(key)
        if furnishingThumbnailOrder.count > 16 { furnishingThumbnails.removeValue(forKey: furnishingThumbnailOrder.removeFirst()) }
        furnishingThumbnails[key] = .ready(image, expires: expires)
        furnishingThumbnailLookupFailures.remove(lookup)
        return true
    }

    /// Existing registry cards need the same current descriptor/rights binding
    /// as a live intake return. Read it without creating or saving an intake.
    func loadFurnishingThumbnail(_ summary: WorkspaceFurnishingSummary, retry: Bool) async {
        let previous = furnishingThumbnailTail, id = UUID(), access = viewportAccessID
        let job = Task { [weak self] in
            await previous?.value
            guard !Task.isCancelled, let self, self.viewportAccessID == access else { return }
            await self.loadExistingFurnishingThumbnail(summary, retry: retry)
        }
        furnishingThumbnailTail = job; furnishingThumbnailTailID = id
        await withTaskCancellationHandler { await job.value } onCancel: { job.cancel() }
        if furnishingThumbnailTailID == id { furnishingThumbnailTail = nil; furnishingThumbnailTailID = nil }
    }

    private func loadExistingFurnishingThumbnail(_ summary: WorkspaceFurnishingSummary, retry: Bool) async {
        if durableFurnishingThumbnailsAvailable {
            do { if try await loadSelectedFurnishingThumbnail(summary) { return } }
            catch {
                guard let lookup = furnishingThumbnailLookupKey(summary), !Task.isCancelled else { return }
                let stale = furnishingThumbnailOrder.filter { $0.access == lookup.access && $0.furnishingID == summary.id && $0.version == summary.version }
                for key in stale { furnishingThumbnails.removeValue(forKey: key) }
                furnishingThumbnailOrder.removeAll { stale.contains($0) }
                furnishingThumbnailLookupFailures.insert(lookup)
                return
            }
        }
        guard furnishingThumbnailFrameReady, viewportController.furnishingThumbnailSupported,
              let lookup = furnishingThumbnailLookupKey(summary), let api else { return }
        switch furnishingThumbnail(for: summary) {
        case .ready, .rendering: return
        case .failed where !retry: return
        default: break
        }
        let sequence = viewportController.acceptedSequence, context = viewportController.editingContextGeneration
        func current() -> Bool {
            self.api === api && furnishingThumbnailLookupKey(summary) == lookup && furnishingThumbnailFrameReady
                && viewportController.acceptedSequence == sequence && viewportController.editingContextGeneration == context
        }
        furnishingThumbnailLookupFailures.remove(lookup)
        do {
            let item = try await api.workspaceFurnishing(summary.id, version: summary.version)
            try Task.checkCancellation()
            guard current() else { return }
            guard item.id == summary.id, item.version == summary.version, item.details.kind == summary.kind,
                  item.details.name == summary.name, item.details.rights.permissions.display else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
            await loadFurnishingThumbnail(RegisteredFurnishing(access: lookup.access, item: item), retry: retry)
        } catch {
            guard !Task.isCancelled, current() else { return }
            if furnishingThumbnailLookupFailures.count >= 16, let old = furnishingThumbnailLookupFailures.first {
                furnishingThumbnailLookupFailures.remove(old)
            }
            furnishingThumbnailLookupFailures.insert(lookup)
        }
    }

    var furnishingThumbnailFrameReady: Bool {
        workspaceFurnishingIntakeAvailable && !busy && !immutable && !entrancePreviewActive
            && viewportController.acknowledgesFurnishingThumbnail(draft: viewportDraft, tenantId: state?.tenantId,
                                                                  principalId: state?.principalId)
    }

    func loadFurnishingThumbnail(_ registration: RegisteredFurnishing, retry: Bool) async {
        let controller = viewportController
        guard furnishingThumbnailFrameReady, controller.furnishingThumbnailSupported,
              let key = furnishingThumbnailKey(registration), let api, let draft = viewportDraft else { return }
        switch furnishingThumbnails[key] {
        case .ready: return
        case .failed where !retry: return
        case .rendering where retry: return
        default: break
        }
        let job = UUID(), sequence = controller.acceptedSequence, context = controller.editingContextGeneration
        func current() -> Bool {
            guard case .rendering(let running) = furnishingThumbnails[key], running == job else { return false }
            return self.api === api && furnishingThumbnailKey(registration) == key && viewportDraft == draft
                && controller.acceptedSequence == sequence && controller.editingContextGeneration == context
                && furnishingThumbnailFrameReady
        }
        furnishingThumbnailOrder.removeAll { $0 == key }
        furnishingThumbnailOrder.append(key)
        if furnishingThumbnailOrder.count > 16 {
            furnishingThumbnails.removeValue(forKey: furnishingThumbnailOrder.removeFirst())
        }
        furnishingThumbnails[key] = .rendering(job)
        defer {
            if case .rendering(let running) = furnishingThumbnails[key], running == job {
                furnishingThumbnails.removeValue(forKey: key)
                furnishingThumbnailOrder.removeAll { $0 == key }
            }
        }
        do {
            // This read does not set the editor's global busy flag, which would disable the very frame it needs.
            let item = try await api.workspaceFurnishing(key.furnishingID, version: key.version)
            let rights = item.details.rights
            var expires: Date?
            if let deadline = rights.expiresAt {
                let formatter = ISO8601DateFormatter()
                expires = formatter.date(from: deadline)
                if expires == nil {
                    formatter.formatOptions.insert(.withFractionalSeconds)
                    expires = formatter.date(from: deadline)
                }
                guard let expires, expires > .now else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
            }
            guard item == registration.item, rights.permissions.display else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
            try Task.checkCancellation()
            guard current() else { return }
            let data = try await controller.furnishingThumbnail(item, draft: draft)
            try Task.checkCancellation()
            guard current(), expires.map({ $0 > .now }) ?? true else { return }
            guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
                  let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
                  image.width == 256, image.height == 256 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
            furnishingThumbnails[key] = .ready(NSImage(cgImage: image, size: NSSize(width: 256, height: 256)), expires: expires)
        } catch {
            guard !Task.isCancelled, current() else { return }
            furnishingThumbnails[key] = .failed
        }
    }

    private var catalogImageOrder: [String] = []
    private var catalogThumbnailTail: Task<Void, Never>?
    private var catalogThumbnailTailId: UUID?
    @Published private(set) var catalogThumbnailSaving = false
    @Published private var catalogThumbnailAuthority: CatalogThumbnailAuthority?
    #if DEBUG
    private var catalogThumbnailCaptureManifestSHA256: String?
    #endif

    func invalidateCatalogThumbnailAuthority() {
        #if DEBUG
        catalogThumbnailCaptureManifestSHA256 = nil
        #endif
        guard let catalogThumbnailAuthority else { return }
        catalogThumbnailAuthority.invalidate()
        self.catalogThumbnailAuthority = nil
    }

    func confirmCatalogThumbnailAuthority(api: AuthoringAPI, session: AuthoringSession, credentials: LocalAuthoringSession,
                                          manifestSHA256: String?) {
        invalidateCatalogThumbnailAuthority()
        guard self.api === api, self.session?.csrfToken == session.csrfToken,
              session.advertisesRenderedArtworkThumbnail,
              viewportCredentials?.token == credentials.token,
              state?.tenantId.lowercased() == session.tenantId.lowercased(),
              state?.principalId.lowercased() == session.userId.lowercased() else { return }
        #if DEBUG
        catalogThumbnailCaptureManifestSHA256 = manifestSHA256
        #endif
        catalogThumbnailAuthority = CatalogThumbnailAuthority()
    }
    private struct PendingCatalogThumbnail {
        let access: UUID, requestId: UUID, selectionVersion: Int, rendered: ViewportRenderedThumbnail
        var commitAttempted = false
        #if DEBUG
        var evidence: (CatalogThumbnailValidationCapture, CatalogThumbnailValidationCapture.Reservation)?
        #endif
    }
    @Published private var pendingCatalogThumbnail: PendingCatalogThumbnail?
    @Published private(set) var catalogThumbnailReceipt: NativeRenderedArtworkThumbnailSelectionReceipt?
    var selectedCatalogArtworkSupportsRenderedThumbnail: Bool {
        guard let entry = catalogEntries[catalogArtworkId] else { return false }
        return entry.kind == "sculpture" && entry.primaryMime == "model/gltf-binary"
    }
    private var catalogThumbnailContextIsEditable: Bool {
        session?.advertisesRenderedArtworkThumbnail == true && viewportController.editingContextActive
            && viewportController.connected && !viewportController.boundedReadOnly && viewportController.renderedThumbnailSupported
    }
    private var catalogThumbnailAvailable: Bool {
        catalogThumbnailAuthority != nil && canSelectCatalog && !immutable && !catalogThumbnailSaving
            && catalogThumbnailContextIsEditable && selectedCatalogArtworkSupportsRenderedThumbnail
    }
    private var currentCatalogThumbnail: PendingCatalogThumbnail? {
        guard let value = pendingCatalogThumbnail, value.access == viewportAccessID,
              value.rendered.artworkId == catalogArtworkId,
              catalogEntries[catalogArtworkId]?.revision == value.rendered.revision else { return nil }
        return value
    }
    var hasPreparedCatalogThumbnail: Bool { pendingCatalogThumbnail != nil }
    var preparedCatalogThumbnailPNG: Data? { currentCatalogThumbnail?.rendered.pngData }
    var catalogThumbnailConfirmationAttempted: Bool { pendingCatalogThumbnail?.commitAttempted == true }
    var canPrepareSelectedCatalogThumbnail: Bool { catalogThumbnailAvailable && pendingCatalogThumbnail == nil }
    var canDiscardPreparedCatalogThumbnail: Bool {
        pendingCatalogThumbnail != nil && !catalogThumbnailSaving && !catalogThumbnailConfirmationAttempted
    }
    var canSaveSelectedCatalogThumbnail: Bool {
        guard catalogThumbnailAvailable, currentCatalogThumbnail != nil else { return false }
        #if DEBUG
        if CatalogThumbnailValidationCapture.requested { return currentCatalogThumbnail?.evidence != nil }
        #endif
        return true
    }
    func discardPreparedCatalogThumbnail() {
        guard canDiscardPreparedCatalogThumbnail else { return }
        pendingCatalogThumbnail = nil
    }
    func prepareSelectedCatalogThumbnail() async {
        guard canPrepareSelectedCatalogThumbnail, let entry = catalogEntries[catalogArtworkId],
              let api, let session, let authority = catalogThumbnailAuthority else { return }
        let access = viewportAccessID
        func current() -> Bool {
            catalogThumbnailAuthority === authority && self.api === api && viewportAccessID == access
                && self.session?.csrfToken == session.csrfToken && catalogArtworkId == entry.id
                && self.session?.advertisesRenderedArtworkThumbnail == true
        }
        catalogThumbnailSaving = true
        defer { catalogThumbnailSaving = false }
        do {
            await catalogThumbnailTail?.value
            guard current(), catalogThumbnailContextIsEditable else { return }
            let selection = try await api.artworkThumbnailSelection(artworkId: entry.id, revision: entry.revision,
                                                                          session: session, authority: authority)
            guard current(), catalogThumbnailContextIsEditable else { return }
            guard selection.bindingId == nil else {
                catalogStatus = "이미 선택된 게시용 미리보기가 있습니다. 현재 선택을 먼저 확인하세요."
                return
            }
            let rendered = try await viewportController.renderedThumbnail(artworkId: entry.id, revision: entry.revision)
            guard current(), catalogThumbnailContextIsEditable else { return }
            pendingCatalogThumbnail = PendingCatalogThumbnail(access: access, requestId: UUID(), selectionVersion: selection.selectionVersion, rendered: rendered)
            catalogThumbnailReceipt = nil
            catalogStatus = "미리보기 후보를 확인한 뒤 게시용으로 선택하세요."
        } catch {
            guard current() else { return }
            if (error as? AuthoringFailure)?.status == 401 { invalidateCatalogThumbnailAuthority() }
            catalogStatus = (error as? AuthoringFailure)?.errorDescription ?? "미리보기 후보를 준비하지 못했습니다."
        }
    }
    func saveSelectedCatalogThumbnail() async {
        guard canSaveSelectedCatalogThumbnail, var attempt = currentCatalogThumbnail,
              let api, let session, let authority = catalogThumbnailAuthority else { return }
        let access = viewportAccessID
        func current() -> Bool {
            catalogThumbnailAuthority === authority && self.api === api && viewportAccessID == access
                && self.session?.csrfToken == session.csrfToken
                && self.session?.advertisesRenderedArtworkThumbnail == true
        }
        attempt.commitAttempted = true
        pendingCatalogThumbnail = attempt
        catalogThumbnailSaving = true
        defer { catalogThumbnailSaving = false }
        do {
            let receipt = try await api.saveRenderedArtworkThumbnail(attempt.rendered, requestId: attempt.requestId,
                expectedSelectionVersion: attempt.selectionVersion, session: session, authority: authority)
            guard current() else { return }
            catalogThumbnailReceipt = receipt
            #if DEBUG
            if let (capture, reservation) = attempt.evidence {
                do { try capture.writeSelection(receipt, requestId: attempt.requestId, to: reservation) }
                catch {
                    pendingCatalogThumbnail = nil
                    catalogStatus = "게시용 미리보기는 선택했으나 검증 기록을 저장하지 못했습니다."
                    return
                }
            }
            #endif
            pendingCatalogThumbnail = nil
            catalogStatus = "게시용 미리보기를 선택했습니다."
        } catch {
            guard current() else { return }
            let failure = error as? AuthoringFailure
            if failure?.status == 401 { invalidateCatalogThumbnailAuthority() }
            if let failure, [400, 403, 409, 412, 422].contains(failure.status) { pendingCatalogThumbnail = nil }
            catalogStatus = failure?.errorDescription ?? "선택 결과를 확인하지 못했습니다. 같은 후보로 다시 확인하세요."
        }
    }
    #if DEBUG
    var canCaptureSelectedCatalogThumbnail: Bool {
        guard catalogThumbnailAvailable, let capture = CatalogThumbnailValidationCapture.load() else { return false }
        return capture.canWrite && catalogThumbnailCaptureManifestSHA256 == capture.intent.manifestSHA256
            && capture.intent.artworkId == catalogArtworkId
            && catalogEntries[catalogArtworkId]?.revision == capture.intent.revision
            && !catalogThumbnailConfirmationAttempted
    }
    func captureSelectedCatalogThumbnail() async {
        guard canCaptureSelectedCatalogThumbnail, let capture = CatalogThumbnailValidationCapture.load() else { return }
        if pendingCatalogThumbnail == nil { await prepareSelectedCatalogThumbnail() }
        guard var attempt = currentCatalogThumbnail, attempt.selectionVersion == 0,
              !attempt.commitAttempted, catalogThumbnailAvailable,
              catalogThumbnailCaptureManifestSHA256 == capture.intent.manifestSHA256 else { return }
        do {
            let reservation = try capture.reserve()
            try capture.write(attempt.rendered, to: reservation, requestId: attempt.requestId)
            attempt.evidence = (capture, reservation)
            pendingCatalogThumbnail = attempt
            catalogStatus = "미리보기 후보와 검증 기록을 준비했습니다. 게시용 선택은 별도입니다."
        } catch { catalogStatus = "미리보기 후보의 검증 기록을 확인하지 못했습니다." }
    }
    #endif

    func catalogImageKey(_ entry: ArtworkCatalogEntry) -> String { entry.id + ":" + String(entry.revision) }
    func loadCatalogImage(_ entry: ArtworkCatalogEntry) async {
        let previous = catalogThumbnailTail, jobId = UUID(), access = viewportAccessID
        // Visible cards share one request lane; they cannot overfill the shared producer's bounded queue.
        let job = Task { [weak self] in
            await previous?.value
            guard !Task.isCancelled, let self, self.viewportAccessID == access,
                  self.library.contains(where: { $0.id == entry.id && $0.revision == entry.revision }) else { return }
            await self.loadCatalogImageSerial(entry)
        }
        catalogThumbnailTail = job; catalogThumbnailTailId = jobId
        await withTaskCancellationHandler { await job.value } onCancel: { job.cancel() }
        if catalogThumbnailTailId == jobId { catalogThumbnailTail = nil; catalogThumbnailTailId = nil }
    }
    private func loadCatalogImageSerial(_ entry: ArtworkCatalogEntry) async {
        let key = catalogImageKey(entry), access = viewportAccessID
        guard catalogImages[key] == nil, viewportController.thumbnailSupported else { return }
        do {
            let data = try await viewportController.thumbnail(artworkId: entry.id, revision: entry.revision)
            try Task.checkCancellation()
            guard viewportAccessID == access, library.contains(where: { $0.id == entry.id && $0.revision == entry.revision }) else { return }
            guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
                  let image = CGImageSourceCreateImageAtIndex(source, 0, nil), image.width <= 256, image.height <= 256 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
            if catalogImageOrder.count >= 64, let oldest = catalogImageOrder.first { catalogImages.removeValue(forKey: oldest); catalogImageOrder.removeFirst() }
            catalogImages[key] = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
            catalogImageOrder.removeAll { $0 == key }; catalogImageOrder.append(key); catalogPreviewFailures.remove(key)
        } catch {
            guard !Task.isCancelled, viewportAccessID == access else { return }
            catalogPreviewFailures.insert(key)
            // Keep the first bounded rejection without a redraw/retry loop.
            if !viewportController.diagnostic.hasPrefix("THUMBNAIL ·") {
                viewportController.diagnose("THUMBNAIL", error)
            }
        }
    }
    var canSelectCatalog: Bool { !entrancePreviewActive && !busy && !pending && !formDirty && !positionDirty && !architectureDirty && !architectureApplying && !placingArtwork }
    var visibleCatalog: [ArtworkCatalogEntry] {
        guard let draft else { return [] }
        return library.map { row in
            catalogEntries[row.id] ?? ArtworkCatalogEntry(id: row.id, revision: row.revision, artwork: .object(["metadata": .object(["title": .string(row.title)])]))
        }.filter { $0.matches(query: catalogQuery, filter: catalogFilter, in: draft) }
    }
    @Published private(set) var comparison: NativeStudioResponse?
    @Published private var storedImmutable = false
    /// The frame restriction never grants authority. In selected native mode,
    /// a saved draft remains locked until the exact current frame is acknowledged.
    var immutable: Bool {
        editingLocked(acknowledging: draft)
    }
    private func editingLocked(acknowledging expectedDraft: NativeExhibitionDraft?) -> Bool {
        NativeViewportAccess.locked(selected: selectedBoundedPolicy, saved: state?.etag != nil, storedLocked: storedImmutable || boundedPolicyPending,
            frameReadOnly: viewportController.boundedReadOnly,
            exactOrdinaryFrame: viewportController.acknowledges(draft: expectedDraft, tenantId: state?.tenantId, principalId: state?.principalId))
    }
    // Do not feed the transient pending-ack lock back into the frame's
    // disabled bit: that would create a new frame on every acknowledgement.
    var viewportReadOnly: Bool { (storedImmutable && boundedPlacementCapability == nil) || viewportController.boundedReadOnly }
    var boundedPlacementReady: Bool {
        guard api != nil, let capability = boundedPlacementCapability, state?.etag == capability.etag, state?.pending == nil,
              let selection = viewportSelection, selection.kind == "placement",
              let access = boundedPlacementAccess, viewportAccessID == access,
              let token = boundedPlacementToken, viewportCredentials?.token == token,
              let current = session, let captured = boundedPlacementSession,
              current.tenantId == captured.tenantId, current.userId == captured.userId, current.role == captured.role,
              current.csrfToken == captured.csrfToken, current.expiresAt == captured.expiresAt,
              let imported = oexImport.state, imported.intent == nil, imported.jobId == nil, imported.fileName == nil,
              imported.workspaceId == state?.workspaceId, imported.tenantId == state?.tenantId, imported.principalId == state?.principalId else { return false }
        return viewportController.acknowledgesPlacement(draft: draft, tenantId: state?.tenantId, principalId: state?.principalId,
                                                        selection: selection, capability: capability)
    }
    /// Do not turn an uncertain selected-owner save into a generic PUT retry.
    var boundedPendingRecoveryBlocked: Bool {
        NativeViewportAccess.blocksPendingReplay(selected: selectedBoundedPolicy, pending: pending, origin: state?.pending?.origin)
            || (pending && ordinaryBoundedRuntime)
    }
    @Published private(set) var boundedReadOnly = false
    @Published private(set) var boundedPolicyPending = false
    private var selectedBoundedPolicy = false
    // The workspace kind enforces a lock; the host's exact release admission
    // separately controls whether the scoped policy may enable a placement.
    private var ordinaryBoundedRuntime = false
    private var boundedRuntimeQualified = false
    var boundedNavigationLocked: Bool { ordinaryBoundedRuntime }
    @Published private(set) var boundedPlacementCapability: NativeBoundedEditCapability?
    private var boundedPlacementPolicy: NativeViewportPolicy?
    private var boundedPlacementBaseline: NativeExhibitionDraft?
    private var boundedPlacementSession: AuthoringSession?
    private var boundedPlacementAccess: UUID?
    private var boundedPlacementToken: String?
    private var boundedPlacementEpoch: String?
    private var boundedProfileKnown: Bool?
    private var boundedPolicyGeneration = UUID()
    @Published private(set) var reusablePublicationId: String?
    @Published private(set) var busy = false
    @Published private(set) var failure: AuthoringFailure?
    @Published private(set) var notice = ""
    @Published private(set) var exporting = false
    @Published private(set) var exportProgress: OEXExportDisplayProgress?
    @Published private(set) var exportedURL: URL?
    @Published private(set) var exportedSHA256: String?
    @Published var query = ""
    @Published var publicationWatermark = PublicationWatermarkFormBuffer()
    @Published private(set) var publicationWatermarkReadable = true
    @Published private(set) var publicationWatermarkRuntimeAvailable = false
    @Published private(set) var publicationWatermarkStatus = ""
    func configurePublicationWatermark(_ configuration: NativeConfiguration?, ready: Bool) {
        let available = PublicationWatermarkRuntime.supports(manifestSHA256: configuration?.manifestSha256, ready: ready)
        if publicationWatermarkRuntimeAvailable != available { publicationWatermarkRuntimeAvailable = available }
        // A runtime change never discards text or silently turns it into a new setting.
    }
    var canEditPublicationWatermark: Bool {
        publicationWatermarkRuntimeAvailable && publicationWatermarkReadable && draft != nil && !immutable && !busy && !pending && !placingArtwork && !architectureApplying && !entrancePreviewActive
    }
    var publicationWatermarkValid: Bool { (try? publicationWatermark.settings()) != nil }
    func discardPublicationWatermark() {
        guard !busy, !pending else { return }
        syncPublicationWatermark()
    }
    @Published var title = "" { didSet { if oldValue != title { invalidateEntranceThumbnails() } } }
    @Published var entrance = ViewerEntranceSettings() { didSet { invalidateEntranceThumbnails() } }
    @Published var coverEntry = CoverEntryFormBuffer(design: nil) {
        didSet { if Self.entranceThumbnailContent(oldValue) != Self.entranceThumbnailContent(coverEntry) { invalidateEntranceThumbnails() } }
    }
    private static func entranceThumbnailContent(_ fields: CoverEntryFormBuffer) -> DocumentValue? {
        guard let design = try? fields.value(), case .object(var value) = design.raw else { return nil }
        // Every thumbnail explicitly supplies its own theme/layout/effect pack.
        // Selecting that pack does not stale its image; content changes do.
        for key in ["theme", "layout", "effects"] { value.removeValue(forKey: key) }
        return .object(value)
    }
    @Published var entrancePreviewDevice = EntrancePreviewDevice.desktop { didSet { if oldValue != entrancePreviewDevice { invalidateEntranceThumbnails() } } }
    @Published var entrancePreviewScenario = EntrancePreviewScenario.normal { didSet { if oldValue != entrancePreviewScenario { invalidateEntranceThumbnails() } } }
    @Published var entrancePreviewReducedMotion = false { didSet { if oldValue != entrancePreviewReducedMotion { invalidateEntranceThumbnails() } } }
    @Published var entrancePreviewLocale = "" { didSet { if oldValue != entrancePreviewLocale { invalidateEntranceThumbnails() } } }
    @Published var entranceSystemReducedMotion = false { didSet { if oldValue != entranceSystemReducedMotion { invalidateEntranceThumbnails() } } }
    @Published private(set) var entranceThumbnails: [CoverEntryTheme: Data] = [:]
    private var entranceThumbnailGeneration = UUID()
    private func invalidateEntranceThumbnails() {
        entranceThumbnailGeneration = UUID()
        if !entranceThumbnails.isEmpty { entrancePreviewStatus = "" }
        entranceThumbnails = [:]
    }
    @Published private(set) var entrancePreviewStatus = ""
    @Published private(set) var entrancePreviewBusy = false
    @Published private(set) var entrancePreviewDraft: NativeExhibitionDraft?
    @Published private(set) var entranceHostRecovering = false
    private var entranceRecoveryEpoch: String?
    private var entrancePreviewEpoch: String?
    private var entranceCredentialRenewalPending = false
    private var entrancePreviewBase: NativeExhibitionDraft?
    private var entrancePreviewTask: Task<Void, Never>?
    private var entrancePreviewGeneration = UUID()
    var entrancePreviewActive: Bool { entrancePreviewDraft != nil }
    var canRecoverEntranceHost: Bool {
        guard !entranceCredentialRenewalPending, entrancePreviewActive, !busy, !pending, !placingArtwork, !architectureApplying else { return false }
        if entranceHostRecovering { return !entrancePreviewBusy }
        return !viewportController.connected || !entrancePreviewBusy
    }
    @Published private(set) var entranceEditable = true
    @Published var roomId = ""
    @Published var roomName = ""
    @Published var width = 12.0
    @Published var height = 4.0
    @Published var depth = 8.0
    @Published var artworkId = ""
    @Published var placementId = ""
    @Published var installation = InstallationFormBuffer(settings: nil) { didSet { refreshInstallationPreview() } }
    @Published private(set) var installationEditable = false
    @Published private(set) var installationStatus = ""
    @Published private var installationPreview: NativeExhibitionDraft?
    private var installationBaseline = InstallationFormBuffer(settings: nil)
    let viewportController = NativeViewportController()
    @Published var floorTargetKey = ""
    @Published var floorPreset = FloorSettings.Preset.wood
    @Published var floorScope = Set(FloorSettings.Field.allCases)
    @Published var floor = FloorFormBuffer(settings: nil) { didSet {
        if !floor.materialSoundChosen { floorAudioIntentBound = false }
        else if floorAudioRuntimeAvailable && floor.soundChoiceRevision != oldValue.soundChoiceRevision { floorAudioIntentBound = true }
        refreshFloorPreview()
    } }
    @Published private(set) var floorEditable = false
    @Published private(set) var floorStatus = ""
    @Published private var floorPreview: NativeExhibitionDraft?
    private var floorPreviewBase: NativeExhibitionDraft?
    private var floorPreviewAccessID: UUID?
    private var floorPreviewContext: UUID?
    private var floorPreviewHostEpoch: String?
    @Published private(set) var floorRuntimeAvailable = false
    @Published private(set) var floorAudioRuntimeAvailable = false
    @Published private(set) var privateFloorRuntimeAvailable = false
    @Published private(set) var privateFloorCatalog: NativePrivateFloorCatalog?
    @Published private(set) var privateFloorImage: NSImage?
    @Published private(set) var privateFloorLoadFailed = false
    private var privateFloorRequestID: UUID?
    private var floorAudioIntentBound = false
    @Published private(set) var floorImages: [FloorSettings.Preset: NSImage] = [:]
    private var floorImageAttempts = Set<FloorSettings.Preset>()
    private var floorImagesLoading = false
    private var floorBaseline = FloorFormBuffer(settings: nil)
    var floorDirty: Bool { floor != floorBaseline }
    var floorCanReset: Bool { floorBaseline.settings != nil && floor.settings?.preset == floorBaseline.settings?.preset && !floor.restoreLegacy }
    var floorTargets: [FloorSettings.Target] { (try? draft.map { try FloorSettings.targets(in: $0.candidate) }) ?? [] }
    var floorTarget: FloorSettings.Target? { floorTargets.first { $0.key == floorTargetKey } }
    var floorCurrentValues: [FloorSettings.Field: DocumentValue] {
        guard let draft, let target = floorTarget else { return [:] }
        let legacySound = draft.candidate["extensions"]["org.exhibitos.viewer/experience"]["footsteps"].items?.first(where: { target.kind == .surface && $0["surfaceId"].string?.lowercased() == target.id.lowercased() })
        if let settings = (try? floor.value(candidate: draft.candidate)) ?? floor.settings {
            var values = (try? settings.effectiveAppearance(candidate: draft.candidate)) ?? [:]
            values[.sound] = settings.value["sound"] == .null ? legacySound : settings.value["sound"]
            return values
        }
        var values: [FloorSettings.Field: DocumentValue] = [:]
        if target.kind == .surface {
            if case .object(let materials) = draft.candidate["extensions"]["org.exhibitos.studio/materials"]["surfaces"],
               let material = materials.first(where: { $0.key.lowercased() == target.id.lowercased() })?.value {
                for field in [FloorSettings.Field.color, .roughness, .metalness] { values[field] = material[field.rawValue] }
            }
            values[.sound] = draft.candidate["extensions"]["org.exhibitos.viewer/experience"]["footsteps"].items?.first(where: { $0["surfaceId"].string?.lowercased() == target.id.lowercased() })
        }
        return values
    }

    var floorCustomized: Set<FloorSettings.Field> {
        guard let draft, let floorTarget else { return [] }
        return (try? FloorSettings.customizedFields(target: floorTarget, candidate: draft.candidate)) ?? Set(FloorSettings.Field.allCases)
    }
    var installationDirty: Bool { installation != installationBaseline }
    var viewportDraft: NativeExhibitionDraft? {
        if entranceHostRecovering { return draft }
        if entrancePreviewBase == draft, let value = entrancePreviewDraft { return value }
        if viewingPointInput != nil { return viewingPointPreview ?? draft }
        if let value = checkedInfoStandPreview?.displayDraft { return value }
        if let value = furnishingPreview?.displayDraft { return value }
        if let value = sharedFurnishingPreview?.displayDraft { return value }
        if let value = spaceLightingPreview?.displayDraft { return value }
        if let value = plaquePreview?.displayDraft { return value }
        if let value = benchPreview?.displayDraft { return value }
        if let value = lightPreview?.displayDraft { return value }
        if let value = wallFinishPreview?.displayDraft { return value }
        if let value = surfaceMaterialPreview?.displayDraft { return value }
        if let value = surfaceMaterialTexturePreview?.displayDraft { return value }
        if floorDirty { return floorPreview ?? draft }
        return installationDirty ? (installationPreview ?? draft) : draft
    }
    @Published var positionXText = "0.0"
    @Published var positionYText = "1.0"
    @Published var positionZText = "0.0"
    // The text is the single editable buffer, including incomplete numeric input.
    private func positionNumber(_ text: String) -> Double { Double(text.trimmingCharacters(in: .whitespacesAndNewlines)) ?? .nan }
    var x: Double { get { positionNumber(positionXText) } set { positionXText = String(newValue) } }
    var y: Double { get { positionNumber(positionYText) } set { positionYText = String(newValue) } }
    var z: Double { get { positionNumber(positionZText) } set { positionZText = String(newValue) } }
    @Published private(set) var cursor: String?
    @Published var viewportPresentation: ViewportPresentation = .threeD
    @Published var viewportMode = "translate"
    @Published private(set) var viewportView = "isometric"
    @Published private(set) var viewportCameraSequence = 0
    @Published var viewportTransparent = true
    func requestViewportView(_ view: String) { guard !entrancePreviewActive, !placingArtwork else { return }; viewportView = view; viewportCameraSequence += 1 }
    @Published var viewportSelection: ViewportSelection?
    @Published var architecture: ArchitectureFormBuffer?
    private var architectureBaseline: ArchitectureFormBuffer?
    @Published private(set) var architectureStatus = ""
    @Published private(set) var architectureApplying = false
    var architectureGeometryDirty: Bool { architecture != architectureBaseline }
    var architectureDirty: Bool { architectureGeometryDirty || surfaceMaterialDirty || wallFinishDirty || lightDirty || benchDirty || plaqueDirty || spaceLightingDirty || furnishingDirty || viewingPlanDirty }
    @Published var viewingPlan: ViewingPlanFormBuffer? {
        didSet { viewingPlanPreview = nil; viewingPlanPreviewAccessID = nil; viewingPlanStatus = "" }
    }
    private var viewingPlanBaseline: ViewingPlanFormBuffer?
    private var viewingPlanPreview: ViewingPlanPreview?
    private var viewingPlanPreviewAccessID: UUID?
    private var viewingPlanPreviewContext: UUID?
    private var viewingPlanPreviewHostEpoch: String?
    private var viewingPlanPreviewSequence: Int?
    @Published private(set) var viewingPlanStatus = ""
    var viewingPlanDirty: Bool { viewingPlan != viewingPlanBaseline }
    var viewingPlanPreviewing: Bool { viewingPlanPreview != nil }
    var canEditViewingPlan: Bool {
        !entrancePreviewActive && !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty &&
        !architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty &&
        !spaceLightingDirty && !furnishingDirty && !architectureApplying && viewingPlan != nil &&
        viewportController.editingContextActive && viewportController.connected && viewportController.viewingPlanSupported
    }
    var canPreviewViewingPlan: Bool {
        guard canEditViewingPlan, viewingPlanDirty, !viewingPlanPreviewing, let draft,
              let command = try? viewingPlan?.command(in: draft), let candidate = try? command.proposedCandidate(in: draft) else { return false }
        return candidate != draft.candidate
    }
    var canApplyViewingPlan: Bool {
        guard canEditViewingPlan, viewingPlanPreviewAccessID == viewportAccessID,
              viewingPlanPreviewContext == viewportController.editingContextGeneration,
              viewingPlanPreviewHostEpoch == viewportController.hostEpoch,
              viewingPlanPreviewSequence == viewportController.acceptedSequence,
              let preview = viewingPlanPreview, let draft, draft == preview.base else { return false }
        return (try? viewingPlan?.command(in: draft)) == preview.command
    }
    @Published var viewingPointInput: ViewingPointFormBuffer? {
        didSet { clearViewingPointPreview(); viewingPointStatus = "" }
    }
    private var viewingPointBase: NativeExhibitionDraft?
    @Published private var viewingPointPreview: NativeExhibitionDraft?
    private var viewingPointAccessID: UUID?
    private var viewingPointContext: UUID?
    private var viewingPointHostEpoch: String?
    @Published private(set) var viewingPointStatus = ""
    var canBeginViewingPoint: Bool {
        canEditViewingPlan && !viewingPlanDirty && viewingPointInput == nil &&
        (draft.flatMap { try? BenchViewpoint.list(in: $0).count }.map { $0 < 64 } ?? false)
    }
    var canEditViewingPoint: Bool {
        viewingPointInput != nil && draft == viewingPointBase && !entrancePreviewActive && !busy && !pending && !immutable &&
        !placingArtwork && !floorDirty && !nonFloorFormDirty && !positionDirty && !architectureDirty && !architectureApplying &&
        viewportController.editingContextActive && viewportController.viewingPlanSupported
    }
    var canPreviewViewingPoint: Bool {
        guard canEditViewingPoint, viewportController.connected, viewingPointPreview == nil, let fields = viewingPointInput, let draft else { return false }
        return (try? fields.candidate(in: draft)) != nil
    }
    private var checkedViewingPointPreview: NativeExhibitionDraft? {
        guard !immutable, !pending, !entrancePreviewActive, !placingArtwork, !floorDirty, !nonFloorFormDirty,
              !positionDirty, !architectureDirty, !architectureApplying,
              let base = viewingPointBase, draft == base, let fields = viewingPointInput,
              let preview = viewingPointPreview, preview.candidate == (try? fields.candidate(in: base)),
              viewingPointAccessID == viewportAccessID, viewingPointContext == viewportController.editingContextGeneration,
              viewingPointHostEpoch == viewportController.hostEpoch, viewportController.editingContextActive,
              viewportController.viewingPlanSupported, let credentials = viewportCredentials,
              viewportController.confirmsMetadataPreview(preview, tenantId: credentials.tenantId, principalId: credentials.principalId) else { return nil }
        return preview
    }
    var canApplyViewingPoint: Bool { canEditViewingPoint && checkedViewingPointPreview != nil }
    private func clearViewingPointPreview() {
        viewingPointPreview = nil; viewingPointAccessID = nil; viewingPointContext = nil; viewingPointHostEpoch = nil
    }
    private func resetViewingPointInput() {
        viewingPointInput = nil; viewingPointBase = nil; clearViewingPointPreview()
    }
    func beginViewingPoint() {
        guard canBeginViewingPoint, let draft, let room = draft.rooms.first(where: { $0["id"].string == roomId }) ?? draft.rooms.first,
              let id = room["id"].string else { return }
        do { viewingPointBase = draft; viewingPointInput = try ViewingPointFormBuffer(draft: draft, roomId: id) }
        catch { viewingPointStatus = "감상 시점 입력을 열지 못했습니다. 기존 전시는 유지됩니다." }
    }
    func discardViewingPoint() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        resetViewingPointInput()
    }
    func previewViewingPoint() {
        guard canPreviewViewingPoint, let fields = viewingPointInput, let base = viewingPointBase,
              let epoch = viewportController.hostEpoch else { return }
        do {
            var preview = base; try preview.replaceCandidate(fields.candidate(in: base))
            viewingPointAccessID = viewportAccessID; viewingPointContext = viewportController.editingContextGeneration
            viewingPointHostEpoch = epoch; viewingPointPreview = preview
            viewingPointStatus = "시점 형식을 확인하고 있습니다. 시점 추가가 활성화되면 보관할 수 있습니다. 관람 중 바닥·장애물 안전은 별도로 확인합니다."
        } catch { viewingPointStatus = "이름·위치·방향·시야각을 확인하세요. 입력과 기존 시점은 유지됩니다." }
    }
    func applyViewingPoint() async {
        guard canApplyViewingPoint else { return }
        await work(allowViewingPointInput: true) {
            guard let preview = checkedViewingPointPreview, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = preview; try await persist(value); syncFields()
            notice = "감상 시점을 이 Mac에 보관했습니다. 관람 순서에서 선택하고 전시를 저장하세요."
        }
    }
    @Published var furnishing: FurnishingFormBuffer? { didSet { furnishingPreview = nil; furnishingPreviewAccessID = nil; furnishingStatus = "" } }
    private var furnishingBaseline: FurnishingFormBuffer?
    @Published private var furnishingPreview: FurnishingPreview?
    private var furnishingPreviewAccessID: UUID?
    @Published private(set) var furnishingStatus = ""
    @Published private(set) var furnishingCatalogReady = false
    @Published private(set) var furnishingCatalogStatus = ""
    @Published private(set) var furnishingImage: NSImage?
    @Published var furnishingQuery = ""
    @Published var furnishingCategory = "pedestal"
    private var furnishingCatalogAccessID: UUID?
    private var furnishingCatalogLoading = false
    private var furnishingImageAttempted = false
    var furnishingDirty: Bool { furnishing != furnishingBaseline || sharedFurnishing != sharedFurnishingBaseline }
    var furnishingPreviewing: Bool { furnishingPreview != nil || sharedFurnishingPreview != nil }
    var selectedHasFurnishing: Bool {
        guard let draft, !placementId.isEmpty else { return false }
        do { return try SharedFurnishingPreview.pose(in: draft, host: .pedestal, id: placementId) != nil }
        catch { return FurnishingCatalog.hasVisual(in: draft, hostId: placementId) }
    }
    var canEditFurnishing: Bool {
        !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !spaceLightingDirty && !architectureApplying && sharedFurnishing == sharedFurnishingBaseline && viewportController.connected && viewportController.furnishingVisualsSupported && furnishingCatalogReady && furnishing != nil
    }
    var canPreviewFurnishing: Bool { canEditFurnishing && furnishingDirty && !furnishingPreviewing && (furnishing?.selected == true || selectedHasFurnishing) }
    var canApplyFurnishing: Bool {
        guard canEditFurnishing, furnishingPreviewAccessID == viewportAccessID, let preview = furnishingPreview, preview.base == draft else { return false }
        return (try? furnishing?.command(hostId: placementId)) == preview.command
    }
    private func syncFurnishing() {
        furnishing = nil; furnishingBaseline = nil; furnishingStatus = ""
        guard let draft, !placementId.isEmpty else { return }
        do {
            let fields = try FurnishingFormBuffer(draft: draft, hostId: placementId)
            furnishing = fields; furnishingBaseline = fields
        } catch { furnishingStatus = "이 배치의 비품 설정을 읽지 못했습니다. 원래 설정은 유지됩니다." }
    }
    func discardFurnishing() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        syncFurnishing()
    }
    func loadFurnishingCatalog() async {
        guard viewportController.connected, viewportController.furnishingVisualsSupported, !furnishingCatalogLoading, furnishingCatalogAccessID != viewportAccessID, let api else { return }
        furnishingCatalogLoading = true
        defer { furnishingCatalogLoading = false }
        let access = viewportAccessID
        do {
            let available = try await api.furnishings()
            try Task.checkCancellation()
            guard access == viewportAccessID, viewportController.connected, viewportController.furnishingVisualsSupported else { return }
            furnishingCatalogReady = available; furnishingCatalogAccessID = access
            furnishingCatalogStatus = available ? "" : "지금 사용할 수 있는 비품이 없습니다."
        } catch {
            guard !Task.isCancelled, access == viewportAccessID else { return }
            furnishingCatalogAccessID = access
            furnishingCatalogStatus = "비품 목록을 읽지 못했습니다. 연결을 확인한 뒤 다시 확인하세요."
        }
    }
    func retryFurnishingCatalog() async { furnishingCatalogAccessID = nil; await loadFurnishingCatalog() }
    func loadFurnishingImage() async {
        guard furnishingCatalogReady, furnishingImage == nil, !furnishingImageAttempted, viewportController.connected else { return }
        furnishingImageAttempted = true
        let previous = catalogThumbnailTail, jobId = UUID(), access = viewportAccessID
        let job = Task { [weak self] in
            await previous?.value
            guard !Task.isCancelled, let self, access == self.viewportAccessID else { return }
            do {
                let data = try await self.viewportController.furnishingThumbnail()
                try Task.checkCancellation()
                guard access == self.viewportAccessID, self.furnishingCatalogReady else { return }
                guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
                      let image = CGImageSourceCreateImageAtIndex(source, 0, nil), image.width == 256, image.height == 256 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                self.furnishingImage = NSImage(cgImage: image, size: NSSize(width: 256, height: 256))
            } catch {
                guard !Task.isCancelled, access == self.viewportAccessID else { return }
                self.viewportController.diagnose("FURNISHING_THUMBNAIL", error)
            }
        }
        catalogThumbnailTail = job; catalogThumbnailTailId = jobId
        await withTaskCancellationHandler { await job.value } onCancel: { job.cancel() }
        if catalogThumbnailTailId == jobId { catalogThumbnailTail = nil; catalogThumbnailTailId = nil }
    }
    func previewFurnishing() async {
        guard canPreviewFurnishing, let base = draft, let fields = furnishing else { return }
        let access = viewportAccessID, host = placementId
        do {
            let command = try fields.command(hostId: host)
            architectureApplying = true
            defer { architectureApplying = false }
            let preview = try await viewportController.furnishingVisuals(command)
            await work {
                guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty, !surfaceMaterialDirty, !lightDirty, !benchDirty, !plaqueDirty, !spaceLightingDirty,
                      architectureApplying, access == viewportAccessID, host == placementId, base == draft, preview.base == base, furnishing == fields else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                furnishingPreview = preview; furnishingPreviewAccessID = access
                furnishingStatus = command.pose == nil ? "받침 외형을 뺀 상태입니다. 기존 보수적 받침은 남습니다. 적용 전에는 저장되지 않습니다." : "받침을 미리 보고 있습니다. 적용하면 선택한 배치에 보관합니다. 작품 위치는 바뀌지 않습니다."
            }
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("FURNISHING_PREVIEW_FAILED"); viewportController.diagnose("FURNISHING", error) }
    }
    func applyFurnishing() async {
        guard canApplyFurnishing else { return }
        await work {
            guard !pending, !immutable, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty, !surfaceMaterialDirty, !lightDirty, !benchDirty, !plaqueDirty, !spaceLightingDirty, !architectureApplying, furnishingPreviewAccessID == viewportAccessID, viewportController.connected, viewportController.furnishingVisualsSupported, let preview = furnishingPreview, let fields = furnishing, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, command: fields.command(hostId: placementId))
            try await persist(value); syncFields()
            notice = "받침 변경을 이 Mac에 보관했습니다. 한 번의 되돌리기로 복원할 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    @Published var sharedFurnishing: SharedFurnishingFormBuffer? { didSet { sharedFurnishingPreview = nil; sharedFurnishingPreviewAccessID = nil; sharedFurnishingStatus = "" } }
    private var sharedFurnishingBaseline: SharedFurnishingFormBuffer?
    @Published private var sharedFurnishingPreview: SharedFurnishingPreview?
    private var sharedFurnishingPreviewAccessID: UUID?
    @Published private(set) var sharedFurnishingStatus = ""
    @Published private(set) var sharedFurnishingCatalogReady = false
    @Published private(set) var sharedFurnishingCatalog: [DocumentValue] = []
    @Published var sharedFurnishingQuery = ""
    @Published var sharedFurnishingCategory = "all"
    private var sharedFurnishingCatalogAccessID: UUID?
    private var sharedFurnishingCatalogLoading = false
    var sharedFurnishingDirty: Bool { sharedFurnishing != sharedFurnishingBaseline }
    var sharedFurnishingTarget: (SharedFurnishingCommand.Host, String)? {
        if !selectedBenchId.isEmpty { return (.bench, selectedBenchId) }
        if !selectedLightId.isEmpty { return (.light, selectedLightId) }
        if let selection = viewportSelection, selection.kind == "opening" { return (.opening, selection.id) }
        if let selection = viewportSelection, selection.kind == "placement" { return (.pedestal, selection.id) }
        return nil
    }
    var canEditSharedFurnishing: Bool {
        !entrancePreviewActive && !viewingPlanDirty && viewingPointInput == nil && !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !spaceLightingDirty && !architectureApplying && furnishing == furnishingBaseline && viewportController.editingContextActive && viewportController.connected && viewportController.sharedFurnishingVisualsSupported && sharedFurnishingCatalogReady && sharedFurnishingCatalogAccessID == viewportAccessID && sharedFurnishing != nil
    }
    var canPreviewSharedFurnishing: Bool { canEditSharedFurnishing && sharedFurnishing != sharedFurnishingBaseline && sharedFurnishingPreview == nil && (sharedFurnishing?.selected == true || sharedFurnishingBaseline?.selected == true) }
    var canApplySharedFurnishing: Bool {
        guard canEditSharedFurnishing, sharedFurnishingPreviewAccessID == viewportAccessID, let preview = sharedFurnishingPreview,
              preview.base == draft, let form = sharedFurnishing, let target = sharedFurnishingTarget,
              form.host == target.0, form.hostId == target.1 else { return false }
        return (try? form.command()) == preview.command
    }
    func syncSharedFurnishing() {
        guard !busy, !architectureApplying, sharedFurnishing == sharedFurnishingBaseline else { return }
        sharedFurnishing = nil; sharedFurnishingBaseline = nil; sharedFurnishingStatus = ""
        guard let draft, let target = sharedFurnishingTarget else { return }
        do { let form = try SharedFurnishingFormBuffer(draft: draft, host: target.0, hostId: target.1); sharedFurnishing = form; sharedFurnishingBaseline = form }
        catch { sharedFurnishingStatus = "비품 설정을 읽지 못했습니다. 기존 설정은 유지됩니다." }
    }
    func discardSharedFurnishing() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        sharedFurnishing = sharedFurnishingBaseline
    }
    func loadSharedFurnishingCatalog() async {
        guard viewportController.connected, viewportController.sharedFurnishingVisualsSupported, !sharedFurnishingCatalogLoading,
              sharedFurnishingCatalogAccessID != viewportAccessID, let api else { return }
        sharedFurnishingCatalogLoading = true; defer { sharedFurnishingCatalogLoading = false }
        sharedFurnishingCatalogReady = false; sharedFurnishingCatalog = []
        let access = viewportAccessID
        do {
            let items = try await api.sharedFurnishings(); try Task.checkCancellation()
            guard access == viewportAccessID, viewportController.connected, viewportController.sharedFurnishingVisualsSupported else { return }
            sharedFurnishingCatalog = items
            sharedFurnishingCatalogReady = true; sharedFurnishingCatalogAccessID = access
            if sharedFurnishingStatus == "비품 목록을 확인하지 못했습니다. 기존 설정은 유지됩니다." { sharedFurnishingStatus = "" }
        } catch {
            guard !Task.isCancelled, access == viewportAccessID else { return }
            sharedFurnishingCatalogAccessID = access; sharedFurnishingStatus = "비품 목록을 확인하지 못했습니다. 기존 설정은 유지됩니다."
        }
    }
    func retrySharedFurnishingCatalog() async { sharedFurnishingCatalogAccessID = nil; await loadSharedFurnishingCatalog() }
    func previewSharedFurnishing() async {
        guard canPreviewSharedFurnishing, let base = draft, let form = sharedFurnishing, let target = sharedFurnishingTarget,
              target.0 == form.host, target.1 == form.hostId else { return }
        let access = viewportAccessID
        do {
            let command = try form.command(); architectureApplying = true; defer { architectureApplying = false }
            let selected = (target.0 == .bench || target.0 == .light) ? target.1 : nil
            let preview = try await viewportController.sharedFurnishingVisuals(command, selectedInspectorId: selected)
            await work {
                guard architectureApplying, access == viewportAccessID, base == draft, preview.base == base, sharedFurnishing == form,
                      let current = sharedFurnishingTarget, current.0 == target.0, current.1 == target.1,
                      !pending, !immutable, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty, !surfaceMaterialDirty, !lightDirty, !benchDirty, !plaqueDirty, !spaceLightingDirty else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                sharedFurnishingPreview = preview; sharedFurnishingPreviewAccessID = access
                sharedFurnishingStatus = "미리보기입니다. 적용 전에는 전시에 보관되지 않습니다."
            }
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("FURNISHING_PREVIEW_FAILED"); viewportController.diagnose("FURNISHING_V2", error) }
    }
    func applySharedFurnishing() async {
        guard canApplySharedFurnishing else { return }
        await work {
            guard let preview = sharedFurnishingPreview, let form = sharedFurnishing, let draft, var value = state,
                  let target = sharedFurnishingTarget, target.0 == form.host, target.1 == form.hostId,
                  sharedFurnishingPreviewAccessID == viewportAccessID, viewportController.connected, viewportController.sharedFurnishingVisualsSupported else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, command: form.command()); try await persist(value)
            sharedFurnishingBaseline = form; syncFields()
            notice = "비품 변경을 이 Mac에 보관했습니다. 전시 저장은 별도로 진행합니다."
        }
        syncSharedFurnishing()
    }
    @Published var wallFinish: WallFinishFormBuffer? { didSet { clearWallFinishPreview() } }
    private var wallFinishBaseline: WallFinishFormBuffer?
    @Published private var wallFinishPreview: WallFinishPreview?
    private var wallFinishPreviewAccessID: UUID?
    private var wallFinishPreviewContext: UUID?
    private var wallFinishPreviewHost: String?
    @Published private(set) var wallFinishStatus = ""
    var wallFinishDirty: Bool { wallFinish != wallFinishBaseline }
    var wallFinishPreviewing: Bool { wallFinishPreview != nil }
    private var wallFinishContextEditable: Bool {
        !entrancePreviewActive && !pending && !immutable && !placingArtwork && viewingPointInput == nil
            && !floorDirty && !nonFloorFormDirty && !positionDirty && !architectureGeometryDirty
            && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !spaceLightingDirty
            && !furnishingDirty && !viewingPlanDirty && viewportController.editingContextActive
            && viewportController.connected && viewportController.wallFinishSupported
            && viewportSelection?.kind == "surface" && viewportSelection?.id == wallFinish?.surfaceId && wallFinish != nil
    }
    var canEditWallFinish: Bool { !busy && !architectureApplying && wallFinishContextEditable }
    var canPreviewWallFinish: Bool { canEditWallFinish && wallFinishDirty && wallFinishPreview == nil }
    private var checkedWallFinishPreview: WallFinishPreview? {
        guard wallFinishContextEditable, let preview = wallFinishPreview, preview.base == draft,
              let fields = wallFinish, (try? fields.command()) == preview.command,
              wallFinishPreviewAccessID == viewportAccessID,
              wallFinishPreviewContext == viewportController.editingContextGeneration,
              wallFinishPreviewHost == viewportController.hostEpoch, let credentials = viewportCredentials,
              viewportController.confirmsMetadataPreview(preview.displayDraft, tenantId: credentials.tenantId, principalId: credentials.principalId) else { return nil }
        return preview
    }
    var canApplyWallFinish: Bool { canEditWallFinish && checkedWallFinishPreview != nil }
    private func clearWallFinishPreview() {
        wallFinishPreview = nil; wallFinishPreviewAccessID = nil; wallFinishPreviewContext = nil; wallFinishPreviewHost = nil; wallFinishStatus = ""
    }
    private func syncWallFinish() {
        guard !wallFinishDirty else { return }
        wallFinish = nil; wallFinishBaseline = nil
        guard let draft, let selection = viewportSelection, selection.kind == "surface",
              draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == selection.id && $0["type"].string == "wall" }) == true else { return }
        do { let fields = try WallFinishFormBuffer(draft: draft, surfaceId: selection.id); wallFinish = fields; wallFinishBaseline = fields }
        catch { wallFinishStatus = "벽 마감 정보를 읽지 못했습니다. 기존 설정은 유지됩니다." }
    }
    func discardWallFinish() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        wallFinish = wallFinishBaseline; syncWallFinish()
    }
    func previewWallFinish() async {
        guard canPreviewWallFinish, let fields = wallFinish, let base = draft else { return }
        let access = viewportAccessID, context = viewportController.editingContextGeneration, host = viewportController.hostEpoch
        do {
            let command = try fields.command(); architectureApplying = true; defer { architectureApplying = false }
            let preview = try await viewportController.wallFinish(command)
            guard architectureApplying, wallFinishContextEditable, viewportAccessID == access,
                  viewportController.editingContextGeneration == context, viewportController.hostEpoch == host,
                  draft == base, preview.base == base, wallFinish == fields else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            wallFinishPreview = preview; wallFinishPreviewAccessID = access; wallFinishPreviewContext = context; wallFinishPreviewHost = host
            wallFinishStatus = "확인한 벽 마감을 미리 보고 있습니다. 문과 작품의 여유 공간을 확인한 뒤 적용하세요."
        } catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure("WALL_FINISH_PREVIEW_FAILED")
            viewportController.diagnose("WALL_FINISH", error)
        }
    }
    func applyWallFinish() async {
        guard canApplyWallFinish else { return }
        await work {
            guard !architectureApplying, let preview = checkedWallFinishPreview, let fields = wallFinish,
                  let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, command: fields.command(), selectedSurfaceId: fields.surfaceId)
            try await persist(value); wallFinishBaseline = wallFinish; syncFields()
            notice = "벽 마감을 이 Mac에 보관했습니다. 되돌리기로 복원할 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    @Published var spaceLighting: SpaceLightingFormBuffer? { didSet { spaceLightingPreview = nil; spaceLightingPreviewAccessID = nil; spaceLightingStatus = "" } }
    private var spaceLightingBaseline: SpaceLightingFormBuffer?
    @Published private var spaceLightingPreview: SpaceLightingPreview?
    private var spaceLightingPreviewAccessID: UUID?
    @Published private(set) var spaceLightingStatus = ""
    var spaceLightingDirty: Bool { spaceLighting != spaceLightingBaseline }
    var spaceLightingPreviewing: Bool { spaceLightingPreview != nil }
    var canEditSpaceLighting: Bool {
        !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !architectureApplying && viewportController.connected && viewportController.spaceSettingsSupported
    }
    var canPreviewSpaceLighting: Bool {
        guard canEditSpaceLighting, spaceLightingDirty, !spaceLightingPreviewing else { return false }
        if let draft, let command = try? spaceLighting?.command(), let candidate = try? command.proposedCandidate(in: draft), candidate == draft.candidate { return false }
        return true
    }
    var canApplySpaceLighting: Bool {
        guard canEditSpaceLighting, spaceLightingPreviewAccessID == viewportAccessID, let preview = spaceLightingPreview,
              preview.base == draft, let fields = spaceLighting else { return false }
        return (try? fields.command()) == preview.command
    }
    @Published private(set) var selectedLightId = ""
    @Published var light: LightFormBuffer? { didSet { spaceLightingPreview = nil; spaceLightingPreviewAccessID = nil; lightPreview = nil; lightPreviewAccessID = nil; lightStatus = "" } }
    private var lightBaseline: LightFormBuffer?
    @Published private var lightPreview: LightPreview?
    private var lightPreviewAccessID: UUID?
    @Published private(set) var lightStatus = ""
    var lightDirty: Bool { light != lightBaseline }
    var lightPreviewing: Bool { lightPreview != nil }
    var canSelectLight: Bool { canSelectCatalog }
    var canPreviewLight: Bool {
        !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !surfaceMaterialDirty && !benchDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && !architectureApplying && lightDirty && !lightPreviewing && viewportController.connected && viewportController.lightInspectorSupported
            && (light?.detailsChanged != true || viewportController.lightFixtureSupported)
    }
    var canApplyLight: Bool {
        // The renderer acknowledges the temporary light document while the
        // saved draft and the preview's base remain the source of edit authority.
        guard !busy, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
              !surfaceMaterialDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying, viewportController.connected, viewportController.lightInspectorSupported,
              lightPreviewAccessID == viewportAccessID, let preview = lightPreview, preview.base == draft,
              !editingLocked(acknowledging: preview.displayDraft),
              selectedLightId == preview.command.lightId, let fields = light else { return false }
        guard !preview.command.usesLightDetails || viewportController.lightFixtureSupported else { return false }
        return (try? fields.command()) == preview.command
    }
    @Published private(set) var selectedBenchId = ""
    @Published var bench: BenchFormBuffer? { didSet { benchPreview = nil; benchPreviewAccessID = nil; benchStatus = "" } }
    private var benchBaseline: BenchFormBuffer?
    @Published private(set) var benchAdding = false
    @Published private(set) var benchRemoving = false
    @Published private var benchPreview: BenchPreview?
    private var benchPreviewAccessID: UUID?
    @Published private(set) var benchStatus = ""
    var benchDirty: Bool { benchAdding || benchRemoving || bench != benchBaseline }
    var benchPreviewing: Bool { benchPreview != nil }
    var benches: [BenchSettings] { guard let draft else { return [] }; return (try? BenchSettings.list(in: draft)) ?? [] }
    var benchMetadataEditable: Bool {
        guard let draft else { return false }
        do { _ = try BenchSettings.list(in: draft); _ = try BenchViewpoint.list(in: draft); return true } catch { return false }
    }
    var canEditBench: Bool {
        !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && !architectureApplying && viewportController.connected && viewportController.benchInspectorSupported && benchMetadataEditable
    }
    var canAddBench: Bool { canEditBench && !benchDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && benches.count < 32 && draft?.rooms.contains(where: { $0["id"].string == roomId }) == true }
    var canSelectBench: Bool { canSelectCatalog }
    var canPreviewBench: Bool { canEditBench && benchDirty && !benchPreviewing }
    var canRemoveBench: Bool { canEditBench && !benchDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && !selectedBenchId.isEmpty && !benchAdding }
    var canApplyBench: Bool {
        guard canEditBench, benchPreviewAccessID == viewportAccessID, let preview = benchPreview,
              preview.base == draft, selectedBenchId == preview.command.benchId else { return false }
        return (try? currentBenchCommand()) == preview.command
    }
    @Published private(set) var selectedPlaqueId = ""
    @Published var plaque: PlaqueFormBuffer? { didSet { plaquePreview = nil; plaquePreviewAccessID = nil; plaqueStatus = "" } }
    private var plaqueBaseline: PlaqueFormBuffer?
    @Published private(set) var plaqueAdding = false
    @Published private(set) var plaqueRemoving = false
    @Published private var plaquePreview: PlaquePreview?
    private var plaquePreviewAccessID: UUID?
    @Published private(set) var plaqueStatus = ""
    var plaqueDirty: Bool { plaqueAdding || plaqueRemoving || plaque != plaqueBaseline }
    var plaquePreviewing: Bool { plaquePreview != nil }
    var plaques: [PlaqueSettings] { guard let draft else { return [] }; return (try? PlaqueSettings.list(in: draft)) ?? [] }
    var plaqueMetadataEditable: Bool { guard let draft else { return false }; return (try? PlaqueSettings.list(in: draft)) != nil }
    var canEditPlaque: Bool {
        !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !architectureApplying && viewportController.connected && !furnishingDirty && !spaceLightingDirty && viewportController.plaqueInspectorSupported && plaqueMetadataEditable
    }
    var canAddPlaque: Bool { canEditPlaque && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && plaques.count < 32 && draft?.rooms.contains(where: { $0["id"].string == roomId }) == true }
    var canSelectPlaque: Bool { canSelectCatalog }
    var canPreviewPlaque: Bool { canEditPlaque && plaqueDirty && !plaquePreviewing }
    var canRemovePlaque: Bool { canEditPlaque && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && !selectedPlaqueId.isEmpty && !plaqueAdding }
    var canApplyPlaque: Bool {
        // The renderer acknowledges the temporary plaque frame. The saved
        // base still owns the edit and must match the preview exactly.
        guard !busy, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
              !surfaceMaterialDirty, !lightDirty, !benchDirty, !furnishingDirty, !spaceLightingDirty, !architectureApplying,
              viewportController.connected, viewportController.plaqueInspectorSupported, plaqueMetadataEditable,
              plaquePreviewAccessID == viewportAccessID, let preview = plaquePreview,
              preview.base == draft, !editingLocked(acknowledging: preview.displayDraft),
              selectedPlaqueId == preview.command.plaqueId else { return false }
        return (try? currentPlaqueCommand()) == preview.command
    }
    @Published var surfaceMaterial: SurfaceMaterialFormBuffer? { didSet { clearSurfaceMaterialPreview() } }
    private var surfaceMaterialBaseline: SurfaceMaterialFormBuffer?
    @Published var surfaceMaterialTexture: SurfaceMaterialTextureFormBuffer? { didSet { clearSurfaceMaterialPreview() } }
    private var surfaceMaterialTextureBaseline: SurfaceMaterialTextureFormBuffer?
    @Published private var surfaceMaterialPreview: SurfaceMaterialPreview?
    @Published private var surfaceMaterialTexturePreview: SurfaceMaterialTexturePreview?
    private var surfaceMaterialPreviewAccessID: UUID?
    @Published private(set) var surfaceMaterialStatus = ""
    @Published private(set) var surfaceMaterialCatalog: NativeSurfaceMaterialCatalog?
    @Published private(set) var surfaceMaterialImages: [String: NSImage] = [:]
    @Published private(set) var surfaceMaterialCatalogLoading = false
    @Published private(set) var surfaceMaterialCatalogStatus = ""
    private var surfaceMaterialCatalogAccessID: UUID?
    private var surfaceMaterialCatalogRequestID: UUID?
    private var surfaceMaterialTextureDirty: Bool { surfaceMaterialTexture != surfaceMaterialTextureBaseline }
    var surfaceMaterialDirty: Bool { surfaceMaterial != surfaceMaterialBaseline || surfaceMaterialTextureDirty }
    var surfaceMaterialPreviewing: Bool { surfaceMaterialPreview != nil || surfaceMaterialTexturePreview != nil }
    var surfaceMaterialTexturesSupported: Bool { viewportController.connected && viewportController.surfaceMaterialTexturesSupported && surfaceMaterialTexture != nil }
    private var surfaceMaterialRuntimeSupported: Bool { (surfaceMaterialTextureBaseline?.catalogId == nil && !surfaceMaterialTextureDirty) || surfaceMaterialTexturesSupported }
    var canEditSurfaceMaterial: Bool { !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !lightDirty && !benchDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && !architectureApplying && viewportController.editingContextActive && viewportController.connected && surfaceMaterialRuntimeSupported }
    var canPreviewSurfaceMaterial: Bool { canEditSurfaceMaterial && surfaceMaterialDirty && !surfaceMaterialPreviewing }
    var canApplySurfaceMaterial: Bool {
        guard !busy, !pending, !immutable, !formDirty, !positionDirty, !architectureGeometryDirty, !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying,
              !placingArtwork, viewportController.editingContextActive, viewportController.connected, surfaceMaterialRuntimeSupported,
              surfaceMaterialPreviewAccessID == viewportAccessID else { return false }
        if let preview = surfaceMaterialTexturePreview {
            return surfaceMaterialTexturesSupported && preview.base == draft && preview.command.selection == viewportSelection && (try? currentSurfaceMaterialTextureCommand()) == preview.command
        }
        guard !surfaceMaterialTextureDirty, let preview = surfaceMaterialPreview, preview.base == draft,
              preview.command.selection == viewportSelection, let fields = surfaceMaterial else { return false }
        return (try? fields.command()) == preview.command
    }
    private func clearSurfaceMaterialPreview() {
        surfaceMaterialPreview = nil; surfaceMaterialTexturePreview = nil; surfaceMaterialPreviewAccessID = nil; surfaceMaterialStatus = ""
    }
    private func currentSurfaceMaterialTextureCommand() throws -> SurfaceMaterialTextureCommand {
        guard let fields = surfaceMaterial, let texture = surfaceMaterialTexture else { throw AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
        return .init(surfaceId: fields.surfaceId, material: try fields.command().material, texture: try texture.binding())
    }
    var canApplyArchitecture: Bool { !busy && !pending && !immutable && !formDirty && !positionDirty && !architectureApplying && architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && viewportController.connected }
    @Published private(set) var viewportAccessID = UUID() {
        didSet {
            furnishingThumbnails.removeAll()
            furnishingThumbnailOrder.removeAll()
            furnishingThumbnailLookupFailures.removeAll()
            furnishingThumbnailTail?.cancel()
        }
    }
    private(set) var viewportCredentials: LocalAuthoringSession?
    @Published private var past: [NativeExhibitionDraft] = []
    @Published private var future: [NativeExhibitionDraft] = []
    var canUndo: Bool { !entrancePreviewActive && (!immutable || boundedPlacementReady) && !past.isEmpty && !pending && !busy && !formDirty && !positionDirty && !architectureDirty && !architectureApplying && !placingArtwork }
    var canRedo: Bool { !entrancePreviewActive && (!immutable || boundedPlacementReady) && !future.isEmpty && !pending && !busy && !formDirty && !positionDirty && !architectureDirty && !architectureApplying && !placingArtwork }
    private var api: AuthoringAPI?
    private var session: AuthoringSession?
    private var read: (() async throws -> NativeDocumentState)?
    private var write: ((NativeDocumentState) async throws -> NativeDocumentState)?
    var draft: NativeExhibitionDraft? { state?.draft }
    var pending: Bool { state?.pending != nil }
    // A staged wall command blocks ordinary native/3D edits, while its own
    // negotiated auxiliary request still needs the unchanged editable frame.
    var viewportFormDirty: Bool { viewingPointInput != nil || floorDirty || nonFloorFormDirty }
    var formDirty: Bool { viewportFormDirty || wallFinishDirty || infoStandPreviewing }
    private var nonFloorFormDirty: Bool {
        guard let draft else { return false }
        let entranceDirty = (entranceEditable && ((try? draft.viewerEntranceSettings()) != entrance || coverEntry.isDirty)) || installationDirty || publicationWatermark.isDirty
        guard let room = draft.rooms.first(where: { $0["id"].string == roomId }) else { return title != draft.title || entranceDirty }
        return title != draft.title || entranceDirty || roomName != room["name"].string || width != room["dimensions"]["width"].double || height != room["dimensions"]["height"].double || depth != room["dimensions"]["depth"].double
    }
    var positionDirty: Bool {
        guard [x, y, z].allSatisfy({ $0.isFinite }) else { return true }
        if let p = draft?.placements.first(where: { $0["id"].string == placementId }), let position = p["transform"]["position"].items, position.count == 3 { return x != position[0].double || y != position[1].double || z != position[2].double }
        return !artworkId.isEmpty
    }
    var canExport: Bool { !busy && !placingArtwork && !dirty && state?.etag != nil && draft != nil }
    var canEditRoomAudio: Bool { !busy && roomAudioAvailable }
    private var roomAudioAvailable: Bool {
        !entrancePreviewActive && !immutable && !dirty && !placingArtwork &&
            viewportController.editingContextActive && state?.etag != nil && api != nil
    }

    func roomAudioTracks() async throws -> [ApprovedRoomAudioTrack] {
        guard canEditRoomAudio, let api, let state else { throw AuthoringFailure("ROOM_AUDIO_SAVE_REQUIRED") }
        return try await api.roomAudioTracks(in: state)
    }

    func applyRoomAudio(_ settings: RoomAudioSettings, roomId: String, revision: UInt64) async {
        guard canEditRoomAudio else { return }
        await work {
            guard let api, var value = state, value.revision == revision, self.roomId == roomId else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            let next = try await api.prepareRoomAudio(settings, roomId: roomId, in: value)
            guard roomAudioAvailable, self.api === api, state?.revision == revision,
                  state?.workspaceId == value.workspaceId, state?.tenantId == value.tenantId,
                  state?.principalId == value.principalId, self.roomId == roomId else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            guard let next else { return }
            value.draft = next
            try await persist(value)
            notice = "공간 소리를 이 Mac에 적용했습니다. 전시를 저장하세요."
        }
    }

    var placedExitDoorEditorAvailable: Bool { api?.supportsPlacedExitDoors == true && !immutable }
    var canEditPlacedExitDoor: Bool {
        placedExitDoorEditorAvailable && canExport && !entrancePreviewActive && viewportController.editingContextActive
    }

    func applyPlacedExitDoor(_ settings: PlacedExitDoorSettings, revision: UInt64) async {
        guard canEditPlacedExitDoor else { return }
        await work {
            guard let api, var value = state, value.revision == revision else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            let next = try await api.preparePlacedExitDoor(settings, in: value)
            guard placedExitDoorEditorAvailable, viewportController.editingContextActive, self.api === api,
                  state?.revision == revision, state?.workspaceId == value.workspaceId,
                  state?.tenantId == value.tenantId, state?.principalId == value.principalId else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            guard let next else { return }
            value.draft = next
            try await persist(value)
            notice = "출구 문 목적지를 적용했습니다. 전시를 저장하세요."
        }
    }
    @Published private(set) var sceneSnapshotting = false
    @Published private(set) var sceneSnapshotStatus = ""
    var canCaptureSceneSnapshot: Bool {
        !sceneSnapshotting && canExport && !pending && !entrancePreviewActive && !immutable &&
        viewportPresentation == .threeD && !viewportTransparent && viewportController.editingContextActive &&
        viewportController.connected && viewportController.sceneSnapshotSupported &&
        viewportController.acknowledges(draft: draft, tenantId: state?.tenantId, principalId: state?.principalId)
    }
    private var sceneSnapshotConnectionGeneration = UUID()

    /// In-process API on this model's authenticated, acknowledged live WK host.
    /// No GUI trigger, file panel, document write or selected-target requirement.
    func captureSceneSnapshot(camera: SceneSnapshotCamera, placementId: String) async throws -> SceneSnapshotResult {
        do {
            let image = try await withSceneSnapshot(camera: camera, placementId: placementId) { image, _, _ in image }
            sceneSnapshotStatus = "전시 r\(image.savedRevision)의 시점 이미지를 준비했습니다."
            return image
        } catch {
            sceneSnapshotStatus = "시점 이미지를 준비하지 못했습니다. 기존 전시와 입력은 유지됩니다."
            throw error
        }
    }

    private func withSceneSnapshot<Result>(camera: SceneSnapshotCamera, placementId: String,
        consume: @MainActor (SceneSnapshotResult, NativeStudioResponse, @MainActor () -> Bool) async throws -> Result) async throws -> Result {
        try Task.checkCancellation()
        guard canCaptureSceneSnapshot, !camera.isInspection || viewportController.sceneSnapshotInspectionSupported,
              let original = state, let base = draft, let etag = original.etag,
              original.savedEditVersion == base.document["editVersion"].integer, let api, let session else {
            throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY")
        }
        let access = viewportAccessID, connection = sceneSnapshotConnectionGeneration
        let context = viewportController.editingContextGeneration, host = viewportController.hostEpoch
        let selection = viewportSelection, accepted = viewportController.acceptedSequence
        func unchanged() -> Bool {
            viewportAccessID == access && sceneSnapshotConnectionGeneration == connection && self.api === api &&
            viewportController.editingContextGeneration == context && viewportController.hostEpoch == host &&
            viewportController.acceptedSequence == accepted && viewportController.editingContextActive &&
            viewportController.acknowledges(draft: base, tenantId: original.tenantId, principalId: original.principalId) &&
            state?.revision == original.revision && state?.workspaceId == original.workspaceId &&
            state?.tenantId == original.tenantId && state?.principalId == original.principalId &&
            state?.etag == etag && state?.savedEditVersion == original.savedEditVersion &&
            draft == base && canExport && !pending && !entrancePreviewActive && !immutable &&
            viewportSelection == selection && viewportPresentation == .threeD && !viewportTransparent
        }
        sceneSnapshotting = true; sceneSnapshotStatus = "저장된 전시의 시점 이미지를 준비하고 있습니다."
        // Never mark the frame disabled/busy, stage a candidate or touch undo.
        // Edits, account/store rebinding and newer frames invalidate the result.
        defer { sceneSnapshotting = false }
        _ = try await api.verifyRetainedSession(session)
        let saved = try await api.exhibition(base.id)
        try Task.checkCancellation()
        guard unchanged(), saved.etag == etag, saved.draft == base else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
        let image = try await viewportController.sceneSnapshot(saved: saved, camera: camera, placementId: placementId)
        let current = try await api.exhibition(base.id)
        try saved.requireSameComparison(current)
        _ = try await api.verifyRetainedSession(session)
        try Task.checkCancellation()
        guard unchanged(), image.savedETag == etag, image.savedRevision == saved.revision,
              image.localEditVersion == original.savedEditVersion else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
        return try await consume(image, saved, unchanged)
    }

    func saveSceneSnapshot(viewpointId: String, placementId: String) async {
        guard let window = NSApp.keyWindow, let api else { return }
        do {
            try await withSceneSnapshot(camera: .savedViewpoint(viewpointId), placementId: placementId) { image, saved, unchanged in
                let panel = NSSavePanel()
                panel.allowedContentTypes = [.png]; panel.canCreateDirectories = true
                panel.nameFieldStringValue = "전시-시점-r\(saved.revision).png"
                panel.title = "감상 시점 이미지 저장"
                panel.message = "저장된 전시 r\(saved.revision)의 감상 시점 · 640 × 360 PNG"
                let answer = await withCheckedContinuation { continuation in
                    panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
                }
                guard answer == .OK, let destination = panel.url else { sceneSnapshotStatus = "이미지 저장을 취소했습니다."; return }
                let beforeCommit = try await api.exhibition(saved.draft.id)
                try saved.requireSameComparison(beforeCommit)
                guard unchanged() else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
                // A new destination preserves any existing file, even after a panel
                // replacement choice. File-system failure never modifies the draft.
                try await image.writeNewPNG(to: destination)
                let visibility: String
                if image.target["inFrustum"] != .bool(true) { visibility = "대상 작품이 화면 밖에 있습니다." }
                else { switch image.target["centerRay"].string {
                    case "clear": visibility = "작품 중심 방향에 가림이 감지되지 않았습니다."
                    case "blocked": visibility = "작품 중심 방향에 가림이 있습니다."
                    default: visibility = "작품 중심의 가림 여부를 확인하지 못했습니다."
                } }
                sceneSnapshotStatus = "전시 r\(saved.revision) · 640 × 360 PNG를 저장했습니다. " + visibility + " 작품 전체의 가림을 보장하지는 않습니다."
            }
        } catch {
            sceneSnapshotStatus = "이미지를 저장하지 못했습니다. 현재 연결·선택·저장 상태와 새 파일 이름을 확인하세요. 기존 전시와 입력은 유지됩니다."
            viewportController.diagnose("SCENE_SNAPSHOT", error)
        }
    }
    var dirty: Bool { state?.dirty == true || formDirty || positionDirty || architectureDirty || architectureApplying }
    /// Adopt the exact persisted pending request first, preserving it if the
    /// final confirmation cannot be stored or reconciled.
    func acceptBoundedPending(_ saved: NativeDocumentState) -> Bool {
        guard let current = state, !busy, !dirty, current.draft == nil, current.pending == nil else { return false }
        let (expectedRevision, overflow) = current.revision.addingReportingOverflow(1)
        guard !overflow, saved.revision == expectedRevision, current.workspaceId == saved.workspaceId,
              current.tenantId == saved.tenantId, current.principalId == saved.principalId,
              saved.draft != nil, saved.etag != nil, saved.pending?.origin == "bounded-import-v1" else { return false }
        state = saved; storedImmutable = selectedBoundedPolicy; boundedPolicyPending = selectedBoundedPolicy; reusablePublicationId = nil; syncFields()
        return true
    }
    func acceptBoundedSaved(_ saved: NativeDocumentState, requestId: String) -> Bool {
        guard let current = state, !busy, let pending = current.pending, pending.requestId == requestId,
              current.workspaceId == saved.workspaceId, current.tenantId == saved.tenantId,
              current.principalId == saved.principalId, saved.pending == nil,
              saved.draft == pending.draft, saved.etag != nil, !saved.dirty else { return false }
        let (expectedRevision, overflow) = current.revision.addingReportingOverflow(1)
        guard !overflow, saved.revision == expectedRevision else { return false }
        state = saved; storedImmutable = selectedBoundedPolicy; boundedPolicyPending = selectedBoundedPolicy; reusablePublicationId = nil; syncFields()
        notice = "가져온 전시를 다시 열었습니다."
        if selectedBoundedPolicy {
            let access = viewportAccessID
            Task { [weak self] in await self?.resolveImportedPolicy(saved, access: access) }
        }
        return true
    }
    func connect(api: AuthoringAPI, session: AuthoringSession, credentials: LocalAuthoringSession, selectedBoundedPolicy: Bool = false, ordinaryBoundedRuntime: Bool = false, boundedRuntimeQualified: Bool = false, read: @escaping () async throws -> NativeDocumentState, write: @escaping (NativeDocumentState) async throws -> NativeDocumentState) async {
        // Explicit reconnect during a preview has a separate acknowledgement
        // boundary. Never run the ordinary access-change reset on this path.
        if entrancePreviewActive {
            await reconnectEntranceAuthority(api: api, session: session, credentials: credentials, read: read, write: write)
            return
        }
        // An owned begin/commit (including its store write) must finish before
        // credentials or store closures can change. A stable ghost can cancel
        // against its OLD connection, then reconnect through normal initialization.
        guard !busy, !placementApplying, !architectureApplying else { return }
        await cancelVisualArtworkPlacement()
        // Cancellation awaits the old renderer. Another owned operation may
        // have started in the meantime; never replace its account/store binding.
        guard !busy, !placingArtwork, !architectureApplying else { return }
        clearInfoStandPreview(); infoStandProposal = nil; infoStandProposalName = nil; infoStandReplacementName = nil
        if let old = self.session, old.tenantId != session.tenantId || old.userId != session.userId {
            guard !dirty else { sceneSnapshotConnectionGeneration = UUID(); self.api = nil; viewportCredentials = nil; failure = AuthoringFailure("DRAFT_ACCOUNT_MISMATCH"); return }
            state = nil; reusablePublicationId = nil; items = []; library = []; comparison = nil; exportedURL = nil; exportedSHA256 = nil; catalogArtworkId = ""; catalogStatus = ""
        }
        boundedPolicyGeneration = UUID()
        boundedPlacementCapability = nil; boundedPlacementPolicy = nil; boundedPlacementBaseline = nil
        boundedPlacementSession = nil; boundedPlacementAccess = nil; boundedPlacementToken = nil; boundedPlacementEpoch = nil; boundedProfileKnown = nil
        sceneSnapshotConnectionGeneration = UUID()
        self.api = api; self.session = session
        self.selectedBoundedPolicy = selectedBoundedPolicy
        self.ordinaryBoundedRuntime = ordinaryBoundedRuntime && !selectedBoundedPolicy
        self.boundedRuntimeQualified = boundedRuntimeQualified
        boundedPolicyPending = (selectedBoundedPolicy || self.ordinaryBoundedRuntime) && state?.etag != nil
        if !selectedBoundedPolicy && !self.ordinaryBoundedRuntime { boundedReadOnly = false }
        // Capability is tied to the current runtime; fail closed until its
        // authenticated lookup succeeds, retaining the local document.
        reusablePublicationId = nil
        if state?.etag != nil { storedImmutable = true }
        let accessChanged = viewportCredentials?.token != credentials.token || viewportCredentials?.origin != credentials.origin
        viewportCredentials = credentials
        if accessChanged { pendingCatalogThumbnail = nil; clearWallFinishPreview(); sharedFurnishingCatalogReady = false; sharedFurnishingCatalog = []; sharedFurnishingCatalogAccessID = nil; sharedFurnishingPreview = nil; sharedFurnishingPreviewAccessID = nil; entrancePreviewTask?.cancel(); entrancePreviewGeneration = UUID(); entrancePreviewDraft = nil; entrancePreviewBase = nil; entranceHostRecovering = false; entranceRecoveryEpoch = nil; entrancePreviewBusy = false; entrancePreviewStatus = ""; invalidateEntranceThumbnails(); viewportAccessID = UUID(); furnishingCatalogReady = false; furnishingCatalogAccessID = nil; furnishingCatalogStatus = ""; furnishingImage = nil; furnishingImageAttempted = false; surfaceMaterialPreview = nil; spaceLightingPreview = nil; spaceLightingPreviewAccessID = nil; lightPreview = nil; lightPreviewAccessID = nil; benchPreview = nil; benchPreviewAccessID = nil; plaquePreview = nil; plaquePreviewAccessID = nil; catalogTask?.cancel(); catalogEntries = [:]; catalogFailures = []; catalogImages = [:]; catalogImageOrder = []; catalogPreviewFailures = [] }
        floorPreviewAccessID = nil; floorPreviewContext = nil; floorPreviewHostEpoch = nil
        if accessChanged {
            floorImages = [:]; floorImageAttempts = []
            clearPrivateFloorCatalog()
        }
        self.read = read; self.write = write
        viewingPlanPreview = nil; viewingPlanPreviewAccessID = nil; viewingPlanPreviewContext = nil; viewingPlanPreviewHostEpoch = nil; viewingPlanPreviewSequence = nil
        clearViewingPointPreview()
        // Reconnecting may replace the native store closures even for the same
        // account/origin. Keep text, but never apply a prior binding's preview.
        clearSurfaceMaterialPreview()
        if accessChanged { surfaceMaterialImages = [:]; surfaceMaterialCatalog = nil; surfaceMaterialCatalogAccessID = nil; surfaceMaterialCatalogRequestID = nil; surfaceMaterialCatalogLoading = false; surfaceMaterialCatalogStatus = "" }
        furnishingPreview = nil; furnishingPreviewAccessID = nil; sharedFurnishingPreview = nil; sharedFurnishingPreviewAccessID = nil; spaceLightingPreview = nil; spaceLightingPreviewAccessID = nil; lightPreview = nil; lightPreviewAccessID = nil; benchPreview = nil; benchPreviewAccessID = nil; plaquePreview = nil; plaquePreviewAccessID = nil
        await work {
            if state == nil {
                let stored = try await read()
                guard stored.tenantId.lowercased() == session.tenantId.lowercased(), stored.principalId.lowercased() == session.userId.lowercased() else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                state = stored
                // The persisted document may be a published snapshot. Lock it
                // before any later list/capability request can fail.
                if stored.etag != nil { storedImmutable = true; boundedPolicyPending = selectedBoundedPolicy || self.ordinaryBoundedRuntime; reusablePublicationId = nil }
                syncFields()
                if stored.pending != nil { notice = "저장 요청의 응답을 확인해야 합니다. 같은 요청으로 다시 확인하세요." }
                else if stored.dirty { notice = "이 Mac에 보관한 미완료 전시를 복원했습니다." }
            }
            try await list()
            if let draft, let etag = state?.etag {
                let accessID = viewportAccessID, policyGeneration = boundedPolicyGeneration
                let access = try await publishedEditingAccess(draft.id, api: api)
                let restricted = try await checkBoundedPolicy(draft.id, etag: etag, api: api, access: accessID)
                guard viewportAccessID == accessID, boundedPolicyGeneration == policyGeneration, self.api === api, self.state?.draft == draft, self.state?.etag == etag else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                boundedReadOnly = restricted; boundedPolicyPending = self.ordinaryBoundedRuntime
                storedImmutable = access.immutable || restricted || self.ordinaryBoundedRuntime; reusablePublicationId = access.publicationId
            } else { storedImmutable = false; boundedReadOnly = false; boundedPolicyPending = false; reusablePublicationId = nil }
        }
    }
    func beginEntranceCredentialRenewal() -> Bool {
        guard entrancePreviewActive else { return true }
        guard !busy, !pending, !placingArtwork, !architectureApplying,
              entrancePreviewBase == draft, viewportCredentials != nil,
              let epoch = viewportController.hostEpoch ?? entranceRecoveryEpoch ?? entrancePreviewEpoch else { return false }
        entrancePreviewTask?.cancel(); entrancePreviewTask = nil; entrancePreviewGeneration = UUID()
        entranceRecoveryEpoch = epoch; entranceHostRecovering = true
        entranceCredentialRenewalPending = true; entrancePreviewBusy = true
        invalidateEntranceThumbnails()
        entrancePreviewStatus = "같은 작업 공간의 연결을 갱신하고 있습니다. 입력과 편집 잠금은 유지됩니다."
        return true
    }
    func entranceCredentialRenewalFailed() {
        guard entrancePreviewActive, entranceCredentialRenewalPending else { return }
        entrancePreviewBusy = false
        entrancePreviewStatus = "작업 공간 연결을 갱신하지 못했습니다. 입력과 미리보기 잠금을 유지합니다. 작업 공간 다시 연결을 선택하세요."
    }
    private func reconnectEntranceAuthority(api: AuthoringAPI, session: AuthoringSession, credentials: LocalAuthoringSession,
                                            read: @escaping () async throws -> NativeDocumentState,
                                            write: @escaping (NativeDocumentState) async throws -> NativeDocumentState) async {
        guard entranceCredentialRenewalPending, !busy, !pending, !placingArtwork, !architectureApplying,
              let old = viewportCredentials, credentials.isRenewal(of: old),
              session.tenantId.lowercased() == old.tenantId.lowercased(), session.userId.lowercased() == old.principalId.lowercased(),
              let base = draft, entrancePreviewBase == base, let previousEpoch = entranceRecoveryEpoch else {
            entranceCredentialRenewalFailed()
            return
        }
        sceneSnapshotConnectionGeneration = UUID()
        self.api = api; self.session = session; self.read = read; self.write = write
        viewportCredentials = credentials
        viewportAccessID = UUID() // Same model/buffers/history; a new owned WK coordinator.
        let access = viewportAccessID, generation = UUID()
        entrancePreviewGeneration = generation; entranceCredentialRenewalPending = false
        entrancePreviewTask = Task { [weak self] in
            guard let self else { return }
            do {
                try await self.viewportController.waitForOrdinaryRecovery(replacing: previousEpoch, draft: base,
                                                                         tenantId: credentials.tenantId, principalId: credentials.principalId)
                guard self.entrancePreviewGeneration == generation, self.viewportAccessID == access, self.draft == base else { return }
                self.confirmEntranceHostRecovery()
            } catch {
                if self.entrancePreviewGeneration == generation {
                    self.entrancePreviewBusy = false; self.entrancePreviewTask = nil
                    self.entrancePreviewStatus = "갱신한 연결의 편집 화면을 기다리고 있습니다. 입력과 잠금을 유지합니다. 작업 공간 다시 연결로 재시도하세요."
                }
            }
        }
    }
    func previewEntrance(_ action: EntrancePreviewAction = .show, thumbnails: Bool = false, systemReducedMotion: Bool) {
        guard !entranceCredentialRenewalPending, !busy, !pending, !placingArtwork, !architectureApplying, !entrancePreviewBusy, entranceEditable,
              !surfaceMaterialPreviewing, !lightPreviewing, !benchPreviewing, !plaquePreviewing, !furnishingPreviewing, !spaceLightingPreviewing, !infoStandPreviewing,
              let base = draft else { return }
        do {
            entranceSystemReducedMotion = systemReducedMotion
            let display = try entrancePreviewDraft ?? appliedFields()
            let access = viewportAccessID, generation = UUID()
            let thumbnailGeneration = entranceThumbnailGeneration
            let device = entrancePreviewDevice, scenario = entrancePreviewScenario
            let reduced = systemReducedMotion || entrancePreviewReducedMotion
            let locale = entrancePreviewLocale.isEmpty ? nil : entrancePreviewLocale
            entrancePreviewTask?.cancel(); entrancePreviewGeneration = generation
            entrancePreviewEpoch = viewportController.hostEpoch
            entrancePreviewBase = base; entrancePreviewDraft = display; entrancePreviewBusy = true
            entrancePreviewStatus = thumbnails ? "실제 표지 화면을 준비하고 있습니다." : "관람 미리보기를 준비하고 있습니다."
            entrancePreviewTask = Task { [weak self] in
                guard let self else { return }
                @MainActor func check(_ output: NativeEntrancePreviewOutput, theme: CoverEntryTheme? = nil) throws {
                    try Task.checkCancellation()
                    guard self.entrancePreviewGeneration == generation, self.viewportAccessID == access,
                          self.draft == base, self.entrancePreviewDraft == display,
                          !thumbnails || self.entranceThumbnailGeneration == thumbnailGeneration,
                          output.request.matchesContext(draft: display, device: self.entrancePreviewDevice, scenario: self.entrancePreviewScenario,
                                                        reducedMotion: self.entranceSystemReducedMotion || self.entrancePreviewReducedMotion,
                                                        theme: theme, locale: self.entrancePreviewLocale.isEmpty ? nil : self.entrancePreviewLocale) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                func input(_ action: EntrancePreviewAction, theme: CoverEntryTheme? = nil) -> NativeEntrancePreviewInput {
                    NativeEntrancePreviewInput(action: action, device: device, scenario: scenario, reducedMotion: reduced, theme: theme, locale: locale, draft: display)
                }
                do {
                    if thumbnails {
                        for theme in CoverEntryTheme.allCases {
                            let output = try await self.viewportController.entrancePreview(input(.thumbnail, theme: theme))
                            try check(output, theme: theme)
                            if let bytes = output.png { self.entranceThumbnails[theme] = bytes }
                        }
                        let output = try await self.viewportController.entrancePreview(input(.close))
                        try check(output)
                        self.entrancePreviewDraft = nil; self.entrancePreviewBase = nil
                        self.entrancePreviewStatus = self.entranceThumbnails.count == 5 ? "같은 전시의 실제 표지 다섯 가지를 불러왔습니다." : "공간의 문턱은 확인된 공간 이미지가 필요합니다. 준비된 표지는 사용할 수 있습니다."
                    } else {
                        let output = try await self.viewportController.entrancePreview(input(action))
                        try check(output)
                        self.entrancePreviewStatus = action == .walk ? "같은 전시를 관람하고 있습니다. 편집 복귀로 입력과 시점을 이어갑니다." : "관람객에게 보이는 실제 표지입니다. 미리보기는 저장하지 않습니다."
                    }
                } catch {
                    if self.entrancePreviewGeneration == generation {
                        self.entrancePreviewStatus = "미리보기를 준비하지 못했습니다. 입력은 유지됩니다. 편집 복귀 후 연결과 이미지 상태를 확인하세요."
                        self.viewportController.diagnose("ENTRANCE", error)
                    }
                }
                if self.entrancePreviewGeneration == generation { self.entrancePreviewBusy = false; self.entrancePreviewTask = nil }
            }
        } catch { entrancePreviewStatus = "표지 입력을 확인하세요. 숫자 범위와 이미지·언어 설정을 적용할 수 없습니다." }
    }
    func closeEntrancePreview() {
        if entranceHostRecovering {
            confirmEntranceHostRecovery()
            return
        }
        let display = entrancePreviewDraft, generation = UUID()
        entrancePreviewTask?.cancel(); entrancePreviewGeneration = generation; entrancePreviewBusy = true
        entrancePreviewTask = Task { [weak self] in
            guard let self else { return }
            do {
                guard let display else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let input = NativeEntrancePreviewInput(action: .close, device: self.entrancePreviewDevice, scenario: self.entrancePreviewScenario,
                                                      reducedMotion: self.entranceSystemReducedMotion || self.entrancePreviewReducedMotion,
                                                      theme: nil, locale: self.entrancePreviewLocale.isEmpty ? nil : self.entrancePreviewLocale, draft: display)
                _ = try await self.viewportController.entrancePreview(input)
            } catch {
                if self.entrancePreviewGeneration == generation {
                    self.entrancePreviewBusy = false; self.entrancePreviewTask = nil
                    self.entrancePreviewStatus = "편집 복귀를 확인하지 못했습니다. 미리보기와 입력을 유지합니다. 연결을 확인한 뒤 편집 복귀를 다시 선택하세요."
                    self.viewportController.diagnose("ENTRANCE_CLOSE", error)
                }
                return
            }
            guard self.entrancePreviewGeneration == generation else { return }
            // Unlock only after the current host acknowledged editor mode.
            self.entrancePreviewDraft = nil; self.entrancePreviewBase = nil; self.entrancePreviewBusy = false; self.entrancePreviewTask = nil
            self.entrancePreviewStatus = "편집으로 돌아왔습니다. 입력·선택·실행 취소 이력은 유지됩니다."
        }
    }
    func recoverEntrancePreviewHost(reload: () -> Void) {
        guard !entranceCredentialRenewalPending else { return }
        guard canRecoverEntranceHost, let base = draft, let previousEpoch = viewportController.hostEpoch,
              let credentials = viewportCredentials else { return }
        entrancePreviewTask?.cancel()
        let generation = UUID(), access = viewportAccessID
        entrancePreviewGeneration = generation; entranceRecoveryEpoch = previousEpoch
        entranceHostRecovering = true; entrancePreviewBusy = true
        invalidateEntranceThumbnails()
        // Reuse the existing representable identity replacement. Its old
        // coordinator invalidates its own epoch, nonce and callbacks. No app,
        // service, model or authoring history is restarted or discarded.
        reload()
        entrancePreviewStatus = "같은 전시의 편집 화면을 다시 연결하고 있습니다. 입력은 유지됩니다."
        entrancePreviewTask = Task { [weak self] in
            guard let self else { return }
            do {
                try await self.viewportController.waitForOrdinaryRecovery(replacing: previousEpoch, draft: base,
                                                                         tenantId: credentials.tenantId, principalId: credentials.principalId)
                guard self.entrancePreviewGeneration == generation, self.viewportAccessID == access, self.draft == base else { return }
                self.confirmEntranceHostRecovery()
            } catch {
                if self.entrancePreviewGeneration == generation {
                    self.entrancePreviewBusy = false; self.entrancePreviewTask = nil
                    self.entrancePreviewStatus = "새 편집 화면의 응답을 기다리고 있습니다. 연결을 확인한 뒤 편집 화면 다시 열기를 선택하세요. 입력과 미리보기 잠금은 유지됩니다."
                }
            }
        }
    }
    func confirmEntranceHostRecovery() {
        guard !entranceCredentialRenewalPending, entranceHostRecovering, let previousEpoch = entranceRecoveryEpoch, let base = entrancePreviewBase, draft == base,
              let credentials = viewportCredentials,
              viewportController.ordinaryRecoveryReady(replacing: previousEpoch, draft: base, tenantId: credentials.tenantId, principalId: credentials.principalId) else { return }
        entrancePreviewTask?.cancel(); entrancePreviewTask = nil; entrancePreviewGeneration = UUID()
        entrancePreviewDraft = nil; entrancePreviewBase = nil; entranceHostRecovering = false; entranceRecoveryEpoch = nil; entrancePreviewBusy = false
        entrancePreviewStatus = "편집 화면을 다시 연결했습니다. 입력·선택·실행 취소 이력과 마지막으로 선택한 보기는 유지됩니다."
    }
    var workspaceMaterialPreviewAvailable: Bool {
        viewportCredentials != nil && api?.workspaceMaterialRuntimeQualified == true
            && session?.workspaceMaterialProfile == "workspace-material-intake-v1"
    }
    var workspaceMaterialIntakeAvailable: Bool {
        workspaceMaterialPreviewAvailable && !selectedBoundedPolicy
    }
    var workspaceMaterialMultimapAvailable: Bool {
        workspaceMaterialPreviewAvailable && api?.workspaceMaterialMultimapRuntimeQualified == true
    }
    var workspaceWallMaterialAvailable: Bool {
        workspaceMaterialIntakeAvailable && api?.workspaceWallMaterialRuntimeQualified == true
    }
    var workspaceFurnishingIntakeAvailable: Bool {
        viewportCredentials != nil && !selectedBoundedPolicy && api?.workspaceFurnishingRuntimeQualified == true
            && session?.workspaceFurnishingProfile == "workspace-furnishing-intake-v1"
    }
    var workspaceFurnishingStoolAvailable: Bool {
        workspaceFurnishingIntakeAvailable && session?.advertisesWorkspaceFurnishingStool == true
    }
    var workspaceFurnishingRoomVisualAvailable: Bool {
        workspaceFurnishingIntakeAvailable && session?.advertisesWorkspaceFurnishingRoomVisual == true
    }
    var workspaceArtworkFrameAvailable: Bool {
        workspaceFurnishingIntakeAvailable && session?.advertisesWorkspaceArtworkFrame == true
    }
    var workspaceArtworkFramePreparationAvailable: Bool {
        workspaceArtworkFrameAvailable && api?.workspaceFramePreparationRuntimeQualified == true
            && session?.advertisesWorkspaceArtworkFramePreparation == true
    }
    var workspaceInfoStandPreparationAvailable: Bool {
        workspaceFurnishingRoomVisualAvailable && api?.workspaceInfoStandPreparationRuntimeQualified == true
            && session?.advertisesWorkspaceInfoStandPreparation == true
    }
    @Published private(set) var infoStandProposalName: String?
    @Published private(set) var infoStandReplacementName: String?
    @Published private(set) var infoStandStatus = ""
    @Published private var infoStandProposal: WorkspaceInfoStandProposal?
    @Published private var infoStandPreview: WorkspaceInfoStandPreview?
    @Published private var infoStandTargetFocused = false
    private struct InfoStandBinding {
        let access: UUID, context: UUID
        let host: String
        let api: AuthoringAPI
        let session: AuthoringSession
        let credentials: LocalAuthoringSession
    }
    private var infoStandBinding: InfoStandBinding?
    private var infoStandFocusBinding: InfoStandBinding?
    private var infoStandFocusCameraSequence: Int?
    var infoStandPreviewing: Bool { infoStandPreview != nil }
    private var infoStandTargetPlacement: String? {
        guard let draft, let proposal = infoStandProposal,
              let plaque = try? PlaqueSettings.list(in: draft).first(where: { $0.id == proposal.replacement.plaqueId }),
              case .artwork(let id) = plaque.source else { return nil }
        return id
    }
    var infoStandTargetTitle: String? {
        guard let draft, let id = infoStandTargetPlacement,
              let placement = draft.placements.first(where: { $0["id"].string == id }) else { return nil }
        return draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] })?["metadata"]["title"].string
    }
    var infoStandPlaqueSize: String? {
        guard let replacement = infoStandProposal?.replacement else { return nil }
        return String(format: "%.0f × %.0f cm", replacement.plaqueWidth * 100, replacement.plaqueHeight * 100)
    }
    private func infoStandContextReady(acknowledging expected: NativeExhibitionDraft?) -> Bool {
        workspaceFurnishingRoomVisualAvailable && session?.advertisesWorkspaceInfoStandReplacement == true
            && !selectedBoundedPolicy && !ordinaryBoundedRuntime && !pending && state?.dirty == false && state?.draft != nil && state?.etag != nil
            && !entrancePreviewActive && !placingArtwork && !architectureApplying
            && !viewportFormDirty && !wallFinishDirty && !positionDirty && !architectureGeometryDirty
            && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !spaceLightingDirty
            && !furnishingDirty && !viewingPlanDirty && viewingPointInput == nil
            && !furnishingPreviewing && !surfaceMaterialPreviewing && !wallFinishPreviewing
            && !lightPreviewing && !benchPreviewing && !plaquePreviewing && !spaceLightingPreviewing
            && viewportPresentation == .threeD && viewportController.connected && viewportController.editingContextActive
            && !editingLocked(acknowledging: expected)
    }
    var canOpenInfoStandProposal: Bool { !busy && !infoStandPreviewing && infoStandContextReady(acknowledging: draft) }
    var canFocusInfoStandTarget: Bool {
        canOpenInfoStandProposal && infoStandProposal?.etag == state?.etag && infoStandTargetPlacement != nil
            && viewportController.findSelectionSupported
    }
    var canPreviewInfoStandReplacement: Bool {
        guard canFocusInfoStandTarget, infoStandTargetFocused, let binding = infoStandFocusBinding else { return false }
        return matchesInfoStandBinding(binding) && infoStandFocusCameraSequence == viewportCameraSequence
            && viewportSelection?.kind == "placement" && viewportSelection?.id == infoStandTargetPlacement
    }
    private func matchesInfoStandBinding(_ binding: InfoStandBinding) -> Bool {
        guard self.api === binding.api, viewportAccessID == binding.access,
              viewportController.editingContextGeneration == binding.context, viewportController.hostEpoch == binding.host,
              let session, let credentials = viewportCredentials else { return false }
        return session.tenantId == binding.session.tenantId && session.userId == binding.session.userId
            && session.role == binding.session.role && session.csrfToken == binding.session.csrfToken
            && session.expiresAt == binding.session.expiresAt
            && session.advertisesWorkspaceInfoStandReplacement
            && credentials.tenantId == binding.credentials.tenantId && credentials.principalId == binding.credentials.principalId
            && credentials.origin == binding.credentials.origin && credentials.token == binding.credentials.token
    }
    private var checkedInfoStandPreview: WorkspaceInfoStandPreview? {
        guard let preview = infoStandPreview, let binding = infoStandBinding, let current = state,
              matchesInfoStandBinding(binding), (try? preview.apply(to: current)) != nil else { return nil }
        return preview
    }
    var canApplyInfoStandReplacement: Bool {
        guard !busy, let preview = checkedInfoStandPreview, preview.changed,
              let credentials = viewportCredentials,
              infoStandContextReady(acknowledging: preview.displayDraft) else { return false }
        return viewportController.confirmsMetadataPreview(preview.displayDraft, tenantId: credentials.tenantId,
                                                         principalId: credentials.principalId)
    }
    private func clearInfoStandPreview() {
        infoStandPreview = nil; infoStandBinding = nil; infoStandTargetFocused = false
        infoStandFocusBinding = nil; infoStandFocusCameraSequence = nil
    }
    func cancelInfoStandPreview() {
        guard !busy, infoStandPreviewing else { return }
        clearInfoStandPreview()
        infoStandStatus = "미리보기를 취소했습니다. 안내대와 명판의 기존 배치는 유지됩니다."
    }
    func openInfoStandProposal() {
        guard canOpenInfoStandProposal else { return }
        let panel = NSOpenPanel(); panel.allowedContentTypes = [.json]; panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false; panel.message = "현재 전시에 준비한 안내대·명판 교체안을 선택하세요."
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            let size = try url.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey])
            guard size.isRegularFile == true, let count = size.fileSize, (1...65_536).contains(count) else {
                throw AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_INVALID")
            }
            let bytes = try Data(contentsOf: url)
            guard bytes.count == count else { throw AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_INVALID") }
            let proposal = try WorkspaceInfoStandProposal(JSONDecoder().decode(DocumentValue.self, from: bytes))
            guard canOpenInfoStandProposal, proposal.etag == state?.etag else { throw AuthoringFailure("EXHIBITION_COMPARISON_STALE") }
            infoStandProposal = proposal; infoStandProposalName = url.lastPathComponent
            infoStandReplacementName = nil; infoStandTargetFocused = false
            infoStandFocusBinding = nil; infoStandFocusCameraSequence = nil
            guard infoStandTargetTitle != nil else {
                infoStandProposal = nil; infoStandProposalName = nil
                throw AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_INVALID")
            }
            infoStandStatus = "교체안을 열었습니다. 대상 작품을 찾은 뒤 안내대와 명판을 미리 보세요."
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_INVALID") }
    }
    func focusInfoStandTarget() async {
        guard canFocusInfoStandTarget, let id = infoStandTargetPlacement, let proposal = infoStandProposal,
              let current = state, let api, let session, let credentials = viewportCredentials,
              let host = viewportController.hostEpoch else { return }
        let access = viewportAccessID, context = viewportController.editingContextGeneration
        let binding = InfoStandBinding(access: access, context: context, host: host, api: api, session: session, credentials: credentials)
        placementId = id; selectPlacement()
        // Selection needs its ordinary acknowledged frame before findSelection.
        // A second press after selection is ready is harmless and never stages.
        guard viewportController.acknowledgesOrdinarySelection(draft: draft, tenantId: state?.tenantId, principalId: state?.principalId,
                                                              selection: .init(kind: "placement", id: id)) else {
            infoStandStatus = "대상 작품을 선택했습니다. 선택이 화면에 표시되면 ‘대상 작품 보기’를 누르세요."
            return
        }
        do {
            try await viewportController.findSelectionChecked()
            guard canFocusInfoStandTarget, matchesInfoStandBinding(binding), infoStandProposal == proposal,
                  state?.revision == current.revision, state?.draft == current.draft, state?.etag == current.etag,
                  viewportSelection == .init(kind: "placement", id: id) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            infoStandTargetFocused = true
            infoStandFocusBinding = binding; infoStandFocusCameraSequence = viewportCameraSequence
            infoStandStatus = "대상 작품의 시점을 맞췄습니다. 이 시점에서 교체안을 미리 볼 수 있습니다."
        } catch { infoStandTargetFocused = false; failure = error as? AuthoringFailure ?? AuthoringFailure("VIEWPORT_FRAME_STALE") }
    }
    func previewInfoStandReplacement() async {
        guard canPreviewInfoStandReplacement, let proposal = infoStandProposal, let base = state, let baseDraft = base.draft,
              let api, let session, let credentials = viewportCredentials, let host = viewportController.hostEpoch else { return }
        guard base.tenantId == session.tenantId, base.principalId == session.userId,
              base.tenantId == credentials.tenantId, base.principalId == credentials.principalId else { return }
        let binding = InfoStandBinding(access: viewportAccessID, context: viewportController.editingContextGeneration,
                                       host: host, api: api, session: session, credentials: credentials)
        await work {
            guard await api.admitsWorkspaceInfoStandReplacement else { throw AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_UNAVAILABLE") }
            let furnishing = try await api.workspaceFurnishing(proposal.replacement.placement.furnishingId, version: proposal.replacement.placement.furnishingVersion)
            let prospective = try await api.prepareWorkspaceInfoStandReplacement(proposal.replacement, in: base,
                expectedModelSHA256: proposal.modelSHA256, intakeId: proposal.intakeId)
            let saved = try await api.exhibition(baseDraft.id)
            let policy = try await api.nativeViewportPolicy(baseDraft.id)
            try SurfaceMaterialTextureRequest.requireEditingPolicy(policy, document: base)
            let fresh = try await api.workspaceFurnishing(furnishing.id, version: furnishing.version)
            guard matchesInfoStandBinding(binding), infoStandProposal == proposal, state?.revision == base.revision,
                  state?.draft == base.draft, state?.etag == base.etag, state?.pending == nil,
                  furnishing == fresh, furnishing.details.rights.permissions.display,
                  saved.draft == base.draft, saved.etag == base.etag,
                  await api.admitsWorkspaceInfoStandReplacement,
                  infoStandContextReady(acknowledging: draft),
                  let version = (prospective ?? baseDraft).candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["version"].integer else {
                throw AuthoringFailure("VIEWPORT_FRAME_STALE")
            }
            // The API just checked the runtime's supported extension version.
            // The local preview uses that already-validated candidate version.
            let preview = try WorkspaceInfoStandPreview(base: base, replacement: proposal.replacement, furnishing: furnishing,
                supportedExtensionVersion: version, prospectiveDraft: prospective)
            guard let current = state, (try? preview.apply(to: current)) != nil else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            infoStandReplacementName = furnishing.details.name
            guard preview.changed else { infoStandStatus = "현재 안내대·명판과 같은 교체안입니다."; return }
            infoStandBinding = binding; infoStandPreview = preview
            infoStandStatus = "미리보기입니다. 명판이 안내대 위에 놓이는지와 주변 작품·통로의 간격을 확인하세요."
        }
    }
    func applyInfoStandReplacement() async {
        guard canApplyInfoStandReplacement, let preview = checkedInfoStandPreview,
              let binding = infoStandBinding, let proposal = infoStandProposal, let baseDraft = preview.base.draft else { return }
        await work(allowInfoStandPreview: true) {
            guard let current = state, canApplyInfoStandAuthority(preview, binding: binding),
                  await binding.api.admitsWorkspaceInfoStandReplacement else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            _ = try preview.apply(to: current)
            let prospective = try await binding.api.prepareWorkspaceInfoStandReplacement(proposal.replacement, in: current,
                expectedModelSHA256: proposal.modelSHA256, intakeId: proposal.intakeId)
            let saved = try await binding.api.exhibition(baseDraft.id)
            let policy = try await binding.api.nativeViewportPolicy(baseDraft.id)
            try SurfaceMaterialTextureRequest.requireEditingPolicy(policy, document: current)
            let fresh = try await binding.api.workspaceFurnishing(preview.furnishing.id, version: preview.furnishing.version)
            guard canApplyInfoStandAuthority(preview, binding: binding), infoStandProposal == proposal, let live = state,
                  fresh == preview.furnishing, fresh.details.rights.permissions.display,
                  saved.draft == current.draft, saved.etag == current.etag,
                  prospective?.candidate == preview.displayDraft.candidate,
                  await binding.api.admitsWorkspaceInfoStandReplacement else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            var next = live; next.draft = try preview.apply(to: live)
            try await persist(next)
            clearInfoStandPreview(); infoStandProposal = nil; infoStandProposalName = nil; syncFields()
            infoStandStatus = "안내대와 명판 교체를 이 Mac에 보관했습니다. 전시 저장은 별도로 진행하세요."
            notice = infoStandStatus
        }
    }
    private func canApplyInfoStandAuthority(_ preview: WorkspaceInfoStandPreview, binding: InfoStandBinding) -> Bool {
        guard matchesInfoStandBinding(binding), infoStandProposal?.replacement == preview.replacement,
              let current = state, (try? preview.apply(to: current)) != nil,
              infoStandContextReady(acknowledging: preview.displayDraft) else { return false }
        return viewportController.confirmsMetadataPreview(preview.displayDraft, tenantId: binding.credentials.tenantId,
                                                         principalId: binding.credentials.principalId)
    }
    var workspacePortalFinishAvailable: Bool {
        workspaceFurnishingIntakeAvailable && api?.workspacePortalFinishRuntimeQualified == true
            && session?.advertisesWorkspacePortalFinish == true
    }
    var workspacePortalRectangularFitAvailable: Bool {
        workspacePortalFinishAvailable && session?.advertisesWorkspaceRectangularPortalFit == true
    }
    var canEditWorkspacePortalFinish: Bool {
        workspacePortalFinishAvailable && canSelectCatalog && !immutable && !dirty && !viewingPlanDirty
            && !architectureDirty && !formDirty && !positionDirty && state?.etag != nil && viewportController.editingContextActive
    }
    var canEditWorkspaceArtworkFrame: Bool {
        workspaceArtworkFrameAvailable && canSelectCatalog && !immutable && !dirty && !viewingPlanDirty
            && state?.etag != nil && viewportController.editingContextActive
    }
    var canEditWorkspaceBenchSeat: Bool {
        workspaceFurnishingIntakeAvailable && canSelectCatalog && !immutable && !dirty && !benchDirty
            && !viewingPlanDirty && !architectureDirty && !formDirty && !positionDirty
            && state?.etag != nil && viewportController.editingContextActive
    }
    func workspaceBenchSeatPolicyETag(using api: AuthoringAPI) async -> String? {
        guard let state else { return nil }
        return try? await api.workspaceBenchSeatPolicy(in: state).etag
    }
    func withWorkspaceFurnishingAccess<T>(_ access: UUID, _ body: (AuthoringAPI) async throws -> T) async throws -> T {
        guard workspaceFurnishingIntakeAvailable, access == viewportAccessID, !busy, !entrancePreviewActive, !infoStandPreviewing,
              !placingArtwork, !architectureApplying, !pending, let api else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        busy = true
        defer { busy = false }
        guard await api.admitsWorkspaceFurnishings else { throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE") }
        let result = try await body(api)
        guard access == viewportAccessID else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        return result
    }
    func placeWorkspaceFurnishing(_ placement: WorkspaceFurnishingPlacement, access: UUID,
                                 expectedModelSHA256: String? = nil, infoStandIntakeId: String? = nil) async throws -> Bool {
        try await withWorkspaceFurnishingAccess(access) { api in
            guard !immutable, !formDirty, !positionDirty, !architectureDirty,
                  viewportController.editingContextActive, var current = state else {
                throw AuthoringFailure("ARCHITECTURE_APPLY_REQUIRED")
            }
            let revision = current.revision
            let next = try await api.prepareWorkspaceFurnishingPlacement(placement, in: current,
                expectedModelSHA256: expectedModelSHA256, infoStandIntakeId: infoStandIntakeId)
            guard access == viewportAccessID, state?.revision == revision, state?.draft == current.draft else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            guard let next else { return false }
            current.draft = next
            try await persist(current)
            syncFields()
            notice = "비품 배치 입력을 이 Mac에 보관했습니다. 전시를 저장하세요."
            return true
        }
    }
    func applyWorkspaceBenchSeat(_ attachment: WorkspaceBenchSeatAttachment, access: UUID) async throws -> Bool {
        guard canEditWorkspaceBenchSeat else { throw AuthoringFailure("WORKSPACE_BENCH_SEAT_UNAVAILABLE") }
        return try await withWorkspaceFurnishingAccess(access) { api in
            guard var current = state else { throw AuthoringFailure("WORKSPACE_BENCH_SEAT_UNAVAILABLE") }
            let next = try await api.prepareWorkspaceBenchSeat(attachment, in: current)
            guard state?.revision == current.revision, state?.draft == current.draft,
                  !immutable, !benchDirty, !viewingPlanDirty, !architectureDirty, !formDirty, !positionDirty,
                  viewportController.editingContextActive, access == viewportAccessID else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            guard let next else { return false }
            current.draft = next
            try await persist(current)
            syncFields()
            notice = "착석 설정을 이 Mac에 보관했습니다. 전시를 저장하세요."
            return true
        }
    }
    func applyWorkspaceArtworkFrame(to target: String, access: UUID,
                                    prepare: (AuthoringAPI, NativeDocumentState) async throws -> NativeExhibitionDraft?) async throws -> Bool {
        guard canEditWorkspaceArtworkFrame else { throw AuthoringFailure("FRAME_SAVED_DRAFT_REQUIRED") }
        return try await withWorkspaceFurnishingAccess(access) { api in
            guard var current = state else { throw AuthoringFailure("FRAME_SAVED_DRAFT_REQUIRED") }
            guard target == placementId else { throw AuthoringFailure("FRAME_TARGET_STALE") }
            let next = try await prepare(api, current)
            guard access == viewportAccessID, state?.revision == current.revision, state?.draft == current.draft,
                  !formDirty, !positionDirty, !architectureDirty, !viewingPlanDirty, !immutable,
                  viewportController.editingContextActive, target == placementId else {
                throw AuthoringFailure("FRAME_TARGET_STALE")
            }
            guard let next else { return false }
            current.draft = next
            try await persist(current)
            syncFields()
            notice = "액자 변경을 이 Mac에 보관했습니다. 전시를 저장하세요."
            return true
        }
    }
    func applyWorkspacePortalFinish(openingId: String, furnishingId: String?, access: UUID) async throws -> Bool {
        guard canEditWorkspacePortalFinish else { throw AuthoringFailure("PORTAL_SAVED_DRAFT_REQUIRED") }
        return try await withWorkspaceFurnishingAccess(access) { api in
            guard var current = state else { throw AuthoringFailure("PORTAL_SAVED_DRAFT_REQUIRED") }
            let next = try await api.prepareWorkspacePortalFinish(openingId: openingId, furnishingId: furnishingId, in: current)
            guard access == viewportAccessID, state?.revision == current.revision, state?.draft == current.draft,
                  !formDirty, !positionDirty, !architectureDirty, !viewingPlanDirty, !immutable,
                  viewportController.editingContextActive else { throw AuthoringFailure("PORTAL_TARGET_STALE") }
            guard let next else { return false }
            current.draft = next
            try await persist(current)
            syncFields()
            notice = "출입구 마감 변경을 이 Mac에 보관했습니다. 전시를 저장하세요."
            return true
        }
    }
    func withWorkspaceMaterialAccess<T>(_ access: UUID, _ body: (AuthoringAPI) async throws -> T) async throws -> T {
        guard workspaceMaterialIntakeAvailable, access == viewportAccessID, !busy, !entrancePreviewActive,
              !placingArtwork, !architectureApplying, !pending, let api else {
            throw AuthoringFailure("WORKSPACE_MATERIAL_UNAVAILABLE")
        }
        busy = true
        defer { busy = false }
        guard await api.admitsWorkspaceMaterials else { throw AuthoringFailure("WORKSPACE_MATERIAL_UNAVAILABLE") }
        let result = try await body(api)
        guard access == viewportAccessID else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        return result
    }
    func applyWorkspaceSurfaceMaterial(_ binding: WorkspaceSurfaceMaterialBinding, access: UUID) async throws -> Bool {
        try await withWorkspaceMaterialAccess(access) { api in
            guard !immutable, !formDirty, !positionDirty, !architectureDirty, !viewingPlanDirty, viewingPointInput == nil,
                  var current = state else { throw AuthoringFailure("ARCHITECTURE_APPLY_REQUIRED") }
            let revision = current.revision
            let next = try await api.prepareWorkspaceSurfaceMaterial(binding, in: current)
            guard access == viewportAccessID, state?.revision == revision, state?.draft == current.draft else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            guard let next else { return false }
            current.draft = next
            try await persist(current)
            syncFields()
            notice = "재질 연결을 이 Mac에 보관했습니다. 전시를 저장하세요."
            return true
        }
    }
    private func work(allowViewingPlanInput: Bool = false, allowViewingPointInput: Bool = false, allowInfoStandPreview: Bool = false, _ body: () async throws -> Void) async {
        guard !entrancePreviewActive, !busy, !placingArtwork, api != nil, session != nil,
              allowInfoStandPreview || !infoStandPreviewing else { return }
        guard allowViewingPointInput || viewingPointInput == nil else {
            notice = "감상 시점 입력을 추가하거나 취소한 뒤 진행하세요."
            return
        }
        guard allowViewingPlanInput || !viewingPlanDirty else {
            notice = "관람 계획 입력을 확인하고 적용하거나 취소한 뒤 진행하세요."
            return
        }
        busy = true; failure = nil; notice = ""
        defer { busy = false }
        do { try await body() }
        catch { failure = error as? AuthoringFailure ?? AuthoringFailure((error as? BridgeFailure)?.code ?? "CONNECTION_FAILED") }
    }
    private func persist(_ value: NativeDocumentState, recordUndo: Bool = true) async throws {
        guard let write else { throw AuthoringFailure("EXHIBITION_STORE_REQUIRED") }
        let old = state?.draft
        state = try await write(value)
        if old?.id != value.draft?.id { past = []; future = []; viewportSelection = nil; selectedLightId = ""; selectedBenchId = ""; selectedPlaqueId = ""; exportedURL = nil; exportedSHA256 = nil }
        else if recordUndo, let old, old.candidate != value.draft?.candidate {
            past.append(old); future = []
            while past.count > 30 || past.reduce(0, { $0 + ((try? JSONEncoder().encode($1).count) ?? 1_048_576) }) > 8 * 1024 * 1024 { past.removeFirst() }
        }
    }
    var walkingCollisionAvailable: Bool {
        api != nil && session?.advertisesFurnishingWalkingCollision == true
            && !selectedBoundedPolicy && !ordinaryBoundedRuntime
            && draft != nil && !pending && !boundedPolicyPending
    }

    private var walkingCollisionEditable: Bool {
        walkingCollisionAvailable && !immutable && !placingArtwork && !entrancePreviewActive
            && !formDirty && !positionDirty && !architectureDirty && !architectureApplying
            && !viewingPlanDirty && viewingPointInput == nil && viewportController.editingContextActive
    }

    var canSetWalkingCollision: Bool { walkingCollisionEditable && !busy }

    func walkingCollisionTargets(revision: UInt64) async throws -> [FurnishingWalkingCollisionState] {
        guard walkingCollisionAvailable, let api, let base = state, base.revision == revision else {
            throw AuthoringFailure("FURNISHING_WALKING_COLLISION_UNAVAILABLE")
        }
        let access = viewportAccessID
        let rows = try await api.furnishingWalkingCollisionTargets(in: base)
        try Task.checkCancellation()
        guard self.api === api, access == viewportAccessID, state?.revision == revision,
              state?.draft == base.draft, walkingCollisionAvailable else {
            throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
        }
        return rows
    }

    func setWalkingCollision(_ target: FurnishingWalkingCollisionTarget, setting: FurnishingWalkingCollisionSetting,
                             revision: UInt64) async {
        await work {
            guard walkingCollisionEditable, let api, var current = state, current.revision == revision else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            let access = viewportAccessID, context = viewportController.editingContextGeneration
            let next = try await api.prepareFurnishingWalkingCollision(target: target, setting: setting, in: current)
            try Task.checkCancellation()
            guard self.api === api, access == viewportAccessID, context == viewportController.editingContextGeneration,
                  state?.revision == revision, state?.draft == current.draft, walkingCollisionEditable else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            guard let next else { return }
            current.draft = next
            try await persist(current)
            syncFields()
            notice = "걷기 충돌 설정을 이 Mac에 보관했습니다. 실행 취소로 되돌릴 수 있습니다."
        }
    }

    func acceptViewportCandidate(_ candidate: DocumentValue, base: NativeExhibitionDraft) async {
        await work {
            guard (!immutable || boundedPlacementReady), !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, draft == base, var value = state, var next = draft else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            if boundedPlacementCapability != nil {
                guard boundedPlacementReady, let selection = viewportSelection, let baseline = boundedPlacementBaseline else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                try next.acceptBoundedPlacement(candidate, selection: selection, savedBaseline: baseline)
            } else { try next.acceptGeometry(candidate) }
            value.draft = next
            try await persist(value); syncFields()
            notice = "3D 변경을 이 Mac에 보관했습니다. 전시 저장은 별도로 진행하세요."
        }
    }
    var canUseRoomBaseLight: Bool {
        session?.advertisesRoomBaseLight == true && viewportController.roomBaseLightSupported
            && viewportController.connected && viewportController.editingContextActive
            && !ordinaryBoundedRuntime && boundedPlacementCapability == nil && !immutable
    }
    var canEnsureRoomBaseLight: Bool {
        canUseRoomBaseLight && !roomId.isEmpty && viewportSelection == ViewportSelection(kind: "room", id: roomId)
    }
    func acceptRoomBaseLightCandidate(_ candidate: DocumentValue, base: NativeExhibitionDraft,
                                      action: RoomBaseLightAction, access: UUID) async {
        await work {
            guard canUseRoomBaseLight, access == viewportAccessID, !pending, !formDirty, !positionDirty,
                  !architectureDirty, !architectureApplying, !placingArtwork,
                  draft == base, var value = state, var next = draft else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            if case .ensure(let target) = action, target != roomId { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            guard try action.apply(candidate, to: &next, selected: viewportSelection) else {
                notice = "이 공간에는 이미 기본 조명이 있습니다."
                return
            }
            value.draft = next
            try await persist(value); syncFields()
            notice = "공간의 기본 조명을 이 Mac에 보관했습니다. 전시 저장은 별도로 진행하세요."
        }
    }
    func acceptInstallationCandidate(_ candidate: DocumentValue, base: NativeExhibitionDraft, command: InstallationCommand) async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, draft == base,
                  command.placementId.lowercased() == placementId.lowercased(), var value = state, var next = draft else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            if case .alignOnVerifiedPlinth(let id) = command {
                guard viewportController.connected, viewportController.plinthEnvelopeAlignSupported,
                      viewportSelection == ViewportSelection(kind: "placement", id: id), selectedHasFurnishing else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            }
            try next.acceptInstallationTranslation(candidate, placementId: placementId, axes: command.translationAxes)
            value.draft = next; try await persist(value); syncFields()
            notice = "작품 위치를 이 Mac에 보관했습니다. 실행 취소로 되돌릴 수 있습니다."
        }
    }
    func acceptArchitectureCandidate(_ candidate: DocumentValue, base: NativeExhibitionDraft, command: ArchitectureCommand) async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !surfaceMaterialDirty, !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, architectureApplying,
                  draft == base, try architecture?.command() == command,
                  var value = state, var next = draft else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            try next.acceptArchitectureGeometry(candidate, command: command, selected: viewportSelection); value.draft = next
            try await persist(value); syncFields()
            notice = "공간 변경을 이 Mac에 보관했습니다. 실행 취소로 되돌릴 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    private func syncSurfaceMaterial() {
        clearSurfaceMaterialPreview()
        surfaceMaterial = nil; surfaceMaterialBaseline = nil; surfaceMaterialStatus = ""
        surfaceMaterialTexture = nil; surfaceMaterialTextureBaseline = nil
        guard let draft, let selection = viewportSelection, selection.kind == "surface" else { return }
        do {
            let fields = try SurfaceMaterialFormBuffer(draft: draft, selection: selection)
            surfaceMaterial = fields; surfaceMaterialBaseline = fields
            if let surface = draft.candidate["surfaces"].items?.first(where: { $0["id"].string == selection.id }),
               ["wall", "ceiling"].contains(surface["type"].string ?? "") {
                let texture = try SurfaceMaterialTextureFormBuffer(draft: draft, selection: selection)
                surfaceMaterialTexture = texture; surfaceMaterialTextureBaseline = texture
            }
        } catch { surfaceMaterialStatus = "이 표면의 재질 설정은 현재 편집할 수 없습니다. 저장된 설정은 유지됩니다." }
    }
    func loadSurfaceMaterialCatalog() async {
        guard surfaceMaterialTexturesSupported, !surfaceMaterialCatalogLoading,
              surfaceMaterialCatalogAccessID != viewportAccessID, let api else { return }
        let requestID = UUID()
        surfaceMaterialCatalogRequestID = requestID
        surfaceMaterialCatalogLoading = true
        defer { if surfaceMaterialCatalogRequestID == requestID { surfaceMaterialCatalogLoading = false; surfaceMaterialCatalogRequestID = nil } }
        let access = viewportAccessID
        do {
            let catalog = try await api.surfaceMaterials()
            try Task.checkCancellation()
            guard access == viewportAccessID, surfaceMaterialCatalogRequestID == requestID, surfaceMaterialTexturesSupported else { return }
            surfaceMaterialCatalog = catalog; surfaceMaterialCatalogAccessID = access; surfaceMaterialCatalogStatus = ""
            for item in catalog.items where surfaceMaterialImages[item.id] == nil {
                guard let preview = item.preview else { continue }
                // Optional imagery never blocks the authenticated named choice.
                do {
                    let bytes = try await api.surfaceMaterialPreview(preview)
                    try Task.checkCancellation()
                    guard access == viewportAccessID, surfaceMaterialCatalogRequestID == requestID,
                          surfaceMaterialTexturesSupported else { return }
                    if let source = CGImageSourceCreateWithData(bytes as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
                       let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true,
                            kCGImageSourceThumbnailMaxPixelSize: 128, kCGImageSourceShouldCacheImmediately: true] as CFDictionary) {
                        surfaceMaterialImages[item.id] = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
                    }
                } catch {
                    guard !Task.isCancelled, access == viewportAccessID, surfaceMaterialCatalogRequestID == requestID else { return }
                }
            }
        } catch {
            guard !Task.isCancelled, access == viewportAccessID, surfaceMaterialCatalogRequestID == requestID else { return }
            surfaceMaterialCatalogAccessID = access
            surfaceMaterialCatalogStatus = "재질 목록을 읽지 못했습니다. 연결을 확인한 뒤 다시 불러오세요."
        }
    }
    func retrySurfaceMaterialCatalog() async { surfaceMaterialCatalogAccessID = nil; await loadSurfaceMaterialCatalog() }
    func selectSurfaceMaterial(_ id: String) {
        guard canEditSurfaceMaterial, surfaceMaterialTexturesSupported, surfaceMaterialCatalogAccessID == viewportAccessID,
              surfaceMaterialCatalog?.items.contains(where: { $0.id == id }) == true, var fields = surfaceMaterialTexture else { return }
        do { try fields.select(id); surfaceMaterialTexture = fields }
        catch { failure = error as? AuthoringFailure ?? AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
    }
    func removeSurfaceMaterialTexture() {
        guard canEditSurfaceMaterial, surfaceMaterialTexturesSupported, var fields = surfaceMaterialTexture else { return }
        fields.remove(); surfaceMaterialTexture = fields
    }
    func resetSurfaceMaterialTextureValues() {
        guard canEditSurfaceMaterial, surfaceMaterialTexturesSupported, var fields = surfaceMaterialTexture else { return }
        do { try fields.useRecommendedValues(); surfaceMaterialTexture = fields }
        catch { failure = error as? AuthoringFailure ?? AuthoringFailure("SURFACE_MATERIAL_TEXTURE_INVALID") }
    }
    func plaquePlacementTitle(_ id: String) -> String {
        guard let draft, let placement = draft.placements.first(where: { $0["id"].string?.lowercased() == id.lowercased() }),
              let revision = placement["artworkRevisionId"].string,
              let artwork = draft.artworks.first(where: { $0["revisionId"].string?.lowercased() == revision.lowercased() }) else { return "작품 배치" }
        return artwork["metadata"]["title"].string ?? "작품 배치"
    }
    func plaqueSourceTitle(_ plaque: PlaqueSettings) -> String {
        switch plaque.source {
        case .exhibition: "전시 · " + (draft?.title ?? "전시")
        case .room: "공간 · " + (draft?.rooms.first(where: { $0["id"].string?.lowercased() == plaque.roomId.lowercased() })?["name"].string ?? "공간")
        case .artwork(let id): "작품 · " + plaquePlacementTitle(id)
        }
    }
    private func syncPlaque() {
        plaqueAdding = false; plaqueRemoving = false
        plaque = nil; plaqueBaseline = nil; plaqueStatus = ""
        guard let draft, !selectedPlaqueId.isEmpty else { return }
        do {
            let fields = try PlaqueFormBuffer(draft: draft, plaqueId: selectedPlaqueId)
            plaque = fields; plaqueBaseline = fields
        } catch { plaqueStatus = "이 명판의 설정은 현재 편집할 수 없습니다. 저장된 설정은 유지됩니다." }
    }
    private func clearPlaqueSelection() { selectedPlaqueId = ""; syncPlaque() }
    func selectPlaque(_ id: String) {
        guard canSelectPlaque, id.isEmpty || plaques.contains(where: { $0.id == id }) else { return }
        selectedPlaqueId = id
        if !id.isEmpty {
            selectedLightId = ""; syncLight(); clearBenchSelection()
            viewportSelection = nil; catalogArtworkId = ""; syncArchitecture()
        }
        syncPlaque()
    }
    func beginPlaque() {
        guard canAddPlaque else { return }
        do {
            guard let draft else { return }
            var fields = try PlaqueFormBuffer(roomId: roomId)
            try fields.selectRoom(roomId, in: draft)
            selectedLightId = ""; syncLight(); clearBenchSelection()
            viewportSelection = nil; catalogArtworkId = ""; syncArchitecture()
            selectedPlaqueId = fields.id; plaqueAdding = true; plaqueRemoving = false
            plaqueBaseline = nil; plaque = fields
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("PLAQUE_FIELDS_INVALID") }
    }
    private func currentPlaqueCommand() throws -> PlaqueCommand {
        if plaqueRemoving { return .remove(id: selectedPlaqueId) }
        guard let plaque, plaque.id == selectedPlaqueId else { throw AuthoringFailure("PLAQUE_SELECTION_CHANGED") }
        let settings = try plaque.settings()
        let introduction = plaque.roomIntroductionChanged ? plaque.roomIntroductionText : nil
        return plaqueAdding ? .add(settings, roomIntroductionText: introduction) : .set(settings, roomIntroductionText: introduction)
    }
    func discardPlaque() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        if plaqueAdding { selectedPlaqueId = "" }
        syncPlaque()
    }
    func previewPlaqueRemoval() async {
        guard canRemovePlaque else { return }
        plaqueRemoving = true
        await previewPlaque()
    }
    func previewPlaque() async {
        guard canPreviewPlaque, let base = draft else { return }
        let access = viewportAccessID, selected = selectedPlaqueId, fields = plaque
        do {
            let command = try currentPlaqueCommand()
            _ = try command.proposedCandidate(in: base, selectedPlaqueId: selected)
            architectureApplying = true
            defer { architectureApplying = false }
            // Keep the enabled source frame stable while shared auth/validation
            // owns this request. Busy would send a disabled replacement frame.
            let preview = try await viewportController.plaqueInspector(command, selectedPlaqueId: selected)
            await work {
                guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                      !surfaceMaterialDirty, !lightDirty, !benchDirty, !furnishingDirty && !spaceLightingDirty, architectureApplying, viewportAccessID == access,
                      draft == base, preview.base == base, selectedPlaqueId == selected, plaque == fields,
                      try currentPlaqueCommand() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                plaquePreview = preview; plaquePreviewAccessID = access
                plaqueStatus = plaqueRemoving ? "명판을 뺀 상태를 미리 보고 있습니다. 적용 전 저장 내용은 바뀌지 않습니다." : "확인한 명판을 미리 보고 있습니다. 적용 전 저장 내용은 바뀌지 않습니다."
            }
        } catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure("PLAQUE_PREVIEW_FAILED")
            viewportController.diagnose("PLAQUE", error)
        }
    }
    func applyPlaque() async {
        guard canApplyPlaque else { return }
        await work {
            guard !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                  !surfaceMaterialDirty, !lightDirty, !benchDirty, !furnishingDirty, !spaceLightingDirty, !architectureApplying,
                  viewportController.connected, viewportController.plaqueInspectorSupported,
                  plaquePreviewAccessID == viewportAccessID, let preview = plaquePreview,
                  !editingLocked(acknowledging: preview.displayDraft),
                  let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, selectedPlaqueId: selectedPlaqueId, command: currentPlaqueCommand())
            try await persist(value); syncFields()
            notice = "명판 변경을 이 Mac에 보관했습니다. 되돌리기로 취소할 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    private func syncBench() {
        benchAdding = false; benchRemoving = false
        bench = nil; benchBaseline = nil; benchStatus = ""
        guard let draft, !selectedBenchId.isEmpty else { return }
        do {
            let fields = try BenchFormBuffer(draft: draft, benchId: selectedBenchId)
            bench = fields; benchBaseline = fields
        } catch { benchStatus = "이 벤치의 설정은 현재 편집할 수 없습니다. 저장된 설정은 유지됩니다." }
    }
    private func clearBenchSelection() { selectedBenchId = ""; syncBench() }
    func selectBench(_ id: String) {
        guard canSelectBench, id.isEmpty || benches.contains(where: { $0.id == id }) else { return }
        clearPlaqueSelection()
        selectedBenchId = id
        if !id.isEmpty { selectedLightId = ""; syncLight(); viewportSelection = nil; catalogArtworkId = ""; syncArchitecture() }
        syncBench()
    }
    func beginBench() {
        guard canAddBench else { return }
        do {
            let fields = try BenchFormBuffer(roomId: roomId)
            clearPlaqueSelection()
            selectedLightId = ""; syncLight(); viewportSelection = nil; catalogArtworkId = ""; syncArchitecture()
            selectedBenchId = fields.id; benchAdding = true; benchRemoving = false
            benchBaseline = nil; bench = fields
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("BENCH_FIELDS_INVALID") }
    }
    private func currentBenchCommand() throws -> BenchCommand {
        if benchRemoving { return .remove(id: selectedBenchId) }
        guard let bench, bench.id == selectedBenchId else { throw AuthoringFailure("BENCH_SELECTION_CHANGED") }
        let settings = try bench.settings(), viewpoint = try bench.newViewpoint()
        return benchAdding ? .add(settings, viewpoint: viewpoint) : .set(settings, viewpoint: viewpoint)
    }
    func matchBenchViewpoint() {
        guard canEditBench, !benchRemoving, var fields = bench else { return }
        do { try fields.matchNewViewpointToBench(); bench = fields }
        catch { failure = error as? AuthoringFailure ?? AuthoringFailure("BENCH_FIELDS_INVALID") }
    }
    func discardBench() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        if benchAdding { selectedBenchId = "" }
        syncBench()
    }
    func previewBenchRemoval() async {
        guard canRemoveBench else { return }
        benchRemoving = true
        await previewBench()
    }
    func previewBench() async {
        guard canPreviewBench, let base = draft else { return }
        let access = viewportAccessID, selected = selectedBenchId, fields = bench
        do {
            let command = try currentBenchCommand()
            _ = try command.proposedCandidate(in: base, selectedBenchId: selected)
            architectureApplying = true
            defer { architectureApplying = false }
            // Preserve the enabled source frame during shared authenticated
            // validation. Busy would replace it with a disabled frame.
            let preview = try await viewportController.benchInspector(command, selectedBenchId: selected)
            await work {
                guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                      !surfaceMaterialDirty, !lightDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, architectureApplying, viewportAccessID == access,
                      draft == base, preview.base == base, selectedBenchId == selected, bench == fields,
                      try currentBenchCommand() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                benchPreview = preview; benchPreviewAccessID = access
                benchStatus = benchRemoving ? "벤치를 뺀 상태를 미리 보고 있습니다. 적용 전 저장 내용은 바뀌지 않습니다." : "확인한 벤치를 미리 보고 있습니다. 적용 전 저장 내용은 바뀌지 않습니다."
            }
        } catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure("BENCH_PREVIEW_FAILED")
            viewportController.diagnose("BENCH", error)
        }
    }
    func applyBench() async {
        guard canApplyBench else { return }
        await work {
            guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                  !surfaceMaterialDirty, !lightDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying, benchPreviewAccessID == viewportAccessID,
                  let preview = benchPreview, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, selectedBenchId: selectedBenchId, command: currentBenchCommand())
            try await persist(value); syncFields()
            notice = "벤치 변경을 이 Mac에 보관했습니다. 되돌리기로 취소할 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    private func syncViewingPlan() {
        viewingPlan = nil; viewingPlanBaseline = nil
        guard let draft else { return }
        do {
            let fields = try ViewingPlanFormBuffer(draft: draft)
            viewingPlan = fields; viewingPlanBaseline = fields
        } catch {
            viewingPlanStatus = "이 전시의 관람 계획은 현재 편집할 수 없습니다. 저장된 정보는 유지됩니다."
        }
    }
    func discardViewingPlan() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        syncViewingPlan()
    }
    func previewViewingPlan() async {
        guard canPreviewViewingPlan, let fields = viewingPlan, let base = draft, let hostEpoch = viewportController.hostEpoch else { return }
        let access = viewportAccessID, context = viewportController.editingContextGeneration
        do {
            let command = try fields.command(in: base)
            architectureApplying = true
            defer { architectureApplying = false }
            let preview = try await viewportController.viewingPlan(command)
            guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty,
                  !architectureGeometryDirty, !surfaceMaterialDirty, !lightDirty, !benchDirty,
                  !plaqueDirty, !spaceLightingDirty, !furnishingDirty,
                  viewportAccessID == access, viewportController.editingContextActive,
                  viewportController.editingContextGeneration == context, viewportController.hostEpoch == hostEpoch, draft == base,
                  preview.base == base, viewingPlan == fields else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            viewingPlanPreview = preview; viewingPlanPreviewAccessID = access
            viewingPlanPreviewContext = context; viewingPlanPreviewSequence = viewportController.acceptedSequence
            viewingPlanPreviewHostEpoch = hostEpoch
            viewingPlanStatus = "관람 계획의 형식을 확인했습니다. 적용하면 이 Mac에 보관합니다. 관람 중 안전한 시점은 별도로 확인합니다."
        } catch {
            viewingPlanStatus = "관람 계획을 확인하지 못했습니다. 입력과 기존 전시는 유지됩니다. 연결과 시점 선택을 확인하세요."
            viewportController.diagnose("VIEWING_PLAN", error)
        }
    }
    func applyViewingPlan() async {
        guard canApplyViewingPlan else { return }
        await work(allowViewingPlanInput: true) {
            guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty,
                  !architectureGeometryDirty, !surfaceMaterialDirty, !lightDirty, !benchDirty,
                  !plaqueDirty, !spaceLightingDirty, !furnishingDirty, !architectureApplying,
                  viewportController.editingContextActive, viewportController.connected, viewportController.viewingPlanSupported,
                  viewingPlanPreviewAccessID == viewportAccessID,
                  viewingPlanPreviewContext == viewportController.editingContextGeneration,
                  viewingPlanPreviewHostEpoch == viewportController.hostEpoch,
                  viewingPlanPreviewSequence == viewportController.acceptedSequence,
                  let preview = viewingPlanPreview, let fields = viewingPlan, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, command: fields.command(in: draft))
            try await persist(value); syncFields()
            notice = "관람 계획을 이 Mac에 보관했습니다. 실행 취소로 되돌릴 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    func configureFloorRuntime(_ configuration: NativeConfiguration?, ready: Bool) {
        let privateFloor = NativePrivateFloorCatalog.supports(manifestSHA256: configuration?.manifestSha256, ready: ready)
        if privateFloorRuntimeAvailable != privateFloor {
            privateFloorRuntimeAvailable = privateFloor
            clearPrivateFloorCatalog()
        }
        let available = privateFloor || NativeFloorRuntime.supports(manifestSHA256: configuration?.manifestSha256, ready: ready)
        if floorRuntimeAvailable != available { floorRuntimeAvailable = available }
        let audioAvailable = NativeFloorAudioRuntime.supports(manifestSHA256: configuration?.manifestSha256, ready: ready)
        if floorAudioRuntimeAvailable != audioAvailable {
            floorAudioRuntimeAvailable = audioAvailable
            floorPreviewAccessID = nil; floorPreviewContext = nil; floorPreviewHostEpoch = nil
        }
        if !floorRuntimeAvailable { floorPreviewAccessID = nil; floorPreviewContext = nil; floorPreviewHostEpoch = nil }
    }
    var canEditFloor: Bool {
        floorEditable && floorRuntimeAvailable && viewportController.connected && viewportController.editingContextActive
        && !entrancePreviewActive && !busy && !pending && !immutable && !placingArtwork
        && !nonFloorFormDirty && !positionDirty && !architectureDirty && !architectureApplying
    }
    var canApplyFloor: Bool {
        guard canEditFloor, floorDirty, floorPreviewBase == draft, floorPreviewAccessID == viewportAccessID,
              floorPreviewContext == viewportController.editingContextGeneration, floorPreviewHostEpoch == viewportController.hostEpoch,
              let preview = floorPreview, let credentials = viewportCredentials, var next = draft, let target = floorTarget else { return false }
        do {
            try setFloorCandidate(&next, target: target)
            return next.candidate == preview.candidate && viewportController.confirmsFloorPreview(preview, tenantId: credentials.tenantId, principalId: credentials.principalId)
        } catch { return false }
    }
    private func syncFloor() {
        floorPreview = nil; floorPreviewBase = nil; floorPreviewAccessID = nil; floorPreviewContext = nil; floorPreviewHostEpoch = nil
        do {
            if !floorTargets.contains(where: { $0.key == floorTargetKey }) { floorTargetKey = floorTargets.first?.key ?? "" }
            let settings = try draft.flatMap { draft in try floorTarget.flatMap { try FloorSettings.read(from: draft, target: $0) } }
            floorEditable = floorTarget != nil
            floorAudioIntentBound = false; floorBaseline = FloorFormBuffer(settings: settings); floor = floorBaseline; floorStatus = ""
            floorPreset = settings?.preset ?? .wood
            floorScope = Set(FloorSettings.Field.allCases).subtracting(floorCustomized)
        } catch { floorEditable = false; floorStatus = "바닥 설정을 읽지 못했습니다. 원래 설정은 유지됩니다." }
    }
    func selectFloor(_ key: String) {
        guard !entrancePreviewActive, !busy, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, !placingArtwork,
              floorTargets.contains(where: { $0.key == key }) else { return }
        floorTargetKey = key; syncFloor()
    }
    private func refreshFloorPreview() {
        floorPreviewAccessID = nil; floorPreviewContext = nil; floorPreviewHostEpoch = nil
        guard floorDirty else { floorPreview = nil; floorPreviewBase = nil; floorStatus = ""; return }
        guard floorEditable, floorRuntimeAvailable, let target = floorTarget, let base = draft, var next = draft else { return }
        do {
            try setFloorCandidate(&next, target: target)
            floorPreview = next; floorPreviewBase = base; floorPreviewAccessID = viewportAccessID
            floorPreviewContext = viewportController.editingContextGeneration; floorPreviewHostEpoch = viewportController.hostEpoch
            floorStatus = "입력한 바닥 설정을 공간에 표시하고 있습니다. 화면을 확인한 뒤 적용하세요."
        } catch { floorStatus = "입력을 확인하세요. 입력은 유지되며 마지막 유효 미리보기를 표시합니다." }
    }
    func previewFloor() { guard canEditFloor else { return }; refreshFloorPreview() }
    private func setFloorCandidate(_ next: inout NativeExhibitionDraft, target: FloorSettings.Target) throws {
        if floor.concreteFootstepsRequested {
            try next.replaceCandidate(ConcreteFootsteps.proposedCandidate(in: next, target: target, resourcesAvailable: floorAudioRuntimeAvailable))
            return
        }
        let settings = try floor.value(candidate: next.candidate)
        let declare = try floor.declareSoundResources(available: floorAudioRuntimeAvailable, requiresQualifiedRuntime: floorAudioIntentBound)
        try next.setFloor(settings, target: target, declareDefaultSound: declare)
    }
    var canStageConcreteFootsteps: Bool {
        guard canEditFloor, !floorDirty, floorAudioRuntimeAvailable, let draft, let target = floorTarget else { return false }
        return (try? ConcreteFootsteps.proposedCandidate(in: draft, target: target, resourcesAvailable: true)) != nil
    }
    func stageConcreteFootsteps() {
        guard canStageConcreteFootsteps else { return }
        var fields = floor; fields.concreteFootstepsRequested = true; floor = fields
    }
    func stageFloorPreset(_ preset: FloorSettings.Preset, fields: Set<FloorSettings.Field>) {
        stageFloorMaterial(preset, defaults: FloorSettings.defaults(preset), fields: fields)
    }
    private func stageFloorMaterial(_ preset: FloorSettings.Preset, defaults: [FloorSettings.Field: DocumentValue],
                                    fields: Set<FloorSettings.Field>) {
        guard canEditFloor, !fields.isEmpty, let draft, let target = floorTarget else { return }
        do {
            let prior = try floor.settings.map { _ in try floor.value(candidate: draft.candidate) } ?? nil
            try floor.stage(FloorSettings.applying(preset: preset, defaults: defaults, fields: fields,
                target: target, prior: prior, candidate: draft.candidate), replacing: fields, candidate: draft.candidate)
        } catch { floorStatus = "유효한 입력을 먼저 적용하거나 되돌린 뒤 재질을 선택하세요." }
    }
    var canStagePrivateFloor: Bool {
        canEditFloor && privateFloorRuntimeAvailable && privateFloorCatalog != nil
            && floorTarget?.kind == .surface && !floorDirty
    }
    func stagePrivateFloor() {
        guard canStagePrivateFloor else { return }
        stageFloorMaterial(.wood, defaults: NativePrivateFloorCatalog.defaults, fields: Set(NativePrivateFloorCatalog.defaults.keys))
    }
    private func clearPrivateFloorCatalog() {
        privateFloorRequestID = nil
        privateFloorCatalog = nil
        privateFloorImage = nil
        privateFloorLoadFailed = false
    }
    func loadPrivateFloorCatalog() async {
        guard privateFloorRuntimeAvailable, privateFloorCatalog == nil, privateFloorRequestID == nil, let api else { return }
        let requestID = UUID()
        let access = viewportAccessID
        privateFloorRequestID = requestID
        privateFloorLoadFailed = false
        defer { if privateFloorRequestID == requestID { privateFloorRequestID = nil } }
        do {
            let catalog = try await api.privateFloorCatalog()
            try Task.checkCancellation()
            guard access == viewportAccessID, privateFloorRequestID == requestID, privateFloorRuntimeAvailable else { return }
            privateFloorCatalog = catalog
            // A missing thumbnail never blocks a named material choice.
            let bytes = try await api.privateFloorPreview(catalog)
            try Task.checkCancellation()
            guard access == viewportAccessID, privateFloorRequestID == requestID, privateFloorRuntimeAvailable,
                  let source = CGImageSourceCreateWithData(bytes as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
                  let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceThumbnailMaxPixelSize: 128, kCGImageSourceShouldCacheImmediately: true] as CFDictionary) else { return }
            privateFloorImage = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
        } catch {
            guard !Task.isCancelled, access == viewportAccessID, privateFloorRequestID == requestID else { return }
            privateFloorLoadFailed = privateFloorCatalog == nil
        }
    }
    func resetFloorField(_ field: FloorSettings.Field) {
        guard canEditFloor, floorCanReset, let draft, let persisted = floorBaseline.settings else { return }
        do { try floor.reset(field, to: persisted.preset, candidate: draft.candidate) }
        catch { floorStatus = "현재 보관한 재질의 기본값을 적용하지 못했습니다. 다른 입력은 유지됩니다." }
    }
    func discardFloor() { guard !busy, !pending, !architectureApplying, !placingArtwork else { return }; syncFloor() }
    func applyFloor() async {
        guard canApplyFloor else { return }
        await work {
            guard !immutable, !pending, floorEditable, floorRuntimeAvailable, !nonFloorFormDirty, !positionDirty,
                  !architectureDirty, !architectureApplying, floorPreviewBase == draft, floorPreviewAccessID == viewportAccessID,
                  floorPreviewContext == viewportController.editingContextGeneration, floorPreviewHostEpoch == viewportController.hostEpoch,
                  let target = floorTarget, let preview = floorPreview, let credentials = viewportCredentials,
                  viewportController.editingContextActive, viewportController.confirmsFloorPreview(preview, tenantId: credentials.tenantId, principalId: credentials.principalId),
                  var value = state, var next = draft else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            try setFloorCandidate(&next, target: target)
            guard next.candidate == preview.candidate else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            value.draft = next; try await persist(value); syncFloor()
            notice = "바닥 설정을 이 Mac에 보관했습니다. 실행 취소로 되돌리거나 전시를 저장하세요."
        }
    }
    func loadFloorImages() async {
        guard floorRuntimeAvailable, !floorImagesLoading, let api else { return }
        let access = viewportAccessID
        floorImagesLoading = true
        defer { floorImagesLoading = false }
        for preset in FloorSettings.Preset.allCases {
            guard !Task.isCancelled, access == viewportAccessID, floorRuntimeAvailable else { return }
            guard !floorImageAttempts.contains(preset) else { continue }
            floorImageAttempts.insert(preset)
            do {
                let bytes = try await api.floorPreview(preset)
                guard !Task.isCancelled, access == viewportAccessID, floorRuntimeAvailable else { return }
                guard let source = CGImageSourceCreateWithData(bytes as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
                      let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true,
                        kCGImageSourceThumbnailMaxPixelSize: 128, kCGImageSourceShouldCacheImmediately: true] as CFDictionary) else { throw AuthoringFailure("FLOOR_PREVIEW_INVALID") }
                floorImages[preset] = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
            } catch {
                if Task.isCancelled, access == viewportAccessID { floorImageAttempts.remove(preset) }
                // A missing thumbnail never blocks a named material choice.
            }
        }
    }
    private func syncSpaceLighting() {
        spaceLighting = nil; spaceLightingBaseline = nil; spaceLightingStatus = ""
        guard let draft else { return }
        do { let fields = try SpaceLightingFormBuffer(draft: draft); spaceLighting = fields; spaceLightingBaseline = fields }
        catch { spaceLightingStatus = "이 전시의 조명 설정은 현재 편집할 수 없습니다. 저장된 설정은 유지됩니다." }
    }
    func discardSpaceLighting() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        syncSpaceLighting()
    }
    func previewSpaceLighting() async {
        guard canPreviewSpaceLighting, let fields = spaceLighting, let base = draft else { return }
        let access = viewportAccessID
        do {
            let command = try fields.command()
            architectureApplying = true
            defer { architectureApplying = false }
            let preview = try await viewportController.spaceSettings(command)
            await work {
                guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty, !surfaceMaterialDirty, !lightDirty, !benchDirty, !plaqueDirty,
                      architectureApplying, viewportAccessID == access, draft == base, preview.base == base,
                      try spaceLighting?.command() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                spaceLightingPreview = preview; spaceLightingPreviewAccessID = access
                spaceLightingStatus = "확인한 전시 조명을 미리 보고 있습니다. 취소하면 원래 표시로 돌아갑니다."
            }
        } catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure("SPACE_LIGHTING_PREVIEW_FAILED")
            viewportController.diagnose("SPACE_LIGHTING", error)
        }
    }
    func applySpaceLighting() async {
        guard canApplySpaceLighting else { return }
        await work {
            guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                  !surfaceMaterialDirty, !lightDirty, !benchDirty, !plaqueDirty, !architectureApplying,
                  viewportController.connected, viewportController.spaceSettingsSupported, spaceLightingPreviewAccessID == viewportAccessID,
                  let preview = spaceLightingPreview, let fields = spaceLighting, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, command: fields.command())
            try await persist(value); syncFields()
            notice = "전시 조명을 이 Mac에 보관했습니다. 되돌리기로 취소할 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    private func syncLight() {
        light = nil; lightBaseline = nil; lightStatus = ""
        guard let draft, !selectedLightId.isEmpty else { return }
        do {
            let fields = try LightFormBuffer(draft: draft, selectedLightId: selectedLightId)
            light = fields; lightBaseline = fields
        } catch { lightStatus = "이 조명의 설정은 현재 편집할 수 없습니다. 저장된 설정은 유지됩니다." }
    }
    func selectLight(_ id: String) {
        guard canSelectLight, let draft,
              id.isEmpty || draft.candidate["lights"].items?.contains(where: { $0["id"].string == id }) == true else { return }
        clearBenchSelection(); clearPlaqueSelection()
        selectedLightId = id
        // Native list selection is independent of the strict-v1 viewport kind.
        // Never send kind:"light" to an existing receiver.
        if !id.isEmpty { viewportSelection = nil; catalogArtworkId = ""; syncArchitecture() }
        syncLight()
    }
    func discardLight() {
        guard !busy, !pending, !architectureApplying, !placingArtwork else { return }
        syncLight()
    }
    func previewLight() async {
        guard canPreviewLight, let fields = light, let base = draft else { return }
        let access = viewportAccessID, selected = selectedLightId
        do {
            let command = try fields.command()
            _ = try command.encodedLight(in: base, selectedLightId: selected)
            architectureApplying = true
            defer { architectureApplying = false }
            // Busy is encoded as a disabled frame. Like surface preview, keep
            // that frame stable while the shared validator owns this request.
            let preview = try await viewportController.lightInspector(command, selectedLightId: selected)
            await work {
                guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty, !surfaceMaterialDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty,
                      architectureApplying, viewportAccessID == access, draft == base, preview.base == base, selectedLightId == selected,
                      try light?.command() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                lightPreview = preview; lightPreviewAccessID = access
                lightStatus = "확인한 조명을 미리 보고 있습니다. 적용하면 이 Mac에 보관하며 취소하면 원래 표시로 돌아갑니다."
            }
        } catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure("LIGHT_PREVIEW_FAILED")
            viewportController.diagnose("LIGHT", error)
        }
    }
    func applyLight() async {
        guard canApplyLight else { return }
        await work {
            guard !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                  !surfaceMaterialDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying, lightPreviewAccessID == viewportAccessID,
                  let preview = lightPreview, !editingLocked(acknowledging: preview.displayDraft),
                  let fields = light, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, selectedLightId: selectedLightId, command: fields.command())
            try await persist(value); syncFields()
            notice = "조명 변경을 이 Mac에 보관했습니다. 되돌리기로 취소할 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    func discardSurfaceMaterial() { guard !architectureApplying, !busy, !pending else { return }; syncSurfaceMaterial() }
    func previewSurfaceMaterial() async {
        guard canPreviewSurfaceMaterial, let fields = surfaceMaterial, let draft else { return }
        do {
            let action: NativeGeometryAction
            if surfaceMaterialTextureDirty { action = .surfaceMaterialTexture(try currentSurfaceMaterialTextureCommand()) }
            else { action = .surfaceMaterial(try fields.command()) }
            _ = try action.encodedValue(in: draft, selected: viewportSelection)
            architectureApplying = true
            defer { architectureApplying = false }
            await viewportController.perform(action, model: self)
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("SURFACE_MATERIAL_INVALID") }
    }
    func acceptSurfaceMaterialPreview(_ candidate: DocumentValue, base: NativeExhibitionDraft, command: SurfaceMaterialCommand) async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureGeometryDirty, !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, architectureApplying,
                  viewportController.editingContextActive, surfaceMaterialRuntimeSupported, !surfaceMaterialTextureDirty,
                  draft == base, try surfaceMaterial?.command() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            surfaceMaterialPreview = try SurfaceMaterialPreview(candidate: candidate, base: base, command: command, selected: viewportSelection)
            surfaceMaterialPreviewAccessID = viewportAccessID
            surfaceMaterialStatus = "재질을 미리 보고 있습니다. 적용하면 이 Mac에 보관되며 취소하면 원래 표시로 돌아갑니다."
        }
    }
    func acceptSurfaceMaterialTexturePreview(_ candidate: DocumentValue, base: NativeExhibitionDraft, command: SurfaceMaterialTextureCommand) async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureGeometryDirty, !lightDirty, !benchDirty, !plaqueDirty,
                  !furnishingDirty, !spaceLightingDirty, architectureApplying, viewportController.editingContextActive,
                  surfaceMaterialTexturesSupported, draft == base,
                  try currentSurfaceMaterialTextureCommand() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            surfaceMaterialTexturePreview = try SurfaceMaterialTexturePreview(candidate: candidate, base: base, command: command, selected: viewportSelection)
            surfaceMaterialPreviewAccessID = viewportAccessID
            surfaceMaterialStatus = "재질을 미리 보고 있습니다. 적용하면 이 Mac에 보관되며 취소하면 원래 표시로 돌아갑니다."
        }
    }
    func applySurfaceMaterial() async {
        guard canApplySurfaceMaterial else { return }
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureGeometryDirty, !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying,
                  viewportController.editingContextActive, surfaceMaterialRuntimeSupported, surfaceMaterialPreviewAccessID == viewportAccessID,
                  let fields = surfaceMaterial, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            if let preview = surfaceMaterialTexturePreview {
                guard surfaceMaterialTexturesSupported else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                value.draft = try preview.apply(to: draft, selected: viewportSelection, command: currentSurfaceMaterialTextureCommand())
            } else if let preview = surfaceMaterialPreview, !surfaceMaterialTextureDirty {
                value.draft = try preview.apply(to: draft, selected: viewportSelection, command: fields.command())
            } else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            try await persist(value); syncFields()
            notice = "재질 변경을 이 Mac에 보관했습니다. 실행 취소로 되돌릴 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    func syncArchitecture() {
        syncSurfaceMaterial()
        syncWallFinish()
        guard let draft, let selection = viewportSelection, ["room", "surface", "opening"].contains(selection.kind) else {
            architecture = nil; architectureBaseline = nil; architectureStatus = ""; return
        }
        do {
            let fields = try ArchitectureFormBuffer(draft: draft, selection: selection)
            architecture = fields; architectureBaseline = fields; architectureStatus = ""
        } catch {
            architecture = nil; architectureBaseline = nil
            architectureStatus = "선택한 공간 정보를 읽지 못했습니다. 저장된 설정은 유지됩니다."
        }
    }
    func discardArchitecture() { guard !architectureApplying else { return }; syncArchitecture() }
    func applyArchitecture() async {
        guard canApplyArchitecture, let fields = architecture, let draft else { return }
        do {
            let command = try fields.command()
            _ = try command.encodedValue(in: draft, selected: viewportSelection)
            architectureApplying = true
            defer { architectureApplying = false }
            await viewportController.perform(.architecture(command), model: self)
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("ARCHITECTURE_FIELDS_INVALID") }
    }
    private func refreshInstallationPreview() {
        guard installationEditable, var preview = draft, !placementId.isEmpty else { installationPreview = nil; return }
        do {
            try preview.setInstallation(installation.value(), for: placementId)
            installationPreview = preview
            installationStatus = installationDirty ? "입력한 설정을 미리 보고 있습니다. 설정 적용을 눌러 보관하세요." : ""
        } catch {
            installationStatus = "치수·좌표·색상 입력을 확인하세요. 입력은 유지되며 마지막 유효 미리보기를 표시합니다."
        }
    }
    private func syncInstallation() {
        syncFurnishing()
        installationPreview = nil
        do {
            let settings = try draft.flatMap { value in placementId.isEmpty ? nil : try value.installationSettings(for: placementId) }
            installationEditable = !placementId.isEmpty && !selectedHasFurnishing
            installationBaseline = InstallationFormBuffer(settings: settings)
            installation = installationBaseline
            installationStatus = ""
        } catch {
            installationEditable = false
            installationStatus = "이 배치의 설치 설정을 읽지 못했습니다. 원래 설정은 유지됩니다."
        }
    }
    func discardInstallation() { syncInstallation() }
    func resetInstallation() { installation = InstallationFormBuffer(settings: nil) }
    func applyInstallation() async {
        await work {
            guard !immutable, !pending, !furnishingDirty, !architectureApplying, !selectedHasFurnishing, installationEditable, !placementId.isEmpty, var value = state, var next = draft else { throw AuthoringFailure("EXHIBITION_PENDING") }
            try next.setInstallation(installation.value(), for: placementId)
            value.draft = next; try await persist(value); syncInstallation()
            notice = "설정 적용을 이 Mac에 보관했습니다. 전시 저장은 별도로 진행하세요."
        }
    }
    func undo() async {
        await work {
            guard (!immutable || boundedPlacementReady), !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, let previous = past.last, let current = draft, var value = state else { return }
            var next = current
            if boundedPlacementCapability != nil {
                guard boundedPlacementReady, let selection = viewportSelection, let baseline = boundedPlacementBaseline else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                try next.acceptBoundedPlacement(previous.candidate, selection: selection, savedBaseline: baseline)
            } else { try next.restoreEditHistory(previous) }
            value.draft = next
            try await persist(value, recordUndo: false); past.removeLast(); future.append(current); syncFields()
            notice = "이전 편집으로 되돌렸습니다. 전시 저장본은 바꾸지 않았습니다."
        }
    }
    func redo() async {
        await work {
            guard (!immutable || boundedPlacementReady), !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, let previous = future.last, let current = draft, var value = state else { return }
            var next = current
            if boundedPlacementCapability != nil {
                guard boundedPlacementReady, let selection = viewportSelection, let baseline = boundedPlacementBaseline else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                try next.acceptBoundedPlacement(previous.candidate, selection: selection, savedBaseline: baseline)
            } else { try next.restoreEditHistory(previous) }
            value.draft = next
            try await persist(value, recordUndo: false); future.removeLast(); past.append(current); syncFields()
            notice = "편집을 다시 적용했습니다."
        }
    }
    func selectViewport(_ selection: ViewportSelection) {
        guard !entrancePreviewActive, !placingArtwork, !busy, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, let draft, selection.belongs(to: draft) else { return }
        selectedLightId = ""; syncLight(); clearBenchSelection(); clearPlaqueSelection()
        if let room = roomForSelection(selection, draft: draft), room != roomId { roomId = room; selectRoom() }
        if selection.kind == "room" { roomId = selection.id; selectRoom() }
        if selection.kind == "placement" {
            placementId = selection.id
            if let room = draft.placements.first(where: { $0["id"].string == selection.id })?["roomId"].string { roomId = room; selectRoom() }
            selectPlacement()
        }
        viewportSelection = selection
        if selection.kind == "surface", draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == selection.id && $0["type"].string == "floor" }) == true {
            floorTargetKey = FloorSettings.Target(kind: .surface, id: selection.id).key; syncFloor()
        }
        if selection.kind != "placement" { catalogArtworkId = "" }
        syncArchitecture()
    }
    private func list(more: Bool = false) async throws {
        guard let api else { return }
        let page = try await api.exhibitions(query: query, cursor: more ? cursor : nil)
        if more { items += page.items.filter { row in !items.contains(where: { $0.id == row.id }) } } else { items = page.items }
        cursor = page.nextCursor
        let artworks = try await api.artworks()
        library = artworks.items.filter { $0.approvedRevision == $0.revision }
        libraryCursor = artworks.nextCursor
        refreshCatalogMetadata()
    }
    var catalogMetadataProgress: (processed: Int, total: Int)? {
        guard catalogTask != nil else { return nil }
        let processed = library.filter { row in
            catalogEntries[row.id]?.revision == row.revision || catalogFailures.contains(row.id)
        }.count
        return processed < library.count ? (processed, library.count) : nil
    }
    private func refreshCatalogMetadata() {
        catalogTask?.cancel()
        let current = Dictionary(library.map { ($0.id, $0.revision) }, uniquingKeysWith: { _, newer in newer })
        catalogEntries = catalogEntries.filter { current[$0.key] == $0.value.revision }
        catalogFailures = []
        // Reuse the exact approved snapshots already present in this draft.
        for artwork in draft?.artworks ?? [] {
            guard let id = (artwork["extensions"]["org.exhibitos.studio/cms"]["artworkId"].string ?? artwork["id"].string)?.lowercased(),
                  let revision = artwork["extensions"]["org.exhibitos.studio/cms"]["revision"].integer,
                  current[id] == Int(revision) else { continue }
            catalogEntries[id] = ArtworkCatalogEntry(id: id, revision: Int(revision), artwork: artwork)
        }
        let access = viewportAccessID, rows = library
        catalogTask = Task { [weak self] in
            guard let self, let api = self.api else { return }
            for row in rows {
                guard !Task.isCancelled, self.viewportAccessID == access else { return }
                if self.catalogEntries[row.id] != nil { continue }
                do {
                    let approved = try await api.studioArtwork(row.id)
                    guard !Task.isCancelled, self.viewportAccessID == access else { return }
                    guard approved.artwork["extensions"]["org.exhibitos.studio/cms"]["revision"].integer == Int64(row.revision) else { throw AuthoringFailure("ARTWORK_SCOPE_CHANGED") }
                    self.catalogEntries[row.id] = ArtworkCatalogEntry(id: row.id, revision: row.revision, artwork: approved.artwork)
                } catch {
                    guard !Task.isCancelled, self.viewportAccessID == access else { return }
                    self.catalogFailures.insert(row.id)
                }
            }
        }
    }
    func retryCatalogMetadata() { refreshCatalogMetadata() }
    func loadMoreCatalog() async {
        await work {
            guard let api, let libraryCursor else { return }
            let page = try await api.artworks(cursor: libraryCursor)
            library += page.items.filter { row in row.approvedRevision == row.revision && !library.contains(where: { $0.id == row.id }) }
            self.libraryCursor = page.nextCursor
            refreshCatalogMetadata()
        }
    }
    func revealApprovedCatalogArtwork(_ id: String, revision: Int) async {
        guard canSelectCatalog, !immutable, let api, let base = draft else { return }
        let access = viewportAccessID
        var revealed: ArtworkSummary?
        await work {
            let approved = try await api.studioArtwork(id).artwork
            let cms = approved["extensions"]["org.exhibitos.studio/cms"]
            guard cms["artworkId"].string == id, cms["revision"].integer == Int64(revision) else {
                throw AuthoringFailure("ARTWORK_SCOPE_CHANGED")
            }
            let first = try await api.artworks()
            var page = first
            var seenCursors: Set<String> = []
            while true {
                try Task.checkCancellation()
                guard self.api === api, viewportAccessID == access, draft == base, !immutable else {
                    throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED")
                }
                if let row = page.items.first(where: { $0.id == id }) {
                    guard !row.archived, row.revision == revision, row.approvedRevision == revision else {
                        throw AuthoringFailure("ARTWORK_SCOPE_CHANGED")
                    }
                    library = first.items.filter { !$0.archived && $0.approvedRevision == $0.revision && $0.id != id }
                    library.insert(row, at: 0)
                    libraryCursor = first.nextCursor
                    catalogEntries[id] = ArtworkCatalogEntry(id: id, revision: revision, artwork: approved)
                    refreshCatalogMetadata()
                    revealed = row
                    return
                }
                guard let cursor = page.nextCursor, seenCursors.insert(cursor).inserted else {
                    throw AuthoringFailure("ARTWORK_SCOPE_CHANGED")
                }
                page = try await api.artworks(cursor: cursor)
            }
        }
        guard let revealed, self.api === api, viewportAccessID == access, draft == base, canSelectCatalog, !immutable else { return }
        catalogQuery = ""
        catalogFilter = .all
        selectCatalog(revealed.id)
        catalogRevealID = revealed.id
    }
    func selectCatalog(_ id: String) {
        guard canSelectCatalog, library.contains(where: { $0.id == id }), let draft else { return }
        catalogArtworkId = id
        if let placement = draft.catalogPlacement(for: id, preferring: placementId) {
            selectViewport(ViewportSelection(kind: "placement", id: placement))
            catalogStatus = "공간에 배치한 작품을 선택했습니다. 시점은 그대로 유지됩니다."
        } else {
            placementId = ""; viewportSelection = nil; syncInstallation(); syncArchitecture()
            catalogStatus = "아직 배치하지 않은 원본 작품입니다. 배치할 위치를 선택할 수 있습니다."
        }
    }
    func prepareCatalogPlacement() {
        guard canSelectCatalog, !immutable, !catalogArtworkId.isEmpty else { return }
        placementId = ""; viewportSelection = nil; syncInstallation(); syncArchitecture()
        artworkId = catalogArtworkId
        catalogStatus = "배치할 작품을 골랐습니다. 위치를 확인한 뒤 배치하세요. 원본 작품은 바뀌지 않습니다."
    }
    func beginVisualArtworkPlacement() async {
        guard let entry = catalogEntries[catalogArtworkId] else { return }
        _ = await beginVisualArtworkPlacement(entry, requiresSelection: true)
    }
    func beginVisualArtworkPlacement(_ entry: ArtworkCatalogEntry, requiresSelection: Bool) async -> ArtworkPlacementIntent? {
        guard canPlaceVisually(entry), catalogEntries[entry.id]?.revision == entry.revision,
              let api, let credentials = viewportCredentials, let base = draft else { return nil }
        let access = viewportAccessID, room = roomId, attempt = UUID()
        placementAttempt = attempt; placementApplying = true; failure = nil
        catalogStatus = "승인한 작품을 확인하고 있습니다."
        defer { if placementAttempt == attempt { placementAttempt = nil }; placementApplying = false }
        do {
            let approved = try await api.studioArtwork(entry.id)
            guard placementAttempt == attempt, viewportAccessID == access, draft == base, roomId == room, (!requiresSelection || catalogArtworkId == entry.id),
                  catalogEntries[entry.id]?.revision == entry.revision,
                  !immutable, !pending, !formDirty, !positionDirty, !architectureDirty else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            let intent = try ArtworkPlacementIntent(artworkId: entry.id, expectedRevision: entry.revision, tenantId: credentials.tenantId, artwork: approved.artwork, roomId: room, base: base)
            artworkPlacement = intent; artworkPlacementTarget = nil
            _ = try await viewportController.placement(intent, phase: .begin)
            guard artworkPlacement?.requestId == intent.requestId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            catalogStatus = intent.targetKind == .wall ? "3D 공간의 벽을 클릭해 미리 본 뒤 ‘여기에 배치’를 선택하세요." : "3D 공간의 바닥을 클릭해 미리 본 뒤 ‘여기에 배치’를 선택하세요."
            return intent
        } catch {
            artworkPlacement = nil; artworkPlacementTarget = nil
            catalogStatus = "3D 배치를 시작하지 못했습니다. 기존 선택과 입력은 유지됩니다. 숫자로도 배치할 수 있습니다."
            viewportController.diagnose("PLACE", error)
            return nil
        }
    }
    func receivePlacementTarget(_ value: DocumentValue) {
        guard let intent = artworkPlacement, draft == intent.base,
              let target = try? ViewportPlacementTarget(value: value, intent: intent) else { return }
        artworkPlacementTarget = target
        catalogStatus = target.valid ? "이 위치를 미리 보고 있습니다. ‘여기에 배치’를 선택하면 전시에 추가합니다." : "여기에 배치할 수 없습니다. 선택한 공간의 다른 벽이나 바닥을 클릭하세요."
    }
    func placementCancelled(_ requestId: String) {
        guard artworkPlacement?.requestId == requestId else { return }
        placementAttempt = nil
        artworkPlacement = nil; artworkPlacementTarget = nil
        catalogStatus = "배치 미리보기를 취소했습니다. 기존 선택과 입력은 유지됩니다."
    }
    func cancelVisualArtworkPlacement() async {
        guard placingArtwork else { return }
        placementAttempt = nil
        guard let intent = artworkPlacement else { catalogStatus = "배치 준비를 취소했습니다. 기존 선택과 입력은 유지됩니다."; return }
        placementCancelled(intent.requestId)
        _ = try? await viewportController.placement(intent, phase: .cancel)
    }
    func commitVisualArtworkPlacement() async {
        guard !placementApplying, !busy, !pending, !immutable, !formDirty, !positionDirty, !architectureDirty,
              let intent = artworkPlacement, let target = artworkPlacementTarget, target.valid,
              let api, draft == intent.base else { return }
        let access = viewportAccessID, attempt = UUID()
        placementAttempt = attempt; placementApplying = true; failure = nil
        defer { if placementAttempt == attempt { placementAttempt = nil }; placementApplying = false }
        do {
            let approved = try await api.studioArtwork(intent.artworkId)
            guard placementAttempt == attempt, approved.artwork == intent.artwork, viewportAccessID == access, draft == intent.base,
                  artworkPlacement?.requestId == intent.requestId, artworkPlacementTarget == target,
                  roomId == intent.roomId, !immutable, !pending, !formDirty, !positionDirty, !architectureDirty,
                  let placement = try await viewportController.placement(intent, phase: .commit, target: target),
                  placementAttempt == attempt, artworkPlacement?.requestId == intent.requestId, artworkPlacementTarget == target,
                  viewportAccessID == access, let current = draft, current == intent.base,
                  let surface = target.surfaceId, let offset = target.offset, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try intent.applying(placement, surfaceId: surface, offset: offset, to: current)
            try await persist(value)
            artworkPlacement = nil; artworkPlacementTarget = nil
            placementId = intent.placementId; viewportSelection = ViewportSelection(kind: "placement", id: intent.placementId)
            syncFields()
            catalogStatus = "작품을 이 Mac에 배치했습니다. 되돌리기로 취소하거나 전시 저장을 진행하세요."
        } catch {
            await cancelVisualArtworkPlacement()
            catalogStatus = "이 위치에 배치하지 못했습니다. 현재 전시와 입력은 유지됩니다. 위치를 다시 선택하세요."
            viewportController.diagnose("PLACE", error)
        }
    }
    func cancelPositionInput() {
        guard !busy, !pending else { return }
        if let values = draft?.placements.first(where: { $0["id"].string == placementId })?["transform"]["position"].items, values.count == 3 {
            x = values[0].double ?? 0; y = values[1].double ?? 0; z = values[2].double ?? 0
        } else { x = 0; y = 1; z = 0 }
        if failure?.code == "PLACEMENT_INVALID" { failure = nil }
    }
    func cancelCatalogPlacement() {
        guard !busy, !pending else { return }
        artworkId = ""
        catalogStatus = "새 배치 선택을 취소했습니다. 원본 작품과 기존 배치는 그대로입니다."
    }
    func refresh(more: Bool = false) async { await work { try await list(more: more) } }
    func new() async {
        await work(allowViewingPlanInput: true, allowViewingPointInput: true) {
            openingStage = .creating
            defer { openingStage = nil }
            guard !ordinaryBoundedRuntime, !pending, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            value.draft = try NativeExhibitionDraft.new(roomBaseLight: session?.advertisesRoomBaseLight == true)
            value.etag = nil; value.savedEditVersion = nil
            try await persist(value); storedImmutable = false; reusablePublicationId = nil; comparison = nil; syncFields()
            notice = "새 전시를 이 Mac에 보관했습니다. 공간과 작품을 구성한 뒤 전시를 저장하세요."
        }
    }
    /// Fail closed for selected native saved drafts until the server derives
    /// the policy for this exact session, exhibition and saved ETag.
    private func checkBoundedPolicy(_ id: String, etag: String, revision: Int? = nil, api: AuthoringAPI, access: UUID) async throws -> Bool {
        guard selectedBoundedPolicy else { return false }
        guard let originalSession = session, let workspace = state?.workspaceId else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
        let policyGeneration = boundedPolicyGeneration
        let policy = try await api.nativeViewportPolicy(id)
        guard selectedBoundedPolicy, viewportAccessID == access, boundedPolicyGeneration == policyGeneration, self.api === api, state?.workspaceId == workspace,
              let currentSession = session, currentSession.tenantId == originalSession.tenantId,
              currentSession.userId == originalSession.userId, currentSession.role == originalSession.role,
              currentSession.csrfToken == originalSession.csrfToken, currentSession.expiresAt == originalSession.expiresAt,
              policy.exhibitionId.lowercased() == id.lowercased(), policy.etag == etag,
              revision == nil || policy.revision == revision else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
        return policy.boundedReadOnly
    }
    /// Read the exact saved revision and import record before considering the
    /// narrow ordinary-session placement path. All failures retain the lock.
    func prepareOrdinaryBoundedPlacement() async {
        guard ordinaryBoundedRuntime, !selectedBoundedPolicy, let api, let initial = state,
              let draft = initial.draft, let etag = initial.etag, let originalSession = session,
              let credentials = viewportCredentials, let imported = oexImport.state else { return }
        let access = viewportAccessID, generation = boundedPolicyGeneration
        do {
            let saved = try await api.exhibition(draft.id)
            let policy = try await api.nativeViewportPolicy(draft.id)
            let publication = try await publishedEditingAccess(draft.id, api: api, expected: saved)
            guard ordinaryBoundedRuntime, viewportAccessID == access, boundedPolicyGeneration == generation,
                  self.api === api, let current = state, current.revision == initial.revision,
                  current.draft == initial.draft, current.etag == etag,
                  let live = session, live.tenantId == originalSession.tenantId, live.userId == originalSession.userId,
                  live.role == originalSession.role, live.csrfToken == originalSession.csrfToken, live.expiresAt == originalSession.expiresAt,
                  viewportCredentials?.token == credentials.token, viewportCredentials?.origin == credentials.origin,
                  let currentImport = oexImport.state, currentImport.revision == imported.revision,
                  policy.exhibitionId.lowercased() == draft.id.lowercased(), policy.etag == etag,
                  policy.revision == saved.revision, saved.etag == etag,
                  let profile = policy.boundedProfile, let required = policy.boundedEditRequired else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            if !required {
                guard !policy.boundedReadOnly,
                      profile ? boundedRuntimeQualified : policy.confirmsOrdinaryDraft,
                      saved.draft == draft,
                      currentImport.intent == nil, currentImport.jobId == nil, currentImport.fileName == nil,
                      currentImport.workspaceId == current.workspaceId, currentImport.tenantId == current.tenantId,
                      currentImport.principalId == current.principalId else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                boundedProfileKnown = profile
                boundedReadOnly = false; boundedPolicyPending = false
                // An unqualified ordinary session may copy a verified published
                // source, but must keep the existing publication row locked.
                storedImmutable = publication.immutable || (!boundedRuntimeQualified && publication.publicationId != nil)
                reusablePublicationId = publication.publicationId
                return
            }
            guard boundedRuntimeQualified, !publication.immutable, saved.draft == draft,
                  let capability = NativeBoundedEditAdmission.admit(policy: policy, document: current, importState: currentImport,
                                                                    session: live, selectedImport: false) else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            boundedPlacementPolicy = policy; boundedPlacementBaseline = saved.draft; boundedPlacementSession = live
            boundedPlacementAccess = access; boundedPlacementToken = credentials.token; boundedPlacementEpoch = nil
            boundedPlacementCapability = capability
            boundedReadOnly = false; boundedPolicyPending = false; storedImmutable = true
            notice = "이 전시는 선택한 작품의 위치·회전·크기를 조정할 수 있습니다."
        } catch {
            guard viewportAccessID == access, boundedPolicyGeneration == generation else { return }
            boundedPlacementCapability = nil; boundedPlacementPolicy = nil; boundedPlacementBaseline = nil
            boundedPlacementSession = nil; boundedPlacementEpoch = nil; boundedProfileKnown = nil
            storedImmutable = true; boundedPolicyPending = true
            notice = "전시의 편집 가능 상태를 확인하지 못했습니다. 저장된 입력은 유지됩니다."
        }
    }

    func acceptBoundedPlacementFrame(_ frame: ViewportFrame, _ response: ViewportAcceptance) -> Bool {
        guard ordinaryBoundedRuntime, let capability = boundedPlacementCapability, let policy = boundedPlacementPolicy,
              let document = state, let imported = oexImport.state, let session, let captured = boundedPlacementSession,
              let access = boundedPlacementAccess, viewportAccessID == access,
              viewportCredentials?.token == boundedPlacementToken, boundedPlacementEpoch == nil || boundedPlacementEpoch == frame.epoch,
              NativeBoundedEditAdmission.acknowledges(response, frame: frame, expected: capability, policy: policy,
                  ownerEpoch: viewportController.hostEpoch, currentSequence: frame.sequence, document: document,
                  importState: imported, session: session, capturedSession: captured,
                  firstAdmission: boundedPlacementEpoch == nil, selectedImport: selectedBoundedPolicy) else { return false }
        boundedPlacementEpoch = frame.epoch
        return true
    }
    private func resolveImportedPolicy(_ saved: NativeDocumentState, access: UUID) async {
        guard selectedBoundedPolicy, let id = saved.draft?.id, let etag = saved.etag, let api else { return }
        let policyGeneration = boundedPolicyGeneration
        do {
            let restricted = try await checkBoundedPolicy(id, etag: etag, api: api, access: access)
            let publication = try await publishedEditingAccess(id, api: api)
            guard viewportAccessID == access, boundedPolicyGeneration == policyGeneration, self.api === api, state?.draft == saved.draft, state?.etag == etag, state?.pending == nil else { return }
            boundedReadOnly = restricted; boundedPolicyPending = false
            storedImmutable = publication.immutable || restricted; reusablePublicationId = publication.publicationId
        } catch {
            guard viewportAccessID == access, boundedPolicyGeneration == policyGeneration, state?.draft == saved.draft, state?.etag == etag else { return }
            storedImmutable = true; boundedPolicyPending = true
            notice = "전시의 편집 가능 상태를 확인하지 못했습니다. 작업 공간을 다시 연결하세요."
        }
    }
    /// Publication identity is resolved only by the authenticated server; an
    /// older runtime keeps the existing read-only behavior.
    private func publishedEditingAccess(_ id: String, api: AuthoringAPI, expected: NativeStudioResponse? = nil) async throws -> (immutable: Bool, publicationId: String?) {
        let records = try await api.publications(exhibitionId: id)
        guard let selected = records.first else { return (false, nil) }
        do {
            let response = try await api.reusableEditingDraft(publicationId: selected.publicationId)
            guard response.authoringExhibitionId.lowercased() == id.lowercased() else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            if let expected, (response.revision != expected.revision || response.etag != expected.etag || response.draft != expected.draft) { throw AuthoringFailure("EXHIBITION_COMPARISON_STALE", status: 409) }
            return (false, selected.publicationId)
        } catch let error as AuthoringFailure where error.status == 404 {
            return (true, nil)
        }
    }
    func open(_ id: String) async {
        await work(allowViewingPlanInput: true, allowViewingPointInput: true) {
            openingStage = .loadingSaved
            defer { openingStage = nil }
            guard !pending, let api, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            if ordinaryBoundedRuntime {
                boundedPolicyGeneration = UUID()
                boundedPlacementCapability = nil; boundedPlacementPolicy = nil; boundedPlacementBaseline = nil
                boundedPlacementSession = nil; boundedPlacementEpoch = nil; boundedProfileKnown = nil
                storedImmutable = true; boundedPolicyPending = true
            }
            let accessID = viewportAccessID, policyGeneration = boundedPolicyGeneration
            let saved = try await api.exhibition(id)
            guard viewportAccessID == accessID, boundedPolicyGeneration == policyGeneration, self.api === api, state?.revision == value.revision else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            let editingAccess = try await publishedEditingAccess(id, api: api, expected: saved)
            let restricted = try await checkBoundedPolicy(id, etag: saved.etag, revision: saved.revision, api: api, access: accessID)
            guard viewportAccessID == accessID, boundedPolicyGeneration == policyGeneration, self.api === api, state?.revision == value.revision else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            value.draft = saved.draft; value.etag = saved.etag; value.savedEditVersion = saved.draft.document["editVersion"].integer
            if selectedBoundedPolicy { storedImmutable = true; boundedPolicyPending = true }
            try await persist(value)
            guard viewportAccessID == accessID, boundedPolicyGeneration == policyGeneration, self.api === api, state?.draft == saved.draft, state?.etag == saved.etag else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            boundedReadOnly = restricted; boundedPolicyPending = ordinaryBoundedRuntime; storedImmutable = editingAccess.immutable || restricted || ordinaryBoundedRuntime; reusablePublicationId = editingAccess.publicationId; comparison = nil; syncFields()
            if ordinaryBoundedRuntime { await prepareOrdinaryBoundedPlacement() }
            if !boundedPolicyPending {
                notice = storedImmutable ? "이 공개용 저장본은 현재 런타임에서 읽기 전용입니다." : editingAccess.publicationId != nil ? "공개된 전시의 편집 초안을 열었습니다. 저장해도 공개본은 바뀌지 않습니다." : "저장한 전시 입력을 불러왔습니다."
            }
            // OEX import can add approved works after the last library fetch.
            // Refresh only after the saved draft is safely opened; a catalogue
            // failure must not undo or misreport that successful open.
            let access = viewportAccessID
            openingStage = .refreshingArtwork
            let refreshFailure = "작품 목록을 새로 확인하지 못했습니다. 전시 목록의 검색으로 다시 확인하세요."
            do {
                let artworks = try await api.artworks()
                try Task.checkCancellation()
                guard viewportAccessID == access, draft?.id == id, self.api === api else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                library = artworks.items.filter { $0.approvedRevision == $0.revision }
                libraryCursor = artworks.nextCursor
                if catalogStatus == refreshFailure { catalogStatus = "" }
                refreshCatalogMetadata()
            } catch {
                guard !Task.isCancelled, viewportAccessID == access, draft?.id == id, self.api === api else { return }
                catalogStatus = refreshFailure
            }
        }
    }
    private func appliedFields(allowPosition: Bool = false) throws -> NativeExhibitionDraft {
        guard !floorDirty else { throw AuthoringFailure("FLOOR_APPLY_REQUIRED") }
        guard !architectureDirty, !architectureApplying else { throw AuthoringFailure("ARCHITECTURE_APPLY_REQUIRED") }
        guard !positionDirty || allowPosition else { throw AuthoringFailure("PLACEMENT_APPLY_REQUIRED") }
        guard var draft else { throw AuthoringFailure("EXHIBITION_REQUIRED") }
        if publicationWatermark.isDirty {
            guard publicationWatermarkRuntimeAvailable, publicationWatermarkReadable else { throw AuthoringFailure("WATERMARK_RUNTIME_UNAVAILABLE") }
            try draft.setPublicationWatermark(publicationWatermark.settings())
        }
        if title != draft.title { try draft.rename(title) }
        if entranceEditable {
            var settings = entrance
            settings.design = try coverEntry.value()
            if try draft.viewerEntranceSettings() != settings { try draft.setViewerEntrance(settings) }
        }
        if installationDirty, installationEditable { try draft.setInstallation(installation.value(), for: placementId) }
        if let room = draft.rooms.first(where: { $0["id"].string == roomId }), roomName != room["name"].string || width != room["dimensions"]["width"].double || height != room["dimensions"]["height"].double || depth != room["dimensions"]["depth"].double {
            try draft.updateRoom(roomId, name: roomName, width: width, height: height, depth: depth)
        }
        return draft
    }
    func keepLocal() async {
        await work {
            guard !immutable, !pending, var value = state else { throw AuthoringFailure(immutable ? "PUBLISHED_DRAFT_IMMUTABLE" : "EXHIBITION_PENDING") }
            value.draft = try appliedFields(); try await persist(value); syncEntrance(); syncInstallation(); syncPublicationWatermark()
            notice = "현재 전시 입력을 이 Mac에 보관했습니다. 아직 전시 저장본에 반영하지 않았습니다."
        }
    }
    var canRefreshApprovedCompactArtwork: Bool {
        guard session?.compactModelVariantProfile == NativeCompactModelVariantRequest.profile,
              !busy, !dirty, !immutable, !pending, !placingArtwork, !entrancePreviewActive,
              let draft, state?.etag != nil,
              let placement = draft.placements.first(where: { $0["id"].string == placementId }),
              let placed = draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] })
        else { return false }
        return placed["artworkType"] == .string("sculpture") && placed["extensions"]["org.exhibitos.viewer/lod"] == .null
    }

    func refreshApprovedCompactArtwork() async {
        guard canRefreshApprovedCompactArtwork else { return }
        await work {
            guard let api, var local = state, var draft = local.draft, let etag = local.etag,
                  await api.admitsCompactModelVariant else { throw AuthoringFailure("IMPORT_MODEL_UNAVAILABLE") }
            let target = placementId, access = viewportAccessID
            guard let row = draft.placements.first(where: { $0["id"].string == target }),
                  let artworkId = draft.artworks.first(where: { $0["revisionId"] == row["artworkRevisionId"] })?["id"].string
            else { throw AuthoringFailure("PLACEMENT_INVALID") }
            let saved = try await api.exhibition(draft.id)
            guard saved.draft == draft, saved.etag == etag else { throw AuthoringFailure("EXHIBITION_COMPARISON_STALE") }
            let approved = try await api.studioArtwork(artworkId)
            guard self.api === api, viewportAccessID == access, placementId == target,
                  state?.revision == local.revision, !dirty else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            try draft.refreshApprovedCompactArtwork(approved.artwork, placementId: target)
            local.draft = draft
            try await persist(local)
            syncFields()
            notice = "승인한 축소본을 기존 배치에 반영했습니다. 전시를 저장해 확정하세요."
        }
    }

    var hasPendingModelCandidateReplacement: Bool { state?.pending?.modelCandidate != nil }
    var hasPendingProductImageReplacement: Bool { state?.pending?.productImage != nil }

    func productImageTarget(placementId target: String) -> (id: String, revision: Int)? {
        guard !selectedBoundedPolicy, !entrancePreviewActive, !placingArtwork,
              let state, let draft = state.draft, state.etag != nil, state.pending == nil,
              let placement = draft.placements.first(where: { $0["id"].string == target }),
              let artwork = draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] }),
              artwork["artworkType"] == .string("image"),
              let id = artwork["id"].string, let revision = artwork["revision"].integer,
              (1..<2_147_483_647).contains(revision),
              artwork["extensions"]["org.exhibitos.studio/cms"]["tenantId"].string == state.tenantId,
              artwork["extensions"]["org.exhibitos.studio/cms"]["artworkId"].string == id,
              artwork["extensions"]["org.exhibitos.studio/cms"]["revision"].integer == revision else { return nil }
        return (id, Int(revision))
    }

    func prepareProductImage(_ url: URL, placementId target: String, authoring: AuthoringModel) async {
        await work {
            guard viewportController.editingContextActive, placementId == target, !dirty, !immutable,
                  let api, let local = state, let draft = local.draft, let etag = local.etag,
                  let artwork = productImageTarget(placementId: target),
                  let transfer = authoring.transfer, transfer.workspaceId == local.workspaceId,
                  transfer.tenantId == local.tenantId, transfer.principalId == local.principalId else {
                throw AuthoringFailure("ARTWORK_REPLACEMENT_UNAVAILABLE")
            }
            let access = viewportAccessID
            let saved = try await api.exhibition(draft.id)
            let editing = try await publishedEditingAccess(draft.id, api: api, expected: saved)
            guard saved.draft == draft, saved.etag == etag, !editing.immutable else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            try await authoring.prepareProductImage(url, artworkId: artwork.id, artworkRevision: artwork.revision,
                expectedAPI: api, isCurrent: { [weak self] in
                    guard let self else { return false }
                    return self.viewportController.editingContextActive && self.api === api
                        && self.viewportAccessID == access && self.state?.revision == local.revision
                        && self.placementId == target && !self.dirty
                })
            notice = "JPEG 원본을 제출했습니다. 준비 상태를 확인한 뒤 전시에 적용하세요."
        }
    }

    func replaceWithProductImage(authoring: AuthoringModel, reviewedReceiptSHA256: String? = nil) async {
        await work {
            guard !selectedBoundedPolicy, viewportController.editingContextActive,
                  let api, await api.admitsProductImagePreparation, var local = state,
                  let draft = local.draft, let etag = local.etag, let transfer = authoring.transfer,
                  authoring.exhibitionAPI === api, transfer.workspaceId == local.workspaceId,
                  transfer.tenantId == local.tenantId, transfer.principalId == local.principalId,
                  let importIntent = transfer.intent, let preparation = importIntent.productImagePreparation,
                  let jobId = transfer.jobId else { throw AuthoringFailure("ARTWORK_REPLACEMENT_UNAVAILABLE") }
            let access = viewportAccessID
            let intent: NativeDocumentState.Pending
            let candidate: NativeProductImageReplacement
            if let pending = local.pending {
                guard let reviewed = pending.productImage, pending.origin == NativeProductImagePreparationIntent.profileID,
                      pending.draft == draft, pending.etag == etag, reviewed.artworkId == importIntent.artworkId,
                      reviewed.artworkRevision == preparation.artworkRevision, reviewed.importJobId == jobId else {
                    throw AuthoringFailure("EXHIBITION_PENDING")
                }
                intent = pending
                candidate = reviewed
            } else {
                guard !dirty, !immutable, let artwork = productImageTarget(placementId: placementId),
                      artwork.id == importIntent.artworkId, artwork.revision == preparation.artworkRevision,
                      let reviewedReceiptSHA256 else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
                let target = placementId
                let saved = try await api.exhibition(draft.id)
                let editing = try await publishedEditingAccess(draft.id, api: api, expected: saved)
                let job = try await api.importJob(jobId)
                guard saved.draft == draft, saved.etag == etag, !editing.immutable,
                      job.id == jobId, let status = job.productImagePreparation,
                      status.validatesReady(intent: importIntent, preparation: preparation, job: job,
                          workspaceID: local.workspaceId, principalID: local.principalId),
                      status.serverReceipt?.sha256 == reviewedReceiptSHA256,
                      self.api === api, viewportAccessID == access, state?.revision == local.revision,
                      authoring.transfer?.revision == transfer.revision, placementId == target, !dirty else {
                    throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
                }
                candidate = .init(artworkId: artwork.id, artworkRevision: artwork.revision, importJobId: jobId,
                                  receiptSha256: reviewedReceiptSHA256)
                intent = .init(etag: etag, draft: draft, origin: NativeProductImagePreparationIntent.profileID,
                               productImage: candidate)
                local.pending = intent
                try await persist(local, recordUndo: false)
            }
            let prefix = "\"studio-r"
            guard etag.hasPrefix(prefix), let revision = etag.dropFirst(prefix.count).split(separator: "-").first.flatMap({ Int($0) }),
                  self.api === api, viewportAccessID == access, state?.pending?.requestId == intent.requestId,
                  authoring.transfer?.revision == transfer.revision else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            let saved: NativeStudioResponse
            do {
                saved = try await api.replaceProductImage(exhibitionId: draft.id, studioRevision: revision, etag: etag,
                    requestId: intent.requestId, importJobId: candidate.importJobId, intent: importIntent,
                    workspaceId: local.workspaceId, principalId: local.principalId, expectedReceiptSHA256: candidate.receiptSha256)
            } catch let error as AuthoringFailure where (error.status == 412 && error.code == "REMOTE_CONFLICT")
                || (error.status == 409 && ["ARTWORK_REPLACEMENT_STALE", "ARTWORK_REPLACEMENT_REFERENCE_UNSUPPORTED"].contains(error.code))
                || ([409, 422].contains(error.status) && error.code == "ARTWORK_REPLACEMENT_INVALID") {
                // The server checks its replay receipt before these rejection
                // branches. Keep the original draft available for comparison.
                let live = try await api.exhibition(draft.id)
                guard self.api === api, viewportAccessID == access, var rejected = state,
                      rejected.pending?.requestId == intent.requestId else { throw error }
                rejected.pending = nil
                try await persist(rejected, recordUndo: false)
                comparison = live
                throw error
            }
            guard self.api === api, viewportAccessID == access, var accepted = state,
                  accepted.pending?.requestId == intent.requestId else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            accepted.draft = saved.draft; accepted.etag = saved.etag
            accepted.savedEditVersion = saved.draft.document["editVersion"].integer; accepted.pending = nil
            try await persist(accepted, recordUndo: false)
            past = []; future = []; comparison = saved
            syncFields()
            if ordinaryBoundedRuntime { await prepareOrdinaryBoundedPlacement() }
            notice = "교체한 이미지가 전시 저장본에 반영됐습니다."
            try await authoring.finishProductImageTransfer(intent: importIntent, jobId: jobId, api: api)
        }
    }

    func finishSavedProductImageTransfer(authoring: AuthoringModel) async {
        await work {
            guard !selectedBoundedPolicy, viewportController.editingContextActive, !dirty, !pending,
                  let api, await api.admitsProductImagePreparation, let local = state, let draft = local.draft,
                  let transfer = authoring.transfer, transfer.workspaceId == local.workspaceId,
                  transfer.tenantId == local.tenantId, transfer.principalId == local.principalId,
                  let intent = transfer.intent, let preparation = intent.productImagePreparation,
                  let jobId = transfer.jobId else { throw AuthoringFailure("ARTWORK_REPLACEMENT_UNAVAILABLE") }
            let saved = try await api.exhibition(draft.id)
            let job = try await api.importJob(jobId)
            guard saved.draft == draft, saved.etag == local.etag, job.id == jobId,
                  let full = job.productImagePreparation?.full,
                  draft.artworks.contains(where: { $0["id"].string == intent.artworkId
                    && $0["revision"].integer == Int64(preparation.artworkRevision + 1)
                    && $0["primaryAssetId"].string == full.assetId }),
                  self.api === api, state?.revision == local.revision, authoring.transfer?.revision == transfer.revision else {
                throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
            }
            try await authoring.finishProductImageTransfer(intent: intent, jobId: jobId, api: api)
            notice = "교체한 이미지와 전시 저장본을 확인했습니다."
        }
    }

    var canReplaceWithSelectedModelCandidate: Bool {
        guard session?.nativeModelCandidateReplacementProfile == NativeApprovedModelCandidateReplacement.profile,
              !busy, !pending, !dirty, !entrancePreviewActive, !placingArtwork,
              let draft, state?.etag != nil, let entry = catalogEntries[catalogArtworkId],
              entry.kind == "sculpture", entry.primaryMime == "model/gltf-binary",
              let placement = draft.placements.first(where: { $0["id"].string == placementId }),
              let placed = draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] })
        else { return false }
        return placed["artworkType"] == .string("sculpture") && placed["id"].string != entry.id
    }

    func replaceWithSelectedModelCandidate() async {
        let starting = canReplaceWithSelectedModelCandidate
        await work {
            guard let api, var local = state, let draft = local.draft, let etag = local.etag,
                  await api.admitsApprovedModelCandidateReplacement,
                  !entrancePreviewActive, !placingArtwork else {
                throw AuthoringFailure("ARTWORK_REPLACEMENT_UNAVAILABLE")
            }
            let access = viewportAccessID
            let intent: NativeDocumentState.Pending
            let candidate: NativeApprovedModelCandidateReplacement
            if let pending = local.pending {
                guard pending.origin == NativeApprovedModelCandidateReplacement.profile,
                      let reviewed = pending.modelCandidate, pending.draft == draft, pending.etag == etag else {
                    throw AuthoringFailure("EXHIBITION_PENDING")
                }
                intent = pending
                candidate = reviewed
            } else {
                guard starting, let entry = catalogEntries[catalogArtworkId] else {
                    throw AuthoringFailure("ARTWORK_REPLACEMENT_UNAVAILABLE")
                }
                let target = placementId
                let saved = try await api.exhibition(draft.id)
                let editing = try await publishedEditingAccess(draft.id, api: api, expected: saved)
                guard saved.draft == draft, saved.etag == etag, !editing.immutable else {
                    throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
                }
                candidate = try await api.discoverApprovedModelCandidate(entry, placementId: target)
                guard self.api === api, viewportAccessID == access, state?.revision == local.revision,
                      catalogArtworkId == entry.id, placementId == target, !dirty else {
                    throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED")
                }
                intent = .init(etag: etag, draft: draft, origin: NativeApprovedModelCandidateReplacement.profile,
                               modelCandidate: candidate)
                local.pending = intent
                try await persist(local, recordUndo: false)
            }
            let etagPrefix = "\"studio-r"
            guard etag.hasPrefix(etagPrefix),
                  let revision = etag.dropFirst(etagPrefix.count).split(separator: "-").first.flatMap({ Int($0) }),
                  (1..<2_147_483_647).contains(revision),
                  self.api === api, viewportAccessID == access,
                  state?.pending?.requestId == intent.requestId else {
                throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED")
            }
            let saved = try await api.replaceApprovedModelCandidate(
                draft: intent.draft, studioRevision: revision, etag: etag, requestId: intent.requestId,
                artworkId: candidate.artworkId, artworkRevision: candidate.artworkRevision,
                importJobId: candidate.importJobId, placementId: candidate.placementId
            )
            guard self.api === api, viewportAccessID == access,
                  var accepted = state, accepted.pending?.requestId == intent.requestId else {
                throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED")
            }
            accepted.draft = saved.draft
            accepted.etag = saved.etag
            accepted.savedEditVersion = saved.draft.document["editVersion"].integer
            accepted.pending = nil
            try await persist(accepted, recordUndo: false)
            past = []; future = []; comparison = saved
            syncFields()
            if ordinaryBoundedRuntime { await prepareOrdinaryBoundedPlacement() }
            notice = "선택한 승인 작품으로 교체한 전시 저장본을 확인했습니다."
        }
    }

    func save() async {
        await work {
            guard let api, var value = state else { return }
            guard value.pending?.productImage == nil else { throw AuthoringFailure("EXHIBITION_PENDING") }
            guard !boundedPendingRecoveryBlocked else { throw AuthoringFailure("EXHIBITION_PENDING") }
            let bounded = boundedPlacementCapability != nil
            let access = viewportAccessID, generation = boundedPolicyGeneration, token = viewportCredentials?.token
            if value.pending == nil {
                if bounded {
                    guard boundedPlacementReady, !formDirty, !positionDirty, !architectureDirty,
                          let baseline = boundedPlacementBaseline, let current = value.draft,
                          let selection = viewportSelection else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    if current.candidate == baseline.candidate {
                        value.draft = baseline; try await persist(value); syncFields()
                        notice = "저장된 배치와 같은 상태입니다."
                        return
                    }
                    var checked = baseline
                    try checked.acceptBoundedPlacement(current.candidate, selection: selection, savedBaseline: baseline)
                } else {
                    guard !immutable else { throw AuthoringFailure("PUBLISHED_DRAFT_IMMUTABLE", status: 409) }
                    value.draft = try appliedFields()
                }
                guard let draft = value.draft else { throw AuthoringFailure("EXHIBITION_REQUIRED") }
                value.pending = .init(etag: value.etag, draft: draft, origin: bounded ? "bounded-placement-v1" : nil)
                try await persist(value) // durable exact intent BEFORE the request
                if bounded {
                    guard viewportAccessID == access, boundedPolicyGeneration == generation,
                          viewportCredentials?.token == token, self.api === api,
                          state?.pending?.requestId == value.pending?.requestId else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                }
                syncEntrance()
                syncPublicationWatermark()
            }
            guard let intent = state?.pending else { throw AuthoringFailure("EXHIBITION_PENDING") }
            if try intent.draft.publicationWatermark()?.requiresQualifiedRuntime == true, !publicationWatermarkRuntimeAvailable { throw AuthoringFailure("WATERMARK_RUNTIME_UNAVAILABLE") }
            let saved: NativeStudioResponse
            do { saved = try await api.saveExhibition(intent.draft, requestId: intent.requestId, etag: intent.etag) }
            catch let error as AuthoringFailure {
                if (!bounded || (viewportAccessID == access && boundedPolicyGeneration == generation && viewportCredentials?.token == token && self.api === api)),
                   ([400, 403, 412, 422].contains(error.status) || error.code == "PUBLISHED_DRAFT_IMMUTABLE"), var rejected = state,
                   rejected.pending?.requestId == intent.requestId {
                    rejected.pending = nil; try await persist(rejected)
                    if error.code == "PUBLISHED_DRAFT_IMMUTABLE" { storedImmutable = true }
                }
                throw error
            }
            guard var accepted = state, accepted.pending?.requestId == intent.requestId else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            if bounded {
                guard viewportAccessID == access, boundedPolicyGeneration == generation, viewportCredentials?.token == token,
                      self.api === api, let cap = boundedPlacementCapability, let captured = boundedPlacementSession, let live = session,
                      live.tenantId == captured.tenantId, live.userId == captured.userId, live.role == captured.role,
                      live.csrfToken == captured.csrfToken, live.expiresAt == captured.expiresAt else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                let (nextRevision, overflow) = cap.revision.addingReportingOverflow(1)
                guard !overflow, saved.revision == nextRevision, saved.etag != cap.etag else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            }
            accepted.draft = saved.draft; accepted.etag = saved.etag; accepted.savedEditVersion = saved.draft.document["editVersion"].integer; accepted.pending = nil
            try await persist(accepted); comparison = nil; syncFields()
            if bounded {
                boundedPlacementCapability = nil; boundedPlacementPolicy = nil; boundedPlacementBaseline = nil
                boundedPlacementSession = nil; boundedPlacementEpoch = nil; boundedPolicyPending = true; storedImmutable = true
            }
            notice = "전시를 저장했습니다. 작품과 권리를 다시 확인한 뒤 미리보기와 내보내기로 진행하세요."
            if bounded {
                await prepareOrdinaryBoundedPlacement()
                do { try await list() } catch {
                    if boundedPolicyPending { catalogStatus = "전시 목록을 새로 확인하지 못했습니다." }
                    else { notice = "전시는 저장했습니다. 전시 목록을 새로 확인하지 못했습니다." }
                }
            } else { try await list() }
        }
    }
    var canCopyForEditing: Bool { !busy && copySourceAvailable }
    private var copySourceAvailable: Bool {
        guard !pending, !boundedReadOnly, !boundedPolicyPending,
              !viewportController.boundedReadOnly, viewportController.editingContextActive else { return false }
        let acknowledged = viewportController.acknowledges(draft: draft, tenantId: state?.tenantId, principalId: state?.principalId)
        if ordinaryBoundedRuntime {
            guard boundedProfileKnown == false, storedImmutable, !dirty, state?.etag != nil,
                  let imported = oexImport.state, imported.intent == nil, imported.jobId == nil, imported.fileName == nil,
                  imported.workspaceId == state?.workspaceId, imported.tenantId == state?.tenantId,
                  imported.principalId == state?.principalId else { return false }
            return acknowledged
        }
        return !selectedBoundedPolicy || state?.etag == nil || acknowledged
    }
    func copyForEditing() async {
        guard canCopyForEditing, let source = state else { return }
        await work {
            guard copySourceAvailable, state?.revision == source.revision,
                  state?.draft == source.draft, state?.etag == source.etag else {
                throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED")
            }
            var value = source
            let current: NativeExhibitionDraft
            if ordinaryBoundedRuntime {
                guard let saved = value.draft else { throw AuthoringFailure("EXHIBITION_REQUIRED") }
                current = saved
            } else {
                var applied = try appliedFields(allowPosition: true)
                if !placementId.isEmpty, applied.placements.contains(where: { $0["id"].string == placementId }) { try applied.movePlacement(placementId, position: [x,y,z]) }
                current = applied
            }
            value.draft = try current.copyForEditing(); value.etag = nil; value.savedEditVersion = nil; value.pending = nil
            try await persist(value); storedImmutable = false; reusablePublicationId = nil; comparison = nil; syncFields()
            notice = "편집할 사본을 이 Mac에 보관했습니다. 원래 공개용 저장본은 유지됩니다."
        }
    }
    func exportOEX() async {
        await work {
            guard !dirty, let draft, let etag = state?.etag, let api else { throw AuthoringFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            exportProgress = nil
            exporting = true
            defer {
                exporting = false
                if exportProgress != .complete { exportProgress = nil }
            }
            let bounded = session?.nativeOexExportProfile == "bounded-file-v1"
            let accessID = viewportAccessID, workspace = state?.workspaceId
            let principal = state?.principalId, tenant = state?.tenantId
            let panel = NSSavePanel()
            panel.title = "OEX 내보내기"
            panel.message = bounded
                ? "전시와 허용된 작품 파일을 새 이름으로 저장합니다. 기존 파일은 덮어쓰지 않습니다."
                : "전시와 내보내기가 허용된 작품 파일을 함께 저장합니다."
            panel.nameFieldStringValue = "전시.oex"
            panel.allowedContentTypes = [UTType(filenameExtension: "oex") ?? .data]
            panel.allowsOtherFileTypes = true; panel.canCreateDirectories = true; panel.isExtensionHidden = false
            guard let window = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.identifier?.rawValue == "manager" }) else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            let response = await withCheckedContinuation { continuation in
                panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
            }
            guard response == .OK, let url = panel.url else { notice = "내보내기를 취소했습니다."; return }
            try Task.checkCancellation()
            if bounded, FileManager.default.fileExists(atPath: url.path) {
                notice = "기존 파일을 보존했습니다. 새 파일 이름으로 다시 내보내세요."
                return
            }
            guard viewportAccessID == accessID, state?.workspaceId == workspace,
                  state?.principalId == principal, state?.tenantId == tenant,
                  self.draft == draft, state?.etag == etag, !dirty else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            exportProgress = .preparing
            let progress: @MainActor @Sendable (NativeOEXExportProgress) -> Void = { [weak self] update in
                guard !Task.isCancelled, let self, self.exporting, self.viewportAccessID == accessID else { return }
                self.exportProgress = .transport(update)
            }
            let archiveSHA256: String
            let writeOutput: (URL) async throws -> Void
            if bounded {
                let archive = try await api.exportBoundedOEX(exhibitionId: draft.id, etag: etag, progress: progress)
                archiveSHA256 = archive.sha256; writeOutput = { try await archive.write(to: $0) }
            } else {
                let archive = try await api.exportOEX(exhibitionId: draft.id, etag: etag, progress: progress)
                archiveSHA256 = archive.sha256; writeOutput = { try await archive.write(to: $0) }
            }
            guard viewportAccessID == accessID, state?.workspaceId == workspace,
                  state?.principalId == principal, state?.tenantId == tenant,
                  self.draft == draft, state?.etag == etag, !dirty else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            try Task.checkCancellation()
            let access = url.startAccessingSecurityScopedResource()
            defer { if access { url.stopAccessingSecurityScopedResource() } }
            exportProgress = .saving
            try await writeOutput(url)
            exportedURL = url; exportedSHA256 = archiveSHA256
            exportProgress = .complete
            notice = "OEX 파일을 저장했습니다. 인터넷 공개는 공개 단계에서 진행하세요."
        }
    }
    func compare() async {
        await work {
            guard let api, let draft, state?.etag != nil else { throw AuthoringFailure("EXHIBITION_NOT_SAVED") }
            comparison = try await api.exhibition(draft.id)
            notice = "저장본을 비교할 수 있습니다. 현재 편집 내용은 바꾸지 않았습니다."
        }
    }
    func adoptComparison() async {
        await work(allowViewingPlanInput: true, allowViewingPointInput: true) {
            guard !ordinaryBoundedRuntime, !pending, let saved = comparison, let api, let original = state,
                  original.draft?.id == saved.draft.id else { throw AuthoringFailure("EXHIBITION_PENDING") }
            let access = viewportAccessID
            let editingAccess = try await publishedEditingAccess(saved.draft.id, api: api)
            let current = try await api.exhibition(saved.draft.id)
            try saved.requireSameComparison(current)
            guard viewportAccessID == access, let valueBefore = state, valueBefore.revision == original.revision,
                  valueBefore.draft == original.draft, valueBefore.etag == original.etag,
                  valueBefore.pending == nil, comparison?.revision == saved.revision,
                  comparison?.etag == saved.etag, comparison?.draft == saved.draft else {
                throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED")
            }
            var value = valueBefore
            value.draft = saved.draft; value.etag = saved.etag; value.savedEditVersion = saved.draft.document["editVersion"].integer
            try await persist(value); storedImmutable = editingAccess.immutable; reusablePublicationId = editingAccess.publicationId; comparison = nil; syncFields()
            notice = "확인한 저장본으로 전환했습니다."
        }
    }
    func addArtwork() async {
        await work {
            guard !immutable, !pending, let api, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            let approved = try await api.studioArtwork(artworkId)
            var next = try appliedFields(allowPosition: true)
            try next.addApprovedArtwork(approved.artwork, roomId: roomId, position: [x, y, z])
            value.draft = next; try await persist(value); artworkId = ""; placementId = next.placements.last?["id"].string ?? ""; syncFields()
            notice = "승인한 작품을 전시에 배치하고 이 Mac에 보관했습니다."
        }
    }
    func movePlacement() async {
        await work {
            guard !immutable, !pending, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            var next = try appliedFields(allowPosition: true); try next.movePlacement(placementId, position: [x, y, z])
            value.draft = next; try await persist(value); syncEntrance(); syncInstallation()
            notice = "작품 위치를 이 Mac에 보관했습니다."
        }
    }
    private func syncEntrance() {
        do {
            entrance = try draft?.viewerEntranceSettings() ?? ViewerEntranceSettings()
            coverEntry = CoverEntryFormBuffer(design: entrance.design)
            entranceEditable = true
        } catch {
            entrance = ViewerEntranceSettings()
            coverEntry = CoverEntryFormBuffer(design: nil)
            entranceEditable = false
            failure = error as? AuthoringFailure ?? AuthoringFailure("VIEWER_ENTRANCE_INVALID")
        }
    }
    private func syncPublicationWatermark() {
        do {
            publicationWatermark = PublicationWatermarkFormBuffer(settings: try draft?.publicationWatermark())
            publicationWatermarkReadable = true; publicationWatermarkStatus = ""
        } catch {
            publicationWatermark = PublicationWatermarkFormBuffer()
            publicationWatermarkReadable = false
            publicationWatermarkStatus = "저장된 워터마크 설정을 이 실행본에서 해석하지 못했습니다. 원래 설정은 유지됩니다."
        }
    }
    func syncFields() {
        clearInfoStandPreview()
        resetViewingPointInput()
        let selected = viewportSelection
        title = draft?.title ?? ""
        syncEntrance()
        syncPublicationWatermark()
        syncFloor()
        if draft?.rooms.contains(where: { $0["id"].string == roomId }) != true { roomId = draft?.rooms.first?["id"].string ?? "" }
        selectRoom(clearPlaqueSelection: false)
        if draft?.placements.contains(where: { $0["id"].string == placementId }) != true { placementId = draft?.placements.first?["id"].string ?? "" }
        selectPlacement(clearLightSelection: false)
        if let selected, let draft, selected.belongs(to: draft) { viewportSelection = selected; if selected.kind != "placement" { catalogArtworkId = "" } }
        syncArchitecture()
        if let draft, draft.candidate["lights"].items?.contains(where: { $0["id"].string == selectedLightId }) == true {
            viewportSelection = nil; syncArchitecture()
        } else { selectedLightId = "" }
        syncLight()
        syncSpaceLighting()
        syncViewingPlan()
        if benches.contains(where: { $0.id == selectedBenchId }) {
            viewportSelection = nil; syncArchitecture()
        } else { selectedBenchId = "" }
        syncBench()
        if plaques.contains(where: { $0.id == selectedPlaqueId }) {
            viewportSelection = nil; syncArchitecture()
        } else { selectedPlaqueId = "" }
        syncPlaque()
    }
    func selectRoom(clearPlaqueSelection: Bool = true) {
        if clearPlaqueSelection && !plaqueDirty && !furnishingDirty && !spaceLightingDirty { self.clearPlaqueSelection() }
        guard let room = draft?.rooms.first(where: { $0["id"].string == roomId }) else { return }
        if let draft, viewportSelection.flatMap({ roomForSelection($0, draft: draft) }) != roomId { viewportSelection = ViewportSelection(kind: "room", id: roomId) }
        roomName = room["name"].string ?? ""; width = room["dimensions"]["width"].double ?? 12; height = room["dimensions"]["height"].double ?? 4; depth = room["dimensions"]["depth"].double ?? 8
        syncArchitecture()
    }
    private func roomForSelection(_ selection: ViewportSelection, draft: NativeExhibitionDraft) -> String? {
        switch selection.kind {
        case "room": return selection.id
        case "placement": return draft.placements.first(where: { $0["id"].string == selection.id })?["roomId"].string
        case "surface": return draft.candidate["surfaces"].items?.first(where: { $0["id"].string == selection.id })?["roomId"].string
        case "opening":
            guard let wall = draft.candidate["openings"].items?.first(where: { $0["id"].string == selection.id })?["surfaceId"].string else { return nil }
            return draft.candidate["surfaces"].items?.first(where: { $0["id"].string == wall })?["roomId"].string
        default: return nil
        }
    }
    func selectPlacement(clearLightSelection: Bool = true) {
        if clearLightSelection {
            guard !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying else { return }
            selectedLightId = ""; syncLight(); clearBenchSelection(); clearPlaqueSelection()
        }
        guard let p = draft?.placements.first(where: { $0["id"].string == placementId }), let values = p["transform"]["position"].items, values.count == 3 else { syncInstallation(); return }
        viewportSelection = ViewportSelection(kind: "placement", id: placementId)
        catalogArtworkId = draft?.catalogSourceID(for: placementId) ?? ""
        x = values[0].double ?? 0; y = values[1].double ?? 0; z = values[2].double ?? 0
        syncInstallation()
        syncArchitecture()
    }
}

enum OEXExportDisplayProgress: Equatable {
    case preparing, transport(NativeOEXExportProgress), saving, complete

    var label: String {
        switch self {
        case .preparing: "작품의 권리와 파일을 확인하고 OEX를 준비하고 있습니다."
        case .transport(.packing(let completed?, let total?)): "OEX 구성 파일 확인 · \(completed) / \(total)개"
        case .transport(.packing): "OEX에 넣을 파일을 확인하고 있습니다."
        case .transport(.checkingPackage): "서버에서 만든 OEX 파일을 확인하고 있습니다."
        case .transport(.receiving(let bytes, let total)):
            total.map { "OEX 파일 받기 · \(formatted(bytes)) / \(formatted($0))" }
                ?? "OEX 파일 받기 · \(formatted(bytes))"
        case .transport(.verifying): "OEX 파일과 내보내기 결과를 확인하고 있습니다."
        case .saving: "선택한 위치에 OEX 파일을 저장하고 있습니다."
        case .complete: "OEX 파일 저장 완료"
        }
    }

    var fraction: Double? {
        switch self {
        case .transport(.packing(let completed?, let total?)): min(0.99, Double(completed) / Double(total))
        case .transport(.receiving(let bytes, let total?)): min(0.99, Double(bytes) / Double(total))
        case .complete: 1
        default: nil
        }
    }

    private func formatted(_ bytes: Int) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }
}

struct NativeOEXExportControls: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    var showFeedback = false
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button("OEX 내보내기", systemImage: "square.and.arrow.up") { Task { await model.exportOEX() } }
                .disabled(!ready || !model.canExport)
                .keyboardShortcut("e", modifiers: [.command, .shift])
            Text("저장한 전시와 허용된 작품 파일을 OEX로 저장합니다.").foregroundStyle(.secondary)
            if ready && (model.dirty || model.state?.etag == nil) { Text("먼저 전시를 저장하세요.").foregroundStyle(.secondary) }
            if let progress = model.exportProgress, model.exporting || model.exportedURL != nil {
                VStack(alignment: .leading, spacing: 4) {
                    Text(progress.label)
                    Group {
                        if let fraction = progress.fraction { ProgressView(value: fraction) }
                        else { ProgressView() }
                    }
                    .progressViewStyle(.linear)
                    .frame(height: 8)
                    .accessibilityLabel(progress.label)
                }
            }
            if let url = model.exportedURL {
                Text(url.path).textSelection(.enabled)
                Button("Finder에서 보기") { NSWorkspace.shared.activateFileViewerSelecting([url]) }
            }
            if showFeedback {
                if let failure = model.failure { Text(failure.localizedDescription); DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) } }
                if !model.notice.isEmpty { Text(model.notice).foregroundStyle(.secondary) }
            }
        }
    }
}
