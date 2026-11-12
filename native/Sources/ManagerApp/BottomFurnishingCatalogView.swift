// SPDX-License-Identifier: Apache-2.0
import AppKit
import ManagerCore
import SwiftUI

enum BottomFurnishingCatalogRow: Identifiable, Sendable {
    case workspace(WorkspaceFurnishingSummary)
    case bundled(BundledWorkspaceFurnishings.Item)
    case shared(catalogId: String, name: String, host: SharedFurnishingCommand.Host)

    var id: String {
        switch self {
        case .workspace(let item): "workspace:\(item.id)"
        case .bundled(let item): "bundled:\(item.id)"
        case .shared(let catalogId, _, _): "shared:\(catalogId)"
        }
    }

    var name: String {
        switch self {
        case .workspace(let item): item.name
        case .bundled(let item): item.name
        case .shared(_, let name, _): name
        }
    }

    fileprivate var category: FurnishingDrawerCategory {
        let kind: WorkspaceFurnishingKind?
        switch self {
        case .workspace(let item): kind = item.kind
        case .bundled(let item): kind = item.kind
        case .shared(_, _, let host):
            switch host {
            case .bench: return .seating
            case .light: return .lighting
            case .opening, .pedestal: return .other
            }
        }
        switch kind {
        case .some(.artworkFrame): return .frames
        case .some(.bench), .some(.stool): return .seating
        default: return .other
        }
    }

    fileprivate var sourceLabel: String {
        switch self {
        case .workspace(let item): item.rightsAvailability.display ? "작업 공간 · v\(item.version)" : "전시 권리 없음"
        case .bundled: "기본 비품 · 등록 필요"
        case .shared: "기본 비품 · 대상에 연결"
        }
    }

    fileprivate var selectable: Bool {
        if case .workspace(let item) = self { return item.rightsAvailability.display }
        return true
    }
}

fileprivate enum FurnishingDrawerCategory: CaseIterable, Hashable {
    case all, frames, seating, lighting, other

    var title: String {
        switch self {
        case .all: "전체"
        case .frames: "액자"
        case .seating: "좌석"
        case .lighting: "조명"
        case .other: "기타"
        }
    }

    var symbol: String {
        switch self {
        case .all, .other: "cube"
        case .frames: "photo.artframe"
        case .seating: "chair"
        case .lighting: "lightbulb"
        }
    }

    var intakeKind: WorkspaceFurnishingKind? {
        switch self {
        case .frames: .artworkFrame
        case .seating: .bench
        case .all, .lighting, .other: nil
        }
    }
}

