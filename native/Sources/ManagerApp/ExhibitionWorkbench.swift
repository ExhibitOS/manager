// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

/// Presentation routes share one model and one mounted viewport. Switching a
/// purpose only replaces its inspector, never the document or rendering host.
struct NativeExhibitionView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let ready: Bool
    let mode: Mode
    let connect: () async -> Void
    var active = true
    var readOnly = false
    var connectionAllowed = true
    var edit: () -> Void = {}
    var publish: () -> Void = {}
    @Environment(\.accessibilityReduceMotion) private var reducedMotion
    @State private var discardPrompt = false
    @State private var pendingOpen: String?
    @State private var pendingNew = false
    @State private var pendingAdopt = false
    @State private var browserVisible = false
    @State private var inspectorVisible = false
    @State private var scope = InspectorScope.selection
    @State private var coverPurpose = false
    @State private var returningToScene = false
    @State private var needsConnection = true
    @State private var inspectorInputs: [String: InstallationInspectorState] = [:]

    private enum InspectorScope: String, CaseIterable, Identifiable {
        case selection, lighting, floor, cover, exhibition, output
        var id: Self { self }
        var title: String {
            switch self {
            case .selection: "선택 속성"
            case .lighting: "전시 조명"
            case .floor: "바닥 재질"
            case .cover: "표지·입장"
            case .exhibition: "전시 정보"
            case .output: "저장·내보내기"
            }
        }
    }
    private var inspectorPresented: Binding<Bool> {
        Binding(get: { active && inspectorVisible && model.draft != nil }, set: { inspectorVisible = $0 })
    }
    private var hasSelectedPlacement: Bool {
        guard let selected = model.viewportSelection, selected.kind == "placement", selected.id == model.placementId,
              let draft = model.draft else { return false }
        return selected.belongs(to: draft)
    }
    private var selectedInputs: Binding<InstallationInspectorState> {
        let key = [model.state?.tenantId ?? "", model.state?.principalId ?? "", model.draft?.id ?? "", model.placementId].joined(separator: ":")
        return Binding(get: { inspectorInputs[key] ?? InstallationInspectorState() }, set: { inspectorInputs[key] = $0 })
    }
    private var formLocked: Bool {
        !ready || readOnly || model.entrancePreviewActive || model.busy || model.pending || model.immutable || model.architectureDirty || model.architectureApplying || model.placingArtwork
    }
    private var saveLocked: Bool { !ready || readOnly || model.busy || model.entrancePreviewActive || (model.immutable && !model.pending) }
    private func requestOpen(_ id: String?) {
        guard !model.entrancePreviewActive, !model.pending, id != nil || (active && !readOnly) else { return }
        if model.dirty { pendingOpen = id; pendingNew = id == nil; pendingAdopt = false; discardPrompt = true }
        else { Task { if let id { await model.open(id) } else if controller.editingContextActive { await model.new() } } }
    }
    private func inspect(_ value: InspectorScope) {
        guard value != .cover || !model.placingArtwork else { return }
        scope = value; inspectorVisible = true; browserVisible = false
        if value == .cover { coverPurpose = true }
    }
    private func showSelection() { inspect(.selection) }
    private func returnToScene() {
        if model.entrancePreviewActive {
            returningToScene = true; model.closeEntrancePreview()
        } else { coverPurpose = false; returningToScene = false; scope = .selection; if readOnly { edit() } }
    }

    var body: some View {
        VStack(spacing: 0) {
            workbenchToolbar
            Divider()
            GeometryReader { geometry in
            HStack(spacing: 0) {
                if browserVisible { exhibitionBrowser.frame(width: 220); Divider() }
                VStack(spacing: 0) {
                    // This node stays at the same structural position for all
                    // modes, purposes, sidebar and inspector presentations.
                    NativeViewportArea(model: model, ready: ready, editable: active && !model.immutable && !readOnly, controller: controller, workbench: true)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    if coverPurpose {
                        Divider()
                        CoverEntryPreviewControls(model: model, viewport: controller, ready: ready, mode: mode, compact: true, editable: active && !readOnly && !model.immutable)
                            .padding(8)
                    } else if !readOnly {
                        Divider()
                        ArtworkCatalogView(model: model, controller: controller, ready: ready, compact: true, condensed: geometry.size.height < 500)
                    }
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            }
            Divider()
            feedback
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .disabled(!active)
        .inspector(isPresented: inspectorPresented) { inspector }
        .inspectorColumnWidth(min: 240, ideal: 300, max: 420)
        .onChange(of: model.selectedPlaqueId) { _, id in if !id.isEmpty { showSelection() } }
        .onChange(of: model.selectedBenchId) { _, id in if !id.isEmpty { showSelection() } }
        .onChange(of: model.selectedLightId) { _, id in if !id.isEmpty { showSelection() } }
        .onChange(of: model.artworkId) { _, id in if !id.isEmpty { showSelection() } }
        .onChange(of: model.viewportSelection) { _, value in if value != nil && !model.entrancePreviewActive { scope = .selection; inspectorVisible = true } }
        .onChange(of: model.draft?.id) { _, id in if id != nil { browserVisible = false } }
        .onChange(of: model.entrancePreviewActive) { _, value in if !value && returningToScene { returnToScene() } }
        .onChange(of: ready) { _, value in if !value { needsConnection = true } }
        .task(id: ready && active && connectionAllowed) {
            if ready && active && connectionAllowed && needsConnection { needsConnection = false; await connect() }
        }
        .onChange(of: model.roomId) { _, _ in model.selectRoom() }
        .onChange(of: model.placementId) { _, _ in model.selectPlacement() }
        .confirmationDialog("현재 전시 입력을 바꿀까요?", isPresented: $discardPrompt) {
            Button("입력을 바꾸고 열기", role: .destructive) { Task { if pendingAdopt { if controller.editingContextActive { await model.adoptComparison() } } else if pendingNew { if controller.editingContextActive { await model.new() } } else if let id = pendingOpen { await model.open(id) } } }
            Button("계속 편집", role: .cancel) {}
        } message: { Text("저장본에 반영하지 않은 현재 입력이 바뀝니다. 필요한 입력은 먼저 이 Mac에 보관하거나 전시를 저장하세요.") }
    }
    private var workbenchToolbar: some View {
        HStack(spacing: 6) {
            Button { browserVisible.toggle(); if browserVisible { inspectorVisible = false } } label: { Label("전시 목록", systemImage: "sidebar.left") }
                .labelStyle(.iconOnly).help("전시 목록 보기 또는 가리기")
            Text(model.draft?.title ?? "전시 선택").font(.headline).lineLimit(1).layoutPriority(-1)
            Spacer(minLength: 4)
            ControlGroup {
                Button { Task { if controller.editingContextActive { await model.undo() } } } label: { Label("되돌리기", systemImage: "arrow.uturn.backward") }.disabled(!ready || readOnly || !model.canUndo)
                Button { Task { if controller.editingContextActive { await model.redo() } } } label: { Label("다시 실행", systemImage: "arrow.uturn.forward") }.disabled(!ready || readOnly || !model.canRedo)
            }.labelStyle(.iconOnly).fixedSize()
            if coverPurpose || readOnly || model.entrancePreviewActive {
                Button { returnToScene() } label: { Label("편집으로", systemImage: "arrow.backward") }
                    .labelStyle(.iconOnly).help("편집으로 돌아가기")
                    .disabled(model.entranceHostRecovering && !model.canRecoverEntranceHost)
                    .keyboardShortcut(.cancelAction)
            } else {
                Button { model.previewEntrance(.walk, systemReducedMotion: reducedMotion) } label: { Label("걸어보기", systemImage: "figure.walk") }
                    .labelStyle(.iconOnly).disabled(!ready || !model.viewportController.connected || !model.viewportController.entrancePreviewSupported || model.busy || model.pending || model.entrancePreviewBusy)
            }
            Button { Task { if controller.editingContextActive { await model.save() } } } label: { Label(model.pending ? "저장 응답 다시 확인" : "전시 저장", systemImage: "square.and.arrow.down") }
                .labelStyle(.iconOnly).disabled(saveLocked).keyboardShortcut("s", modifiers: .command)
            Menu {
                ForEach(InspectorScope.allCases) { value in Button(value.title) { inspect(value) }.disabled(value == .cover && model.placingArtwork) }
                Divider()
                Button(inspectorVisible ? "속성 가리기" : "속성 보기") { inspectorVisible.toggle() }
            } label: { Label("설정", systemImage: "sidebar.right") }
                .labelStyle(.iconOnly).menuStyle(.borderlessButton).fixedSize().disabled(model.draft == nil)
        }.controlSize(.regular).padding(.horizontal, 12).padding(.vertical, 8)
    }
    private var exhibitionBrowser: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                TextField("전시 검색", text: $model.query).onSubmit { Task { await model.refresh() } }
                Button { Task { await model.refresh() } } label: { Label("검색", systemImage: "magnifyingglass") }.labelStyle(.iconOnly)
            }.disabled(!ready || model.busy)
            Button("새 전시", systemImage: "plus") { requestOpen(nil) }.disabled(!ready || readOnly || model.busy || model.pending)
            List(selection: Binding<String?>(get: { model.items.contains(where: { $0.id == model.draft?.id }) ? model.draft?.id : nil }, set: { if let id = $0 { requestOpen(id) } })) {
                ForEach(model.items) { item in
                    VStack(alignment: .leading, spacing: 3) { Text(item.title).lineLimit(2); Text("저장본 \(item.revision)").font(.caption).foregroundStyle(.secondary) }.tag(item.id)
                }
            }.disabled(!ready || model.busy || model.pending)
            if model.cursor != nil { Button("전시 더 보기") { Task { await model.refresh(more: true) } }.disabled(!ready || model.busy) }
        }.padding(10)
    }
    private var feedback: some View {
        VStack(alignment: .leading, spacing: 4) {
            if model.draft == nil {
                HStack {
                    Text(ready ? "전시를 열거나 새로 만드세요." : "작업 환경에서 로컬 제작을 시작하세요.")
                    Button("전시 열기") { browserVisible = true }.disabled(!ready)
                    Button("새 전시") { requestOpen(nil) }.disabled(!ready || readOnly || model.busy)
                }
            } else if model.pending {
                Label("저장 응답을 확인할 때까지 현재 전시를 유지합니다.", systemImage: "clock")
            } else if model.immutable {
                HStack { Text("공개용 저장본").foregroundStyle(.secondary); Button("복제해서 편집") { Task { if controller.editingContextActive { await model.copyForEditing() } } }.disabled(!ready || readOnly || model.busy) }
            } else if model.dirty {
                Text("아직 저장하지 않은 입력이 있습니다.").foregroundStyle(.secondary)
            } else if model.notice.isEmpty {
                Text("전시 저장됨").foregroundStyle(.secondary)
            }
            if model.busy && !model.exporting { ProgressView("전시를 확인하고 있습니다…").controlSize(.small) }
            if let failure = model.failure {
                HStack(alignment: .top) {
                    Label(failure.code == "PLACEMENT_INVALID" ? "위치 X·Y·Z에 올바른 미터 값을 입력하거나 위치 입력을 취소하세요." : failure.localizedDescription, systemImage: "exclamationmark.triangle")
                    DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) }
                }
            } else if !model.notice.isEmpty && !model.dirty { Text(model.notice).foregroundStyle(.secondary).lineLimit(1) }
            if model.entrancePreviewActive && !model.entrancePreviewStatus.isEmpty { Text(model.entrancePreviewStatus).foregroundStyle(.secondary).lineLimit(2) }
            if !ready || (model.failure != nil && model.failure?.code != "PLACEMENT_INVALID") || model.entranceHostRecovering || !controller.diagnostic.isEmpty {
                Button("작업 공간 다시 연결") { Task { await connect() } }.disabled(!ready || model.busy)
            }
        }.font(.callout).frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 12).padding(.vertical, 6)
    }
    private var inspector: some View {
        VStack(spacing: 0) {
            Picker("속성 보기", selection: Binding(get: { scope }, set: { inspect($0) })) {
                ForEach(InspectorScope.allCases) { value in Text(value.title).tag(value) }
            }.padding(12)
            Divider()
            inspectorContent.disabled(readOnly && scope != .output)
        }
    }
    @ViewBuilder private var inspectorContent: some View {
        switch scope {
        case .selection:
            VStack(spacing: 0) {
                DisclosureGroup("대상 선택") {
                    NativeLightPicker(model: model, ready: ready)
                    NativeBenchPicker(model: model, ready: ready)
                    NativePlaquePicker(model: model, ready: ready)
                }.padding(10).disabled(model.entrancePreviewActive)
                if !model.artworkId.isEmpty { ScrollView { placementForm.padding(8) } }
                else if !model.selectedPlaqueId.isEmpty { SelectedPlaqueInspector(model: model, controller: model.viewportController, ready: ready, mode: mode).id(model.selectedPlaqueId) }
                else if !model.selectedBenchId.isEmpty { SelectedBenchInspector(model: model, controller: model.viewportController, ready: ready, mode: mode).id(model.selectedBenchId) }
                else if !model.selectedLightId.isEmpty { SelectedLightInspector(model: model, controller: model.viewportController, ready: ready, mode: mode).id(model.selectedLightId) }
                else if hasSelectedPlacement { SelectedPlacementInspector(model: model, ready: ready, mode: mode, inputs: selectedInputs).id(model.placementId) }
                else { ScrollView { ArchitectureSettingsView(model: model, ready: ready, mode: mode).padding(4) } }
            }
        case .lighting:
            ScrollView {
                Form { SpaceLightingSettingsView(model: model, ready: ready, mode: mode) }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true)
                NativeLightPicker(model: model, ready: ready).padding(12).disabled(model.entrancePreviewActive)
            }
        case .cover:
            ScrollView { Form { CoverEntrySettingsView(model: model, mode: mode) }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true).disabled(formLocked) }
        case .exhibition:
            ScrollView { exhibitionForm.padding(4) }
        case .floor:
            ScrollView { NativeFloorSettingsInspector(model: model, viewport: controller, ready: ready).padding(12) }
        case .output:
            ScrollView { outputControls.padding(12) }
        }
    }
    private var placementForm: some View {
        Form {
            Section("승인한 작품 배치") {
                Text(model.library.first(where: { $0.id == model.artworkId })?.title ?? "작품")
                TextField("위치 X (m)", text: $model.positionXText)
                TextField("위치 Y (m)", text: $model.positionYText)
                TextField("위치 Z (m)", text: $model.positionZText)
                if model.positionDirty { Button("위치 입력 취소") { model.cancelPositionInput() } }
                Button("선택한 작품 배치") { Task { if controller.editingContextActive { await model.addArtwork() } } }
            }
        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true).disabled(!ready || model.entrancePreviewActive || model.busy || model.pending || model.immutable || model.architectureDirty || model.architectureApplying)
    }
    private var exhibitionForm: some View {
        Form {
            Section("전시 정보") {
                TextField("전시 제목", text: $model.title)
                if let draft = model.draft {
                    Picker("공간", selection: $model.roomId) {
                        ForEach(draft.rooms.indices, id: \.self) { index in Text(draft.rooms[index]["name"].string ?? "공간").tag(draft.rooms[index]["id"].string ?? "") }
                    }.disabled(model.formDirty || model.architectureDirty || model.architectureApplying)
                    Picker("배치한 작품", selection: $model.placementId) {
                        Text("배치 선택").tag("")
                        ForEach(draft.placements.indices, id: \.self) { index in
                            let row = draft.placements[index]
                            Text("배치 \(index + 1) · " + (draft.artworks.first(where: { $0["revisionId"] == row["artworkRevisionId"] })?["metadata"]["title"].string ?? "작품")).tag(row["id"].string ?? "")
                        }
                    }.disabled(model.positionDirty || model.formDirty)
                    Text("공간 \(draft.rooms.count)개 · 작품 배치 \(draft.placements.count)개").foregroundStyle(.secondary)
                }
            }
        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true).disabled(formLocked)
    }
    private var outputControls: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("저장·내보내기").font(.headline)
            Button("입력 이 Mac에 보관") { Task { if controller.editingContextActive { await model.keepLocal() } } }.disabled(!ready || readOnly || model.busy || model.pending || model.immutable || model.entrancePreviewActive)
            Button(model.pending ? "저장 응답 다시 확인" : "전시 저장") { Task { if controller.editingContextActive { await model.save() } } }.buttonStyle(.borderedProminent).disabled(saveLocked)
            Button("저장본 비교") { Task { await model.compare() } }.disabled(!ready || model.busy || model.state?.etag == nil)
            if let comparison = model.comparison {
                Text("비교한 저장본: \(comparison.draft.title) · 저장본 \(comparison.revision) · 작품 배치 \(comparison.draft.placements.count)개")
                Button("비교한 저장본 열기") { pendingAdopt = true; pendingNew = false; pendingOpen = nil; discardPrompt = true }.disabled(!ready || readOnly || model.busy || model.pending || model.entrancePreviewActive)
            }
            Divider()
            NativeOEXExportControls(model: model, ready: ready)
            Divider()
            Text("내보내기·공개").font(.headline)
            Text("내보내기·공개 화면에서 공개용 저장본과 정적 전시 파일을 준비합니다.").foregroundStyle(.secondary)
            Button("내보내기·공개로 이동", systemImage: "globe") { publish() }.disabled(model.entrancePreviewActive || model.pending || model.busy)
            Button("작업 공간 다시 연결") { Task { await connect() } }.disabled(!ready || model.busy)
        }
    }
}
