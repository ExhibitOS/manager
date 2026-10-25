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
        }
    }
    var canSelectCatalog: Bool { !busy && !pending && !formDirty && !positionDirty && !architectureDirty && !architectureApplying }
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
    @Published var title = ""
    @Published var entrance = ViewerEntranceSettings()
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
    var viewportDraft: NativeExhibitionDraft? { surfaceMaterialPreview?.displayDraft ?? (installationDirty ? (installationPreview ?? draft) : draft) }
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
    func requestViewportView(_ view: String) { viewportView = view; viewportCameraSequence += 1 }
    @Published var viewportSelection: ViewportSelection?
    @Published var architecture: ArchitectureFormBuffer?
    private var architectureBaseline: ArchitectureFormBuffer?
    @Published private(set) var architectureStatus = ""
    @Published private(set) var architectureApplying = false
    var architectureGeometryDirty: Bool { architecture != architectureBaseline }
    var architectureDirty: Bool { architectureGeometryDirty || surfaceMaterialDirty }
    @Published var surfaceMaterial: SurfaceMaterialFormBuffer? { didSet { surfaceMaterialPreview = nil; surfaceMaterialStatus = "" } }
    private var surfaceMaterialBaseline: SurfaceMaterialFormBuffer?
    @Published private var surfaceMaterialPreview: SurfaceMaterialPreview?
    private var surfaceMaterialPreviewAccessID: UUID?
    @Published private(set) var surfaceMaterialStatus = ""
    var surfaceMaterialDirty: Bool { surfaceMaterial != surfaceMaterialBaseline }
    var surfaceMaterialPreviewing: Bool { surfaceMaterialPreview != nil }
    var canPreviewSurfaceMaterial: Bool { !busy && !pending && !immutable && !formDirty && !positionDirty && !architectureGeometryDirty && !architectureApplying && surfaceMaterialDirty && !surfaceMaterialPreviewing && viewportController.connected }
    var canApplySurfaceMaterial: Bool {
        guard !busy, !pending, !immutable, !formDirty, !positionDirty, !architectureGeometryDirty, !architectureApplying,
              viewportController.connected, surfaceMaterialPreviewAccessID == viewportAccessID,
              let preview = surfaceMaterialPreview, preview.base == draft,
              preview.command.selection == viewportSelection, let fields = surfaceMaterial else { return false }
        return (try? fields.command()) == preview.command
    }
    var canApplyArchitecture: Bool { !busy && !pending && !immutable && !formDirty && !positionDirty && !architectureApplying && architectureGeometryDirty && !surfaceMaterialDirty && viewportController.connected }
    @Published private(set) var viewportAccessID = UUID()
    private(set) var viewportCredentials: LocalAuthoringSession?
    @Published private var past: [NativeExhibitionDraft] = []
    @Published private var future: [NativeExhibitionDraft] = []
    var canUndo: Bool { !immutable && !past.isEmpty && !pending && !busy && !formDirty && !positionDirty && !architectureDirty && !architectureApplying }
    var canRedo: Bool { !immutable && !future.isEmpty && !pending && !busy && !formDirty && !positionDirty && !architectureDirty && !architectureApplying }
    private var api: AuthoringAPI?
    private var session: AuthoringSession?
    private var read: (() async throws -> NativeDocumentState)?
    private var write: ((NativeDocumentState) async throws -> NativeDocumentState)?
    var draft: NativeExhibitionDraft? { state?.draft }
    var pending: Bool { state?.pending != nil }
    var formDirty: Bool {
        guard let draft else { return false }
        let entranceDirty = (entranceEditable && (try? draft.viewerEntranceSettings()) != entrance) || installationDirty
        guard let room = draft.rooms.first(where: { $0["id"].string == roomId }) else { return title != draft.title || entranceDirty }
        return title != draft.title || entranceDirty || roomName != room["name"].string || width != room["dimensions"]["width"].double || height != room["dimensions"]["height"].double || depth != room["dimensions"]["depth"].double
    }
    var positionDirty: Bool {
        guard [x, y, z].allSatisfy({ $0.isFinite }) else { return true }
        if let p = draft?.placements.first(where: { $0["id"].string == placementId }), let position = p["transform"]["position"].items, position.count == 3 { return x != position[0].double || y != position[1].double || z != position[2].double }
        return !artworkId.isEmpty
    }
    var canExport: Bool { !busy && !dirty && state?.etag != nil && draft != nil }
    var dirty: Bool { state?.dirty == true || formDirty || positionDirty || architectureDirty || architectureApplying }
    func connect(api: AuthoringAPI, session: AuthoringSession, credentials: LocalAuthoringSession, read: @escaping () async throws -> NativeDocumentState, write: @escaping (NativeDocumentState) async throws -> NativeDocumentState) async {
        guard !busy else { return }
        if let old = self.session, old.tenantId != session.tenantId || old.userId != session.userId {
            guard !dirty else { self.api = nil; viewportCredentials = nil; failure = AuthoringFailure("DRAFT_ACCOUNT_MISMATCH"); return }
            state = nil; items = []; library = []; comparison = nil; exportedURL = nil; exportedSHA256 = nil; catalogArtworkId = ""; catalogStatus = ""
        }
        self.api = api; self.session = session
        let accessChanged = viewportCredentials?.token != credentials.token || viewportCredentials?.origin != credentials.origin
        viewportCredentials = credentials
        if accessChanged { viewportAccessID = UUID(); surfaceMaterialPreview = nil; catalogTask?.cancel(); catalogEntries = [:]; catalogFailures = []; catalogImages = [:]; catalogImageOrder = []; catalogPreviewFailures = [] }
        self.read = read; self.write = write
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
    private func work(_ body: () async throws -> Void) async {
        guard !busy, api != nil, session != nil else { return }
        busy = true; failure = nil; notice = ""
        defer { busy = false }
        do { try await body() }
        catch { failure = error as? AuthoringFailure ?? AuthoringFailure((error as? BridgeFailure)?.code ?? "CONNECTION_FAILED") }
    }
    private func persist(_ value: NativeDocumentState, recordUndo: Bool = true) async throws {
        guard let write else { throw AuthoringFailure("EXHIBITION_STORE_REQUIRED") }
        let old = state?.draft
        state = try await write(value)
        if old?.id != value.draft?.id { past = []; future = []; viewportSelection = nil; exportedURL = nil; exportedSHA256 = nil }
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
            guard !immutable, !pending, !formDirty, !positionDirty, !surfaceMaterialDirty, architectureApplying,
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
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureGeometryDirty, architectureApplying,
                  draft == base, try surfaceMaterial?.command() == command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            surfaceMaterialPreview = try SurfaceMaterialPreview(candidate: candidate, base: base, command: command, selected: viewportSelection)
            surfaceMaterialPreviewAccessID = viewportAccessID
            surfaceMaterialStatus = "재질을 미리 보고 있습니다. 적용하면 이 Mac에 보관되며 취소하면 원래 표시로 돌아갑니다."
        }
    }
    func applySurfaceMaterial() async {
        guard canApplySurfaceMaterial else { return }
        await work {
            guard !immutable, !pending, !formDirty, !positionDirty, !architectureGeometryDirty, !architectureApplying,
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
        installationPreview = nil
        do {
            let settings = try draft.flatMap { value in placementId.isEmpty ? nil : try value.installationSettings(for: placementId) }
            installationEditable = !placementId.isEmpty
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
            guard !immutable, !pending, installationEditable, !placementId.isEmpty, var value = state, var next = draft else { throw AuthoringFailure("EXHIBITION_PENDING") }
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
        guard !busy, !pending, !formDirty, !positionDirty, !architectureDirty, !architectureApplying, let draft, selection.belongs(to: draft) else { return }
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
        if entranceEditable, try draft.viewerEntranceSettings() != entrance { try draft.setViewerEntrance(entrance) }
        if installationDirty, installationEditable { try draft.setInstallation(installation.value(), for: placementId) }
        if let room = draft.rooms.first(where: { $0["id"].string == roomId }), roomName != room["name"].string || width != room["dimensions"]["width"].double || height != room["dimensions"]["height"].double || depth != room["dimensions"]["depth"].double {
            try draft.updateRoom(roomId, name: roomName, width: width, height: height, depth: depth)
        }
        return draft
    }
    func keepLocal() async {
        await work {
            guard !immutable, !pending, var value = state else { throw AuthoringFailure(immutable ? "PUBLISHED_DRAFT_IMMUTABLE" : "EXHIBITION_PENDING") }
            value.draft = try appliedFields(); try await persist(value); syncInstallation()
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
            value.draft = next; try await persist(value); syncInstallation()
            notice = "작품 위치를 이 Mac에 보관했습니다."
        }
    }
    func syncFields() {
        let selected = viewportSelection
        title = draft?.title ?? ""
        do {
            entrance = try draft?.viewerEntranceSettings() ?? ViewerEntranceSettings()
            entranceEditable = true
        } catch {
            entrance = ViewerEntranceSettings()
            entranceEditable = false
            failure = error as? AuthoringFailure ?? AuthoringFailure("VIEWER_ENTRANCE_INVALID")
        }
        if draft?.rooms.contains(where: { $0["id"].string == roomId }) != true { roomId = draft?.rooms.first?["id"].string ?? "" }
        selectRoom()
        if draft?.placements.contains(where: { $0["id"].string == placementId }) != true { placementId = draft?.placements.first?["id"].string ?? "" }
        selectPlacement()
        if let selected, let draft, selected.belongs(to: draft) { viewportSelection = selected; if selected.kind != "placement" { catalogArtworkId = "" } }
        syncArchitecture()
    }
    func selectRoom() {
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
    func selectPlacement() {
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
    let connect: () async -> Void
    @State private var discardPrompt = false
    @State private var pendingOpen: String?
    @State private var pendingNew = false
    @State private var pendingAdopt = false
    private func installationSurfaces(kind: String) -> [(id: String, name: String)] {
        guard let draft = model.draft,
              let room = draft.placements.first(where: { $0["id"].string == model.placementId })?["roomId"].string else { return [] }
        return (draft.candidate["surfaces"].items ?? []).filter { $0["roomId"].string == room && $0["type"].string == kind }
            .enumerated().compactMap { index, surface in
                guard let id = surface["id"].string else { return nil }
                return (id, (kind == "floor" ? "바닥 " : "벽 ") + String(index + 1))
            }
    }
    private func requestOpen(_ id: String?) {
        guard !model.pending else { return }
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
                }
                Form {
                    Section("전시와 공간") {
                        TextField("전시 제목", text: $model.title)
                        Picker("공간", selection: $model.roomId) {
                            ForEach(draft.rooms.indices, id: \.self) { index in Text(draft.rooms[index]["name"].string ?? "공간").tag(draft.rooms[index]["id"].string ?? "") }
                        }.disabled(model.formDirty || model.architectureDirty || model.architectureApplying)
                    }
                    Section("관람 시작 화면") {
                        Text("관람객이 전시에 들어가기 전에 전시 제목과 이 소개를 봅니다.").foregroundStyle(.secondary)
                        TextField("작가", text: $model.entrance.artist)
                        TextField("전시 소개", text: $model.entrance.introduction, axis: .vertical).lineLimit(3...8)
                        DisclosureGroup("입장권 글과 연출") {
                            TextField("입장권 제목", text: $model.entrance.ticketTitle)
                            TextField("입장권 안내", text: $model.entrance.ticketDetail, axis: .vertical).lineLimit(2...4)
                            Text("비워 두면 전시 제목과 작가를 표시합니다.").foregroundStyle(.secondary)
                            Toggle("입장권 움직임", isOn: $model.entrance.ticketAnimation)
                            Toggle("표지 펼치기", isOn: $model.entrance.coverUnfold)
                            Toggle("문 열기 연출", isOn: $model.entrance.doorEffect)
                            Text("각 연출은 따로 켜거나 끌 수 있습니다. 관람자의 동작 줄이기 설정이 우선합니다.").foregroundStyle(.secondary)
                        }
                        Picker("표지 이미지", selection: Binding<String?>(get: { model.entrance.imageAssetId?.lowercased() }, set: { model.entrance.imageAssetId = $0 })) {
                            Text("제목과 소개만 표시").tag(nil as String?)
                            ForEach(draft.viewerEntranceImageChoices()) { image in
                                Text(image.title).tag(Optional(image.id.lowercased()))
                            }
                        }
                        if model.entrance.imageAssetId != nil {
                            TextField("이미지 설명", text: $model.entrance.imageAlt, axis: .vertical).lineLimit(2...4)
                            Picker("모바일 이미지 위치", selection: $model.entrance.mobilePosition) {
                                Text("가운데").tag(ViewerEntranceSettings.MobilePosition.center)
                                Text("위쪽").tag(ViewerEntranceSettings.MobilePosition.top)
                                Text("아래쪽").tag(ViewerEntranceSettings.MobilePosition.bottom)
                            }
                            Text("작품 목록의 PNG 이미지를 사용합니다. 관람객이 이미지를 이해할 수 있도록 설명을 입력하세요.").foregroundStyle(.secondary)
                        }
                        if !model.entranceEditable { Text("이 시작 화면의 설정을 읽을 수 없습니다. 원래 설정은 보관됩니다.").foregroundStyle(.secondary) }
                    }.disabled(!model.entranceEditable)
                    Section("승인한 작품 배치") {
                        Text(model.artworkId.isEmpty ? "카탈로그에서 원본 작품을 고른 뒤 배치를 선택하세요." : "배치할 작품: " + (model.library.first(where: { $0.id == model.artworkId })?.title ?? "작품"))
                        TextField("위치 X (m)", text: $model.positionXText)
                        TextField("위치 Y (m)", text: $model.positionYText)
                        TextField("위치 Z (m)", text: $model.positionZText)
                        if model.positionDirty { Button("위치 입력 취소") { model.cancelPositionInput() } }
                        Button("선택한 작품 배치") { Task { await model.addArtwork() } }.disabled(model.artworkId.isEmpty)
                        Picker("배치한 작품", selection: $model.placementId) {
                            Text("배치 선택").tag("")
                            ForEach(draft.placements.indices, id: \.self) { index in
                                let placement = draft.placements[index]
                                Text("배치 \(index + 1) · " + (draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] })?["metadata"]["title"].string ?? "작품")).tag(placement["id"].string ?? "")
                            }
                        }
                        .disabled(model.positionDirty || model.formDirty)
                        Button("선택한 배치 위치 적용") { Task { await model.movePlacement() } }.disabled(model.placementId.isEmpty)
                        Text("공간 \(draft.rooms.count)개 · 작품 배치 \(draft.placements.count)개").foregroundStyle(.secondary)
                    }
                }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true).disabled(!ready || model.busy || model.pending || model.immutable || model.architectureDirty || model.architectureApplying)
                ArchitectureSettingsView(model: model, ready: ready)
                if !model.placementId.isEmpty {
                    InstallationSettingsView(buffer: $model.installation, placementId: model.placementId, dirty: model.installationDirty,
                        editable: ready && !model.busy && !model.pending && !model.immutable && model.installationEditable && !model.architectureDirty && !model.architectureApplying,
                        viewport: model.viewportController, canAlign: !model.formDirty && !model.positionDirty,
                        status: model.installationStatus, floors: installationSurfaces(kind: "floor"), walls: installationSurfaces(kind: "wall"),
                        apply: { Task { await model.applyInstallation() } }, discard: { model.discardInstallation() }, reset: { model.resetInstallation() },
                        align: { command in Task { await model.viewportController.perform(.installation(command), model: model) } })
                        .id(model.placementId)
                }
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