@MainActor
struct BottomFurnishingCatalogView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject private var controller: NativeViewportController
    let ready: Bool
    let registeredFurnishing: ExhibitionModel.RegisteredFurnishing?
    let onSelect: (BottomFurnishingCatalogRow) -> Void
    let onAdd: (WorkspaceFurnishingKind?) -> Void
    let catalogDrag: CatalogDragSession?

    @State private var category: FurnishingDrawerCategory = .all
    @State private var query = ""
    @State private var selectedID: String?
    @State private var thumbnailReview: FurnishingThumbnailReviewModel?
    @State private var bundled: [BundledWorkspaceFurnishings.Item] = []
    @State private var workspace: [WorkspaceFurnishingSummary] = []
    @State private var cursor: String?
    @State private var loadedScope: LoadScope?
    @State private var loading = false
    @State private var status: String?

    private struct LoadScope: Equatable {
        let access: UUID
        let available: Bool
        let reveal: String?
    }

    init(model: ExhibitionModel, ready: Bool, registeredFurnishing: ExhibitionModel.RegisteredFurnishing?,
         onSelect: @escaping (BottomFurnishingCatalogRow) -> Void,
         onAdd: @escaping (WorkspaceFurnishingKind?) -> Void, catalogDrag: CatalogDragSession? = nil) {
        self.model = model
        self.controller = model.viewportController
        self.ready = ready
        self.registeredFurnishing = registeredFurnishing
        self.onSelect = onSelect
        self.onAdd = onAdd
        self.catalogDrag = catalogDrag
    }

    private var loadScope: LoadScope {
        .init(access: model.viewportAccessID, available: ready && model.workspaceFurnishingIntakeAvailable,
              reveal: currentRegistration?.item.id)
    }

    private var currentRegistration: ExhibitionModel.RegisteredFurnishing? {
        guard registeredFurnishing?.access == model.viewportAccessID else { return nil }
        return registeredFurnishing
    }

    private struct ThumbnailWork: Equatable {
        let access: UUID
        let furnishingIDs: [String]
        let sequence: Int
    }

    private var thumbnailWork: ThumbnailWork? {
        guard workspaceVisible,
              (model.durableFurnishingThumbnailsAvailable || (model.furnishingThumbnailFrameReady && controller.furnishingThumbnailSupported)) else { return nil }
        var ids = visibleRows.compactMap { row -> String? in
            guard case .workspace(let item) = row, item.rightsAvailability.display else { return nil }; return item.id
        }
        if let recent = currentRegistration?.item.id, let index = ids.firstIndex(of: recent) {
            ids.remove(at: index); ids.insert(recent, at: 0)
        }
        guard !ids.isEmpty else { return nil }
        // One existing renderer at a time, with the same sixteen-entry cache
        // bound; search/category changes cancel this work and choose new cards.
        return .init(access: model.viewportAccessID, furnishingIDs: Array(ids.prefix(16)), sequence: controller.acceptedSequence)
    }

    private var workspaceVisible: Bool { loadScope.available && loadedScope?.access == loadScope.access }
    private var sharedAvailable: Bool { ready && controller.connected && controller.sharedFurnishingVisualsSupported }
    private var canSelect: Bool { ready && controller.editingContextActive && model.canSelectCatalog && !model.immutable }
    private var canAdd: Bool {
        canSelect && loadScope.available
            && (category.intakeKind.map { WorkspaceFurnishingIntakeView.supports($0, model: model) } ?? true)
    }

    private var visibleRows: [BottomFurnishingCatalogRow] {
        let bundledRows = bundled.filter { item in
            item.kind.map { WorkspaceFurnishingIntakeView.supports($0, model: model) } ?? false
        }.map(BottomFurnishingCatalogRow.bundled)
        let sharedRows: [BottomFurnishingCatalogRow] = sharedAvailable ? model.sharedFurnishingCatalog.compactMap { item in
            guard let id = item["catalogId"].string, let name = item["name"].string,
                  let hostName = item["hostKinds"].items?.first?.string,
                  let host = SharedFurnishingCommand.Host(rawValue: hostName) else { return nil }
            return .shared(catalogId: id, name: name, host: host)
        } : []
        let rows = (workspaceVisible ? workspace.map(BottomFurnishingCatalogRow.workspace) : []) + bundledRows + sharedRows
        let search = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return rows.filter { row in
            (category == .all || row.category == category) &&
                (search.isEmpty || row.name.localizedCaseInsensitiveContains(search))
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text("비품 카탈로그").font(.headline)
                TextField("비품 이름 검색", text: $query).textFieldStyle(.roundedBorder)
                Button {
                    Task {
                        await load(loadScope, reveal: nil)
                        if sharedAvailable { await model.retrySharedFurnishingCatalog() }
                    }
                } label: { Label("비품 목록 새로 고침", systemImage: "arrow.clockwise").labelStyle(.iconOnly) }
                    .disabled(!ready || loading || model.busy || (!loadScope.available && !sharedAvailable))
                Button("비품 추가", systemImage: "plus") { onAdd(category.intakeKind) }
                    .disabled(!canAdd)
                    .help(category == .lighting ? "기본 조명은 선택한 조명에 연결할 수 있습니다." : "작업 공간에 비품 등록")
            }
            Picker("비품 종류", selection: $category) {
                ForEach(FurnishingDrawerCategory.allCases, id: \.self) { Text($0.title).tag($0) }
            }.pickerStyle(.segmented).labelsHidden()
            if visibleRows.isEmpty && !loading {
                Text(workspaceVisible && cursor != nil ? "불러온 비품에서 찾지 못했습니다." : "이 조건에 맞는 비품이 없습니다.")
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                ScrollViewReader { proxy in
                    ScrollView(.horizontal) {
                        LazyHStack(alignment: .top, spacing: 8) {
                            ForEach(visibleRows) { row in
                                HStack(spacing: 2) {
                                    draggableCard(row)
                                    if case .workspace(let item) = row,
                                       case .failed = model.furnishingThumbnail(for: item) {
                                        Button("미리보기 다시 만들기", systemImage: "arrow.clockwise") {
                                            guard thumbnailWork != nil else { return }
                                            Task { await model.loadFurnishingThumbnail(item, retry: true) }
                                        }
                                            .labelStyle(.iconOnly)
                                            .disabled(thumbnailWork == nil)
                                    }
                                }.id(row.id)
                            }
                        }.padding(2)
                    }.frame(height: 62)
                    .onChange(of: selectedID, initial: true) { _, id in
                        if let id { proxy.scrollTo(id, anchor: .center) }
                    }
                }
            }
            if loading || status != nil || (workspaceVisible && cursor != nil) {
                HStack {
                    if loading { ProgressView().controlSize(.small).accessibilityLabel("비품 목록 확인 중") }
                    if let status { Text(status).font(.caption).foregroundStyle(.secondary) }
                    Spacer(minLength: 0)
                    if workspaceVisible, let cursor {
                        Button("비품 더 보기") { Task { await load(loadScope, cursor: cursor, reveal: nil) } }
                            .disabled(loading || model.busy)
                    }
                }
            }
        }
        .sheet(item: $thumbnailReview) { FurnishingThumbnailReviewView(model: $0) }
        .controlSize(.small)
        .padding(8)
        .background(Color(nsColor: .controlBackgroundColor))
        .task {
            if let catalog = try? await Task.detached(priority: .userInitiated, operation: { try BundledWorkspaceFurnishings.catalog() }).value,
               !Task.isCancelled { bundled = catalog.items }
        }
        .task(id: sharedAvailable ? model.viewportAccessID : nil) { if sharedAvailable { await model.loadSharedFurnishingCatalog() } }
        .task(id: loadScope) { await loadIfNeeded() }
        .task(id: thumbnailWork) {
            guard let work = thumbnailWork else { return }
            for id in work.furnishingIDs {
                guard !Task.isCancelled, work == thumbnailWork,
                      let item = workspace.first(where: { $0.id == id }) else { return }
                await model.loadFurnishingThumbnail(item, retry: false)
            }
        }
        .onChange(of: model.busy || loading) { _, blocked in if !blocked { Task { await loadIfNeeded() } } }
    }

    @ViewBuilder private func draggableCard(_ row: BottomFurnishingCatalogRow) -> some View {
        if let catalogDrag, let item = CatalogDragItem(furnishing: row), controller.catalogDropSupported,
           canSelect, row.selectable, model.viewportPresentation == .threeD {
            card(row).onDrag { catalogDrag.provider(for: item, model: model) }
                .onAppear { CatalogDropTrace.record("furnishingCardDraggable") }
        } else {
            card(row)
                .onAppear { CatalogDropTrace.record("furnishingCardUnavailable", "session=\(catalogDrag != nil) item=\(CatalogDragItem(furnishing: row) != nil) capability=\(controller.catalogDropSupported) canSelect=\(canSelect) selectable=\(row.selectable) threeD=\(model.viewportPresentation == .threeD)") }
        }
    }

    private func card(_ row: BottomFurnishingCatalogRow) -> some View {
        let selected = selectedID == row.id
        return Button {
            guard canSelect, row.selectable else { return }
            if case .workspace = row, !workspaceVisible { return }
            if case .shared = row, !sharedAvailable { return }
            selectedID = row.id
            onSelect(row)
        } label: {
            HStack(spacing: 5) {
                preview(row)
                VStack(alignment: .leading, spacing: 3) {
                    Text(row.name).font(.caption.weight(.semibold)).lineLimit(1)
                    Text(row.sourceLabel).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                    if let status = previewStatus(row) {
                        Text(status).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
            }
            .padding(7).frame(width: 145, alignment: .leading)
            .background(selected ? Color.accentColor.opacity(0.10) : Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(selected ? Color.accentColor : Color(nsColor: .separatorColor)))
        }
        .buttonStyle(.plain)
        .contextMenu {
            if case .workspace(let item) = row, model.durableFurnishingThumbnailsAvailable {
                Button("대표 이미지 확인·선택", systemImage: "photo") {
                    thumbnailReview = model.makeFurnishingThumbnailReview(item)
                }
            }
        }
        .disabled(!canSelect || !row.selectable)
        .accessibilityLabel("\(row.name), \(row.sourceLabel)")
        .accessibilityValue(previewStatus(row) ?? "")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    @ViewBuilder private func preview(_ row: BottomFurnishingCatalogRow) -> some View {
        if case .workspace(let item) = row, case .ready(let image, let expires) = model.furnishingThumbnail(for: item) {
            if let expires {
                TimelineView(.explicit([.now, expires])) { context in
                    if context.date < expires { Image(nsImage: image).resizable().scaledToFit() }
                    else { Image(systemName: row.category.symbol) }
                }.frame(width: 32, height: 32).accessibilityHidden(true)
            } else {
                Image(nsImage: image).resizable().scaledToFit().frame(width: 32, height: 32).accessibilityHidden(true)
            }
        } else {
            Image(systemName: row.category.symbol).frame(width: 24, height: 32).accessibilityHidden(true)
        }
    }

    private func previewStatus(_ row: BottomFurnishingCatalogRow) -> String? {
        guard case .workspace(let item) = row, item.rightsAvailability.display else { return nil }
        switch model.furnishingThumbnail(for: item) {
        case .ready: return nil
        case .failed: return "미리보기 실패"
        case .rendering: return "미리보기 만드는 중"
        case nil:
            return model.furnishingThumbnailFrameReady && !controller.furnishingThumbnailSupported
                ? "미리보기 지원 안 함" : "미리보기 대기"
        }
    }

    private func loadIfNeeded() async {
        let scope = loadScope
        guard scope.available else { loadedScope = nil; status = nil; return }
        guard scope != loadedScope else { return }
        if scope.access != loadedScope?.access { selectedID = nil; workspace = []; cursor = nil }
        await load(scope, reveal: scope.reveal)
    }

    private func load(_ scope: LoadScope, cursor: String? = nil, reveal: String?) async {
        guard scope.available, scope.access == model.viewportAccessID, !loading, !model.busy else { return }
        loadedScope = scope
        loading = true
        status = nil
        defer { loading = false }
        do {
            let page = try await model.withWorkspaceFurnishingAccess(scope.access) { api in
                try await loadWorkspaceFurnishingCatalog(using: api, cursor: cursor, revealing: reveal)
            }
            try Task.checkCancellation()
            if cursor == nil { workspace = page.items }
            else {
                let existing = Set(workspace.map(\.id))
                workspace += page.items.filter { !existing.contains($0.id) }
            }
            self.cursor = page.nextCursor == cursor ? nil : page.nextCursor
            if let reveal {
                if let item = workspace.first(where: { $0.id == reveal }) {
                    category = .all
                    query = ""
                    selectedID = BottomFurnishingCatalogRow.workspace(item).id
                } else {
                    status = "등록한 비품을 목록에서 찾지 못했습니다."
                }
            }
        } catch {
            guard scope.access == model.viewportAccessID else { return }
            if Task.isCancelled { loadedScope = nil }
            else { status = "비품 목록을 확인하지 못했습니다." }
        }
    }
}
