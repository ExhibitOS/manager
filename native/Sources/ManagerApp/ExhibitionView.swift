// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ImageIO
import UniformTypeIdentifiers
import ManagerCore

@MainActor
final class ExhibitionModel: ObservableObject {
    @Published private(set) var state: NativeDocumentState?
    @Published private(set) var items: [NativeExhibitionSummary] = []
    @Published private(set) var library: [ArtworkSummary] = []
    @Published private(set) var catalogEntries: [String: ArtworkCatalogEntry] = [:]
    @Published private(set) var catalogFailures: Set<String> = []
    @Published var catalogQuery = ""
    @Published var catalogFilter = ArtworkCatalogFilter.all
    @Published private(set) var catalogArtworkId = ""
    @Published private(set) var catalogStatus = ""
    @Published private(set) var artworkPlacement: ArtworkPlacementIntent?
    @Published private(set) var artworkPlacementTarget: ViewportPlacementTarget?
    @Published private(set) var placementApplying = false
    private var placementAttempt: UUID?
    var placingArtwork: Bool { artworkPlacement != nil || placementApplying }
    var canPlaceVisually: Bool { canSelectCatalog && !immutable && viewportPresentation == .threeD && viewportController.connected && viewportController.placementSupported && catalogEntries[catalogArtworkId] != nil }
    @Published private(set) var libraryCursor: String?
    private var catalogTask: Task<Void, Never>?
    @Published private(set) var catalogImages: [String: NSImage] = [:]
    @Published private(set) var catalogPreviewFailures: Set<String> = []
    private var catalogImageOrder: [String] = []
    private var catalogThumbnailTail: Task<Void, Never>?
    private var catalogThumbnailTailId: UUID?
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
    @Published private(set) var immutable = false
    @Published private(set) var busy = false
    @Published private(set) var failure: AuthoringFailure?
    @Published private(set) var notice = ""
    @Published private(set) var exporting = false
    @Published private(set) var exportedURL: URL?
    @Published private(set) var exportedSHA256: String?
    @Published var query = ""
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
    private var entrancePreviewBase: NativeExhibitionDraft?
    private var entrancePreviewTask: Task<Void, Never>?
    private var entrancePreviewGeneration = UUID()
    var entrancePreviewActive: Bool { entrancePreviewDraft != nil }
    var canRecoverEntranceHost: Bool {
        guard entrancePreviewActive, !busy, !pending, !placingArtwork, !architectureApplying else { return false }
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
    var installationDirty: Bool { installation != installationBaseline }
    var viewportDraft: NativeExhibitionDraft? {
        if entranceHostRecovering { return draft }
        if entrancePreviewBase == draft, let value = entrancePreviewDraft { return value }
        if let value = furnishingPreview?.displayDraft { return value }
        if let value = spaceLightingPreview?.displayDraft { return value }
        if let value = plaquePreview?.displayDraft { return value }
        if let value = benchPreview?.displayDraft { return value }
        if let value = lightPreview?.displayDraft { return value }
        if let value = surfaceMaterialPreview?.displayDraft { return value }
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
    var architectureDirty: Bool { architectureGeometryDirty || surfaceMaterialDirty || lightDirty || benchDirty || plaqueDirty || spaceLightingDirty || furnishingDirty }
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
    var furnishingDirty: Bool { furnishing != furnishingBaseline }
    var furnishingPreviewing: Bool { furnishingPreview != nil }
    var selectedHasFurnishing: Bool { draft.map { FurnishingCatalog.hasVisual(in: $0, hostId: placementId) } ?? false }
    var canEditFurnishing: Bool {
        !busy && !pending && !immutable && !placingArtwork && !formDirty && !positionDirty && !architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !spaceLightingDirty && !architectureApplying && viewportController.connected && viewportController.furnishingVisualsSupported && furnishingCatalogReady && furnishing != nil
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
    }
    var canApplyLight: Bool {
        guard !busy, !pending, !immutable, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
              !surfaceMaterialDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying, viewportController.connected, viewportController.lightInspectorSupported,
              lightPreviewAccessID == viewportAccessID, let preview = lightPreview, preview.base == draft,
              selectedLightId == preview.command.lightId, let fields = light else { return false }
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
        guard canEditPlaque, plaquePreviewAccessID == viewportAccessID, let preview = plaquePreview,
              preview.base == draft, selectedPlaqueId == preview.command.plaqueId else { return false }
        return (try? currentPlaqueCommand()) == preview.command
    }
    @Published var surfaceMaterial: SurfaceMaterialFormBuffer? { didSet { surfaceMaterialPreview = nil; surfaceMaterialStatus = "" } }
    private var surfaceMaterialBaseline: SurfaceMaterialFormBuffer?
    @Published private var surfaceMaterialPreview: SurfaceMaterialPreview?
    private var surfaceMaterialPreviewAccessID: UUID?
    @Published private(set) var surfaceMaterialStatus = ""
    var surfaceMaterialDirty: Bool { surfaceMaterial != surfaceMaterialBaseline }
    var surfaceMaterialPreviewing: Bool { surfaceMaterialPreview != nil }
    var canPreviewSurfaceMaterial: Bool { !busy && !pending && !immutable && !formDirty && !positionDirty && !architectureGeometryDirty && !lightDirty && !benchDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && !architectureApplying && surfaceMaterialDirty && !surfaceMaterialPreviewing && viewportController.connected }
    var canApplySurfaceMaterial: Bool {
        guard !busy, !pending, !immutable, !formDirty, !positionDirty, !architectureGeometryDirty, !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying,
              viewportController.connected, surfaceMaterialPreviewAccessID == viewportAccessID,
              let preview = surfaceMaterialPreview, preview.base == draft,
              preview.command.selection == viewportSelection, let fields = surfaceMaterial else { return false }
        return (try? fields.command()) == preview.command
    }
    var canApplyArchitecture: Bool { !busy && !pending && !immutable && !formDirty && !positionDirty && !architectureApplying && architectureGeometryDirty && !surfaceMaterialDirty && !lightDirty && !benchDirty && !plaqueDirty && !furnishingDirty && !spaceLightingDirty && viewportController.connected }
    @Published private(set) var viewportAccessID = UUID()
    private(set) var viewportCredentials: LocalAuthoringSession?
    @Published private var past: [NativeExhibitionDraft] = []
    @Published private var future: [NativeExhibitionDraft] = []
    var canUndo: Bool { !entrancePreviewActive && !immutable && !past.isEmpty && !pending && !busy && !formDirty && !positionDirty && !architectureDirty && !architectureApplying && !placingArtwork }
    var canRedo: Bool { !entrancePreviewActive && !immutable && !future.isEmpty && !pending && !busy && !formDirty && !positionDirty && !architectureDirty && !architectureApplying && !placingArtwork }
    private var api: AuthoringAPI?
    private var session: AuthoringSession?
    private var read: (() async throws -> NativeDocumentState)?
    private var write: ((NativeDocumentState) async throws -> NativeDocumentState)?
    var draft: NativeExhibitionDraft? { state?.draft }
    var pending: Bool { state?.pending != nil }
    var formDirty: Bool {
        guard let draft else { return false }
        let entranceDirty = (entranceEditable && ((try? draft.viewerEntranceSettings()) != entrance || coverEntry.isDirty)) || installationDirty
        guard let room = draft.rooms.first(where: { $0["id"].string == roomId }) else { return title != draft.title || entranceDirty }
        return title != draft.title || entranceDirty || roomName != room["name"].string || width != room["dimensions"]["width"].double || height != room["dimensions"]["height"].double || depth != room["dimensions"]["depth"].double
    }
    var positionDirty: Bool {
        guard [x, y, z].allSatisfy({ $0.isFinite }) else { return true }
        if let p = draft?.placements.first(where: { $0["id"].string == placementId }), let position = p["transform"]["position"].items, position.count == 3 { return x != position[0].double || y != position[1].double || z != position[2].double }
        return !artworkId.isEmpty
    }
    var canExport: Bool { !busy && !placingArtwork && !dirty && state?.etag != nil && draft != nil }
    var dirty: Bool { state?.dirty == true || formDirty || positionDirty || architectureDirty || architectureApplying }
    func connect(api: AuthoringAPI, session: AuthoringSession, credentials: LocalAuthoringSession, read: @escaping () async throws -> NativeDocumentState, write: @escaping (NativeDocumentState) async throws -> NativeDocumentState) async {
        // An owned begin/commit (including its store write) must finish before
        // credentials or store closures can change. A stable ghost can cancel
        // against its OLD connection, then reconnect through normal initialization.
        guard !busy, !placementApplying, !architectureApplying else { return }
        await cancelVisualArtworkPlacement()
        // Cancellation awaits the old renderer. Another owned operation may
        // have started in the meantime; never replace its account/store binding.
        guard !busy, !placingArtwork, !architectureApplying else { return }
        if let old = self.session, old.tenantId != session.tenantId || old.userId != session.userId {
            guard !dirty else { self.api = nil; viewportCredentials = nil; failure = AuthoringFailure("DRAFT_ACCOUNT_MISMATCH"); return }
            state = nil; items = []; library = []; comparison = nil; exportedURL = nil; exportedSHA256 = nil; catalogArtworkId = ""; catalogStatus = ""
        }
        self.api = api; self.session = session
        let accessChanged = viewportCredentials?.token != credentials.token || viewportCredentials?.origin != credentials.origin
        viewportCredentials = credentials
        if accessChanged { entrancePreviewTask?.cancel(); entrancePreviewGeneration = UUID(); entrancePreviewDraft = nil; entrancePreviewBase = nil; entranceHostRecovering = false; entranceRecoveryEpoch = nil; entrancePreviewBusy = false; entrancePreviewStatus = ""; invalidateEntranceThumbnails(); viewportAccessID = UUID(); furnishingCatalogReady = false; furnishingCatalogAccessID = nil; furnishingCatalogStatus = ""; furnishingImage = nil; furnishingImageAttempted = false; surfaceMaterialPreview = nil; spaceLightingPreview = nil; spaceLightingPreviewAccessID = nil; lightPreview = nil; lightPreviewAccessID = nil; benchPreview = nil; benchPreviewAccessID = nil; plaquePreview = nil; plaquePreviewAccessID = nil; catalogTask?.cancel(); catalogEntries = [:]; catalogFailures = []; catalogImages = [:]; catalogImageOrder = []; catalogPreviewFailures = [] }
        self.read = read; self.write = write
        // Reconnecting may replace the native store closures even for the same
        // account/origin. Keep text, but never apply a prior binding's preview.
        furnishingPreview = nil; furnishingPreviewAccessID = nil; spaceLightingPreview = nil; spaceLightingPreviewAccessID = nil; lightPreview = nil; lightPreviewAccessID = nil; benchPreview = nil; benchPreviewAccessID = nil; plaquePreview = nil; plaquePreviewAccessID = nil
        await work {
            if state == nil {
                let stored = try await read()
                guard stored.tenantId.lowercased() == session.tenantId.lowercased(), stored.principalId.lowercased() == session.userId.lowercased() else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                state = stored; syncFields()
                if stored.pending != nil { notice = "저장 요청의 응답을 확인해야 합니다. 같은 요청으로 다시 확인하세요." }
                else if stored.dirty { notice = "이 Mac에 보관한 미완료 전시를 복원했습니다." }
            }
            try await list()
            if let draft, state?.etag != nil { immutable = !(try await api.publications(exhibitionId: draft.id)).isEmpty } else { immutable = false }
        }
    }
    func previewEntrance(_ action: EntrancePreviewAction = .show, thumbnails: Bool = false, systemReducedMotion: Bool) {
        guard !busy, !pending, !placingArtwork, !architectureApplying, !entrancePreviewBusy, entranceEditable,
              !surfaceMaterialPreviewing, !lightPreviewing, !benchPreviewing, !plaquePreviewing, !furnishingPreviewing, !spaceLightingPreviewing,
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
        guard entranceHostRecovering, let previousEpoch = entranceRecoveryEpoch, let base = entrancePreviewBase, draft == base,
              let credentials = viewportCredentials,
              viewportController.ordinaryRecoveryReady(replacing: previousEpoch, draft: base, tenantId: credentials.tenantId, principalId: credentials.principalId) else { return }
        entrancePreviewTask?.cancel(); entrancePreviewTask = nil; entrancePreviewGeneration = UUID()
        entrancePreviewDraft = nil; entrancePreviewBase = nil; entranceHostRecovering = false; entranceRecoveryEpoch = nil; entrancePreviewBusy = false
        entrancePreviewStatus = "편집 화면을 다시 연결했습니다. 입력·선택·실행 취소 이력과 마지막으로 선택한 보기는 유지됩니다."
    }
    private func work(_ body: () async throws -> Void) async {
        guard !entrancePreviewActive, !busy, !placingArtwork, api != nil, session != nil else { return }
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
    func acceptViewportCandidate(_ candidate: DocumentValue, base: NativeExhibitionDraft) async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, draft == base, var value = state, var next = draft else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            try next.acceptGeometry(candidate); value.draft = next
            try await persist(value); syncFields()
            notice = "3D 변경을 이 Mac에 보관했습니다. 전시 저장은 별도로 진행하세요."
        }
    }
    func acceptInstallationCandidate(_ candidate: DocumentValue, base: NativeExhibitionDraft, command: InstallationCommand) async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, draft == base,
                  command.placementId.lowercased() == placementId.lowercased(), var value = state, var next = draft else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
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
        surfaceMaterialPreview = nil; surfaceMaterialPreviewAccessID = nil
        surfaceMaterial = nil; surfaceMaterialBaseline = nil; surfaceMaterialStatus = ""
        guard let draft, let selection = viewportSelection, selection.kind == "surface" else { return }
        do {
            let fields = try SurfaceMaterialFormBuffer(draft: draft, selection: selection)
            surfaceMaterial = fields; surfaceMaterialBaseline = fields
        } catch { surfaceMaterialStatus = "이 표면의 재질 설정은 현재 편집할 수 없습니다. 저장된 설정은 유지됩니다." }
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
            let fields = try PlaqueFormBuffer(roomId: roomId)
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
        return plaqueAdding ? .add(settings) : .set(settings)
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
            guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                  !surfaceMaterialDirty, !lightDirty, !benchDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying, plaquePreviewAccessID == viewportAccessID,
                  let preview = plaquePreview, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
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
            guard !immutable, !pending, !placingArtwork, !formDirty, !positionDirty, !architectureGeometryDirty,
                  !surfaceMaterialDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying, lightPreviewAccessID == viewportAccessID,
                  let preview = lightPreview, let fields = light, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, selectedLightId: selectedLightId, command: fields.command())
            try await persist(value); syncFields()
            notice = "조명 변경을 이 Mac에 보관했습니다. 되돌리기로 취소할 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    func discardSurfaceMaterial() { guard !architectureApplying, !busy, !pending else { return }; syncSurfaceMaterial() }
    func previewSurfaceMaterial() async {
        guard canPreviewSurfaceMaterial, let fields = surfaceMaterial, let draft else { return }
        do {
            let command = try fields.command()
            _ = try command.encodedValue(in: draft, selected: viewportSelection)
            architectureApplying = true
            defer { architectureApplying = false }
            await viewportController.perform(.surfaceMaterial(command), model: self)
        } catch { failure = error as? AuthoringFailure ?? AuthoringFailure("SURFACE_MATERIAL_INVALID") }
    }
    func acceptSurfaceMaterialPreview(_ candidate: DocumentValue, base: NativeExhibitionDraft, command: SurfaceMaterialCommand) async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureGeometryDirty, !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, architectureApplying,
                  draft == base, try surfaceMaterial?.command() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            surfaceMaterialPreview = try SurfaceMaterialPreview(candidate: candidate, base: base, command: command, selected: viewportSelection)
            surfaceMaterialPreviewAccessID = viewportAccessID
            surfaceMaterialStatus = "재질을 미리 보고 있습니다. 적용하면 이 Mac에 보관되며 취소하면 원래 표시로 돌아갑니다."
        }
    }
    func applySurfaceMaterial() async {
        guard canApplySurfaceMaterial else { return }
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureGeometryDirty, !lightDirty, !benchDirty, !plaqueDirty, !furnishingDirty && !spaceLightingDirty, !architectureApplying,
                  surfaceMaterialPreviewAccessID == viewportAccessID, let preview = surfaceMaterialPreview,
                  let fields = surfaceMaterial, let draft, var value = state else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            value.draft = try preview.apply(to: draft, selected: viewportSelection, command: fields.command())
            try await persist(value); syncFields()
            notice = "재질 변경을 이 Mac에 보관했습니다. 실행 취소로 되돌릴 수 있으며 전시 저장은 별도로 진행합니다."
        }
    }
    func syncArchitecture() {
        syncSurfaceMaterial()
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
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, let previous = past.last, let current = draft, var value = state else { return }
            var next = current; try next.replaceCandidate(previous.candidate); value.draft = next
            try await persist(value, recordUndo: false); past.removeLast(); future.append(current); syncFields()
            notice = "이전 편집으로 되돌렸습니다. 전시 저장본은 바꾸지 않았습니다."
        }
    }
    func redo() async {
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, let previous = future.last, let current = draft, var value = state else { return }
            var next = current; try next.replaceCandidate(previous.candidate); value.draft = next
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
        guard canPlaceVisually, let api, let credentials = viewportCredentials, let base = draft,
              let entry = catalogEntries[catalogArtworkId] else { return }
        let access = viewportAccessID, room = roomId, attempt = UUID()
        placementAttempt = attempt; placementApplying = true; failure = nil
        catalogStatus = "승인한 작품을 확인하고 있습니다."
        defer { if placementAttempt == attempt { placementAttempt = nil }; placementApplying = false }
        do {
            let approved = try await api.studioArtwork(entry.id)
            guard placementAttempt == attempt, viewportAccessID == access, draft == base, roomId == room, catalogArtworkId == entry.id,
                  !immutable, !pending, !formDirty, !positionDirty, !architectureDirty else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            let intent = try ArtworkPlacementIntent(artworkId: entry.id, expectedRevision: entry.revision, tenantId: credentials.tenantId, artwork: approved.artwork, roomId: room, base: base)
            artworkPlacement = intent; artworkPlacementTarget = nil
            _ = try await viewportController.placement(intent, phase: .begin)
            guard artworkPlacement?.requestId == intent.requestId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            catalogStatus = intent.targetKind == .wall ? "3D 공간의 벽을 클릭해 미리 본 뒤 ‘여기에 배치’를 선택하세요." : "3D 공간의 바닥을 클릭해 미리 본 뒤 ‘여기에 배치’를 선택하세요."
        } catch {
            artworkPlacement = nil; artworkPlacementTarget = nil
            catalogStatus = "3D 배치를 시작하지 못했습니다. 기존 선택과 입력은 유지됩니다. 숫자로도 배치할 수 있습니다."
            viewportController.diagnose("PLACE", error)
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
    }
    func cancelCatalogPlacement() {
        guard !busy, !pending else { return }
        artworkId = ""
        catalogStatus = "새 배치 선택을 취소했습니다. 원본 작품과 기존 배치는 그대로입니다."
    }
    func refresh(more: Bool = false) async { await work { try await list(more: more) } }
    func new() async {
        await work {
            guard !pending, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            value.draft = try NativeExhibitionDraft.new(); value.etag = nil; value.savedEditVersion = nil
            try await persist(value); immutable = false; comparison = nil; syncFields()
            notice = "새 전시를 이 Mac에 보관했습니다. 공간과 작품을 구성한 뒤 전시를 저장하세요."
        }
    }
    func open(_ id: String) async {
        await work {
            guard !immutable, !pending, let api, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            let saved = try await api.exhibition(id)
            let frozen = !(try await api.publications(exhibitionId: id)).isEmpty
            value.draft = saved.draft; value.etag = saved.etag; value.savedEditVersion = saved.draft.document["editVersion"].integer
            try await persist(value); immutable = frozen; comparison = nil; syncFields()
            notice = frozen ? "공개용 저장본을 열었습니다. 복제해서 편집할 수 있습니다." : "저장한 전시를 열었습니다."
        }
    }
    private func appliedFields(allowPosition: Bool = false) throws -> NativeExhibitionDraft {
        guard !architectureDirty, !architectureApplying else { throw AuthoringFailure("ARCHITECTURE_APPLY_REQUIRED") }
        guard !positionDirty || allowPosition else { throw AuthoringFailure("PLACEMENT_APPLY_REQUIRED") }
        guard var draft else { throw AuthoringFailure("EXHIBITION_REQUIRED") }
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
            value.draft = try appliedFields(); try await persist(value); syncEntrance(); syncInstallation()
            notice = "현재 전시 입력을 이 Mac에 보관했습니다. 아직 전시 저장본에 반영하지 않았습니다."
        }
    }
    func save() async {
        await work {
            guard let api, var value = state else { return }
            if value.pending == nil {
                guard !immutable else { throw AuthoringFailure("PUBLISHED_DRAFT_IMMUTABLE", status: 409) }
                value.draft = try appliedFields()
                guard let draft = value.draft else { throw AuthoringFailure("EXHIBITION_REQUIRED") }
                value.pending = .init(etag: value.etag, draft: draft)
                try await persist(value) // durable exact intent BEFORE the request
                syncEntrance()
            }
            guard let intent = state?.pending else { throw AuthoringFailure("EXHIBITION_PENDING") }
            let saved: NativeStudioResponse
            do { saved = try await api.saveExhibition(intent.draft, requestId: intent.requestId, etag: intent.etag) }
            catch let error as AuthoringFailure {
                if [400, 403, 412, 422].contains(error.status) || error.code == "PUBLISHED_DRAFT_IMMUTABLE", var rejected = state {
                    rejected.pending = nil; try await persist(rejected)
                    if error.code == "PUBLISHED_DRAFT_IMMUTABLE" { immutable = true }
                }
                throw error
            }
            guard var accepted = state, accepted.pending?.requestId == intent.requestId else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            accepted.draft = saved.draft; accepted.etag = saved.etag; accepted.savedEditVersion = saved.draft.document["editVersion"].integer; accepted.pending = nil
            try await persist(accepted); comparison = nil; syncFields()
            notice = "전시를 저장했습니다. 작품과 권리를 다시 확인한 뒤 미리보기와 내보내기로 진행하세요."
            try await list()
        }
    }
    func copyForEditing() async {
        await work {
            guard !pending, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            var current = try appliedFields(allowPosition: true)
            if !placementId.isEmpty, current.placements.contains(where: { $0["id"].string == placementId }) { try current.movePlacement(placementId, position: [x,y,z]) }
            value.draft = try current.copyForEditing(); value.etag = nil; value.savedEditVersion = nil; value.pending = nil
            try await persist(value); immutable = false; comparison = nil; syncFields()
            notice = "편집할 사본을 이 Mac에 보관했습니다. 원래 공개용 저장본은 유지됩니다."
        }
    }
    func exportOEX() async {
        await work {
            guard !dirty, let draft, let etag = state?.etag, let api else { throw AuthoringFailure("EXHIBITION_EXPORT_SAVE_REQUIRED") }
            exporting = true
            defer { exporting = false }
            let panel = NSSavePanel()
            panel.title = "OEX 내보내기"
            panel.message = "전시와 내보내기가 허용된 작품 파일을 함께 저장합니다."
            panel.nameFieldStringValue = "전시.oex"
            panel.allowedContentTypes = [UTType(filenameExtension: "oex") ?? .data]
            panel.allowsOtherFileTypes = true; panel.canCreateDirectories = true; panel.isExtensionHidden = false
            guard let window = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.identifier?.rawValue == "manager" }) else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            let response = await withCheckedContinuation { continuation in
                panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
            }
            guard response == .OK, let url = panel.url else { notice = "내보내기를 취소했습니다."; return }
            try Task.checkCancellation()
            let archive = try await api.exportOEX(exhibitionId: draft.id, etag: etag)
            guard self.draft == draft, state?.etag == etag, !dirty else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
            try Task.checkCancellation()
            let access = url.startAccessingSecurityScopedResource()
            defer { if access { url.stopAccessingSecurityScopedResource() } }
            try await archive.write(to: url)
            exportedURL = url; exportedSHA256 = archive.sha256
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
        await work {
            guard !pending, let saved = comparison, let api, var value = state else { throw AuthoringFailure("EXHIBITION_PENDING") }
            value.draft = saved.draft; value.etag = saved.etag; value.savedEditVersion = saved.draft.document["editVersion"].integer
            let frozen = !(try await api.publications(exhibitionId: saved.draft.id)).isEmpty
            try await persist(value); immutable = frozen; comparison = nil; syncFields()
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
    func syncFields() {
        let selected = viewportSelection
        title = draft?.title ?? ""
        syncEntrance()
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

struct NativeExhibitionView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    let mode: Mode
    let connect: () async -> Void
    @State private var discardPrompt = false
    @State private var pendingOpen: String?
    @State private var pendingNew = false
    @State private var pendingAdopt = false
    @State private var inspectorVisible = true
    @State private var inspectorInputs: [String: InstallationInspectorState] = [:]
    private var hasSelectedPlacement: Bool {
        guard let selected = model.viewportSelection, selected.kind == "placement", selected.id == model.placementId,
              let draft = model.draft else { return false }
        return selected.belongs(to: draft)
    }
    private var inspectorPresented: Binding<Bool> {
        Binding(get: { inspectorVisible && (hasSelectedPlacement || !model.selectedLightId.isEmpty || !model.selectedBenchId.isEmpty || !model.selectedPlaqueId.isEmpty) }, set: { inspectorVisible = $0 })
    }
    private var selectedInputs: Binding<InstallationInspectorState> {
        let key = [model.state?.tenantId ?? "", model.state?.principalId ?? "", model.draft?.id ?? "", model.placementId].joined(separator: ":")
        return Binding(get: { inspectorInputs[key] ?? InstallationInspectorState() }, set: { inspectorInputs[key] = $0 })
    }
    private func requestOpen(_ id: String?) {
        guard !model.entrancePreviewActive, !model.pending else { return }
        if model.dirty { pendingOpen = id; pendingNew = id == nil; pendingAdopt = false; discardPrompt = true }
        else { Task { if let id { await model.open(id) } else { await model.new() } } }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("작품을 배치하고 전시를 저장하세요. 공개는 나중에 선택합니다.").foregroundStyle(.secondary)
            if !ready { Text("준비 화면에서 로컬 제작을 시작하세요.") }
            HStack {
                TextField("전시 검색", text: $model.query).onSubmit { Task { await model.refresh() } }
                Button("검색") { Task { await model.refresh() } }
                Button("새 전시", systemImage: "plus") { requestOpen(nil) }.disabled(model.pending)
            }.disabled(!ready || model.busy)
            List(selection: Binding<String?>(get: { model.items.contains(where: { $0.id == model.draft?.id }) ? model.draft?.id : nil }, set: { if let id = $0 { requestOpen(id) } })) {
                ForEach(model.items) { item in
                    HStack { Text(item.title); Spacer(); Text("저장본 \(item.revision)").foregroundStyle(.secondary) }.tag(item.id)
                }
            }.frame(minHeight: 120, maxHeight: 220).disabled(!ready || model.busy || model.pending)
            if model.cursor != nil { Button("전시 더 보기") { Task { await model.refresh(more: true) } }.disabled(!ready || model.busy) }
            if let draft = model.draft {
                NativeViewportArea(model: model, ready: ready, editable: !model.immutable, controller: model.viewportController)
                ArtworkCatalogView(model: model, controller: model.viewportController, ready: ready)
                if model.immutable { Text("공개용 저장본은 보관됩니다. 변경하려면 사본을 만드세요."); Button("복제해서 편집") { Task { await model.copyForEditing() } }.disabled(!ready || model.busy || model.pending) }
                HStack {
                    Button("되돌리기") { Task { await model.undo() } }.disabled(!model.canUndo || !ready)
                    Button("다시 실행") { Task { await model.redo() } }.disabled(!model.canRedo || !ready)
                    Spacer(minLength: 0)
                    Button(inspectorPresented.wrappedValue ? "선택 속성 접기" : "선택 속성", systemImage: "sidebar.right") { inspectorVisible.toggle() }
                        .disabled(!hasSelectedPlacement && model.selectedLightId.isEmpty && model.selectedBenchId.isEmpty && model.selectedPlaqueId.isEmpty)
                        .help("선택한 작품 배치·조명·벤치·명판의 속성을 오른쪽에 표시하거나 접습니다. 입력은 유지됩니다.")
                }
                CoverEntryPreviewControls(model: model, viewport: model.viewportController, ready: ready, mode: mode)
                Form {
                    Section("전시와 공간") {
                        TextField("전시 제목", text: $model.title)
                        Picker("공간", selection: $model.roomId) {
                            ForEach(draft.rooms.indices, id: \.self) { index in Text(draft.rooms[index]["name"].string ?? "공간").tag(draft.rooms[index]["id"].string ?? "") }
                        }.disabled(model.formDirty || model.architectureDirty || model.architectureApplying)
                    }
                    CoverEntrySettingsView(model: model, mode: mode)
                    Section("승인한 작품 배치") {
                        if !model.artworkId.isEmpty {
                            Text("배치할 작품: " + (model.library.first(where: { $0.id == model.artworkId })?.title ?? "작품"))
                            TextField("위치 X (m)", text: $model.positionXText)
                            TextField("위치 Y (m)", text: $model.positionYText)
                            TextField("위치 Z (m)", text: $model.positionZText)
                            if model.positionDirty { Button("위치 입력 취소") { model.cancelPositionInput() } }
                            Button("선택한 작품 배치") { Task { await model.addArtwork() } }
                        } else {
                            Text("작품 목록이나 공간에서 배치를 고르세요. 위치와 설치 설정은 ‘선택 속성’에서 다듬습니다.").foregroundStyle(.secondary)
                        }
                        Picker("배치한 작품", selection: $model.placementId) {
                            Text("배치 선택").tag("")
                            ForEach(draft.placements.indices, id: \.self) { index in
                                let placement = draft.placements[index]
                                Text("배치 \(index + 1) · " + (draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] })?["metadata"]["title"].string ?? "작품")).tag(placement["id"].string ?? "")
                            }
                        }
                        .disabled(model.positionDirty || model.formDirty)
                        Text("공간 \(draft.rooms.count)개 · 작품 배치 \(draft.placements.count)개").foregroundStyle(.secondary)
                    }
                }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true).disabled(!ready || model.entrancePreviewActive || model.busy || model.pending || model.immutable || model.architectureDirty || model.architectureApplying || model.placingArtwork)
                Form { SpaceLightingSettingsView(model: model, ready: ready, mode: mode) }
                    .formStyle(.grouped).fixedSize(horizontal: false, vertical: true)
                ArchitectureSettingsView(model: model, ready: ready)
                HStack {
                    Button("입력 이 Mac에 보관") { Task { await model.keepLocal() } }.disabled(model.pending || model.immutable)
                    Button(model.pending ? "저장 응답 다시 확인" : "전시 저장") { Task { await model.save() } }.buttonStyle(.borderedProminent).disabled(model.immutable && !model.pending)
                    Button("저장본 비교") { Task { await model.compare() } }.disabled(model.state?.etag == nil)
                }.disabled(!ready || model.busy)
                NativeOEXExportControls(model: model, ready: ready)
                if model.pending { Text("같은 저장 요청을 보관했습니다. 응답 확인 전에는 다른 전시로 전환하지 않습니다.").foregroundStyle(.secondary) }
                if let comparison = model.comparison {
                    Text("비교한 저장본: \(comparison.draft.title) · 저장본 \(comparison.revision) · 작품 배치 \(comparison.draft.placements.count)개")
                    Button("비교한 저장본 열기") { pendingAdopt = true; pendingNew = false; pendingOpen = nil; discardPrompt = true }.disabled(!ready || model.busy || model.pending)
                }
            } else { Text("새 전시를 만들거나 저장한 전시를 여세요.").foregroundStyle(.secondary) }
            if model.busy && !model.exporting { ProgressView("전시를 확인하고 있습니다…") }
            if let failure = model.failure { Text(failure.localizedDescription); DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) } }
            if !model.notice.isEmpty { Text(model.notice).foregroundStyle(.secondary) }
            Button("작업 공간 다시 연결") { Task { await connect() } }.disabled(!ready || model.busy)
        }
        .inspector(isPresented: inspectorPresented) {
            if !model.selectedPlaqueId.isEmpty {
                SelectedPlaqueInspector(model: model, controller: model.viewportController, ready: ready, mode: mode)
                    .id(model.selectedPlaqueId)
            } else if !model.selectedBenchId.isEmpty {
                SelectedBenchInspector(model: model, controller: model.viewportController, ready: ready, mode: mode)
                    .id(model.selectedBenchId)
            } else if !model.selectedLightId.isEmpty {
                SelectedLightInspector(model: model, controller: model.viewportController, ready: ready, mode: mode)
                    .id(model.selectedLightId)
            } else {
                SelectedPlacementInspector(model: model, ready: ready, mode: mode, inputs: selectedInputs)
                    .id(model.placementId)
            }
        }
        .onChange(of: model.selectedPlaqueId) { _, id in if !id.isEmpty { inspectorVisible = true } }
        .onChange(of: model.selectedBenchId) { _, id in if !id.isEmpty { inspectorVisible = true } }
        .onChange(of: model.selectedLightId) { _, id in if !id.isEmpty { inspectorVisible = true } }
        .task(id: ready) { if ready { await connect() } }
        .onChange(of: model.roomId) { _, _ in model.selectRoom() }
        .onChange(of: model.placementId) { _, _ in model.selectPlacement() }
        .confirmationDialog("현재 전시 입력을 바꿀까요?", isPresented: $discardPrompt) {
            Button("입력을 바꾸고 열기", role: .destructive) { Task { if pendingAdopt { await model.adoptComparison() } else if pendingNew { await model.new() } else if let id = pendingOpen { await model.open(id) } } }
            Button("계속 편집", role: .cancel) {}
        } message: { Text("저장본에 반영하지 않은 현재 입력이 바뀝니다. 필요한 입력은 먼저 이 Mac에 보관하거나 전시를 저장하세요.") }
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
            if model.exporting { ProgressView("작품의 권리와 파일을 확인하고 OEX를 준비하고 있습니다…") }
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
