// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import UniformTypeIdentifiers
import ManagerCore

/// Former installation-view @State stays owned by the exhibition view, keyed to
/// its exact placement. Hiding the inspector or changing mode never resets input.
struct InstallationInspectorState {
    var advancedPedestal = false
    var advancedFrame = false
    var positionDetails = false
    var pedestalPoseDetails = false
    var floor = ""
    var wall = ""
    var installHeight = "1.5"
    var wallGap = "0.02"
}

struct SelectedPlacementInspector: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var authoring: AuthoringModel
    let ready: Bool
    let mode: Mode
    @Binding var inputs: InstallationInspectorState
    @State private var choosingJPEG = false
    @State private var confirmImageReplacement = false
    @State private var reviewedReceiptSHA256: String?

    private var selected: (placement: DocumentValue, artwork: DocumentValue)? {
        guard let draft = model.draft, let selection = model.viewportSelection, selection.kind == "placement",
              selection.id == model.placementId,
              let placement = draft.placements.first(where: { $0["id"].string == selection.id }),
              let artwork = draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] }) else { return nil }
        return (placement, artwork)
    }
    private var editable: Bool { ready && !model.busy && !model.pending && !model.immutable && !model.architectureDirty && !model.architectureApplying && !model.placingArtwork }
    private var canAlign: Bool { editable && !model.formDirty && !model.positionDirty && model.viewportController.connected }

    @ViewBuilder private func productImageControls(placementId: String) -> some View {
        if authoring.productImagePreparationAvailable, let target = model.productImageTarget(placementId: placementId),
           let transfer = authoring.transfer {
            let intent = transfer.intent
            let active = intent?.artworkId == target.id && intent?.productImagePreparation != nil
            let readyForReview = active && intent?.productImagePreparation?.artworkRevision == target.revision
            let status = active ? authoring.importJob?.productImagePreparation : nil
            let committed = active && intent?.productImagePreparation.map { $0.artworkRevision + 1 } == target.revision
            GroupBox("전시에 표시할 이미지") {
                VStack(alignment: .leading, spacing: 8) {
                    if active, let name = transfer.fileName { Text(name).textSelection(.enabled) }
                    if committed {
                        Button("교체 저장본 확인") {
                            Task { await model.finishSavedProductImageTransfer(authoring: authoring) }
                        }
                    } else if readyForReview, let status, status.status == .ready, let full = status.full,
                              let receipt = status.serverReceipt {
                        Text("표시용 PNG \(full.width) × \(full.height)")
                        if let coarse = status.coarse { Text("경량 PNG \(coarse.width) × \(coarse.height)") }
                        Button("교체하고 저장…") {
                            reviewedReceiptSHA256 = receipt.sha256
                            confirmImageReplacement = true
                        }
                    } else if readyForReview, let job = authoring.importJob, job.state == "cancelled" {
                        Text("이미지 준비를 취소했습니다.").foregroundStyle(.secondary)
                        Button("취소 확인") { Task { await authoring.changeProductImagePreparation(.cancel, artworkId: target.id) } }
                            .disabled(status?.workerDrained != true)
                    } else if readyForReview, let job = authoring.importJob, status?.status == .failed {
                        Text("이미지를 준비하지 못했습니다.").foregroundStyle(.secondary)
                        if job.canRetry && status?.workerDrained == true {
                            Button("다시 준비") { Task { await authoring.changeProductImagePreparation(.retry, artworkId: target.id) } }
                        }
                        Button("준비 취소") { Task { await authoring.changeProductImagePreparation(.cancel, artworkId: target.id) } }
                    } else if readyForReview, let job = authoring.importJob, job.state != "uploading" {
                        ProgressView("표시용 이미지 준비 중").controlSize(.small)
                        if job.state != "approved" {
                            Button("준비 취소") { Task { await authoring.changeProductImagePreparation(.cancel, artworkId: target.id) } }
                        }
                    } else if transfer.compactVariant == nil && (intent == nil || readyForReview) {
                        Button(readyForReview ? "같은 JPEG 다시 선택…" : "교체할 JPEG 선택…", systemImage: "photo") {
                            choosingJPEG = true
                        }
                    } else {
                        Text("진행 중인 작품 파일 작업을 마친 뒤 선택할 수 있습니다.").foregroundStyle(.secondary)
                    }
                }.padding(4)
            }
            .disabled(!editable || model.dirty || authoring.busy || authoring.dirty)
            .fileImporter(isPresented: $choosingJPEG, allowedContentTypes: [.jpeg]) { result in
                if case .success(let url) = result {
                    Task { await model.prepareProductImage(url, placementId: placementId, authoring: authoring) }
                }
            }
            .confirmationDialog("이 전시의 작품 이미지를 교체할까요?", isPresented: $confirmImageReplacement,
                                titleVisibility: .visible) {
                Button("교체하고 저장") {
                    let receipt = reviewedReceiptSHA256
                    Task { await model.replaceWithProductImage(authoring: authoring, reviewedReceiptSHA256: receipt) }
                }
            } message: {
                Text("이 전시에서 같은 작품을 사용하는 모든 배치에 적용됩니다. JPEG 원본은 보관됩니다.")
            }
        }
    }
    private func surfaces(_ kind: String, placement: DocumentValue) -> [(id: String, name: String)] {
        (model.draft?.candidate["surfaces"].items ?? []).filter { $0["roomId"] == placement["roomId"] && $0["type"].string == kind }
            .enumerated().compactMap { index, row in row["id"].string.map { ($0, (kind == "floor" ? "바닥 " : "벽 ") + String(index + 1)) } }
    }
    private func number(_ value: Double) -> String { value.formatted(.number.precision(.fractionLength(0...4))) }
    private func sourceDimensions(_ size: ArtworkSizePresentation) -> String {
        let axes = size.sourceUnitsUnknown ? [("height", "높이"), ("width", "폭"), ("depth", "깊이")]
            : [("width", "폭"), ("height", "높이"), ("depth", "깊이")]
        let text = axes.compactMap { key, label in
            size.recordedDimensions[key].double.map { label + " " + number($0) + " m" }
        }.joined(separator: " · ")
        return text.isEmpty ? "치수 확인 중" : text
    }
    private func vector(_ value: DocumentValue) -> String {
        value.items?.compactMap(\.double).map(number).joined(separator: ", ") ?? "확인할 수 없음"
    }
    private var positionFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("선택한 공간 기준 · 미터").font(.caption).foregroundStyle(.secondary)
            TextField("가로 위치 X · m", text: $model.positionXText)
            TextField("높이 위치 Y · m", text: $model.positionYText)
            TextField("깊이 위치 Z · m", text: $model.positionZText)
            HStack {
                Button("위치 적용") { Task { await model.movePlacement() } }.disabled(!model.positionDirty)
                Button("위치 입력 취소") { model.cancelPositionInput() }.disabled(!model.positionDirty)
            }
        }.textFieldStyle(.roundedBorder).disabled(!editable)
    }
    var body: some View {
        ScrollView {
            if let selected {
                let size = ArtworkSizePresentation(selected.artwork)
                VStack(alignment: .leading, spacing: 16) {
                    Text("선택한 작품 배치").font(.headline).accessibilityAddTraits(.isHeader)
                    Text(selected.artwork["metadata"]["title"].string ?? "제목 없는 작품").font(.title3.weight(.semibold))
                    Text(selected.artwork["metadata"]["artist"].string ?? "작가 미상").foregroundStyle(.secondary)
                    LabeledContent(size.sourceUnitsUnknown ? "등록된 크기" : "기록된 크기", value: sourceDimensions(size))
                    if size.sourceUnitsUnknown {
                        Text("GLB 원본 단위와 실물 치수의 좌표축 대응은 미확인입니다.").font(.caption).foregroundStyle(.secondary)
                    }
                    if let height = size.provisionalHeightMeters {
                        LabeledContent("임시 표시 높이 · 미보정", value: number(height) + " m")
                    }
                    Text("이 속성은 선택한 배치에 적용됩니다. 작품 원본과 기록된 크기는 유지됩니다.").font(.caption).foregroundStyle(.secondary)
                    Button("공간에서 찾기", systemImage: "scope") { Task { await model.viewportController.findSelection() } }
                        .disabled(!editable || !model.viewportController.findSelectionSupported || !model.canSelectCatalog || model.viewportPresentation == .plan)
                    SceneSnapshotControls(model: model, controller: model.viewportController, placement: selected.placement, ready: ready)
                    if let id = selected.placement["id"].string {
                        productImageControls(placementId: id)
                    }
                    GroupBox("높이 맞추기") {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("작품 중심 높이").font(.caption).foregroundStyle(.secondary)
                            let floors = surfaces("floor", placement: selected.placement)
                            HStack {
                                ForEach([1.2, 1.5, 1.7], id: \.self) { height in
                                    Button(number(height) + " m") {
                                        guard floors.count == 1 else { return }
                                        let command = InstallationCommand.installHeight(placementId: selected.placement["id"].string!, height: height, floorSurfaceId: floors[0].id)
                                        Task { await model.viewportController.perform(.installation(command), model: model) }
                                    }
                                }
                            }.disabled(!canAlign || floors.count != 1)
                            if floors.isEmpty { Text("선택한 공간에 바닥을 만든 뒤 높이를 맞추세요.").font(.caption).foregroundStyle(.secondary) }
                            else if floors.count > 1 { Text("아래 ‘설치 높이와 벽 간격’에서 기준 바닥을 선택하세요.").font(.caption).foregroundStyle(.secondary) }
                        }.padding(4)
                    }
                    if mode == .expert { GroupBox("정확한 위치") { positionFields.padding(4) } }
                    else { DisclosureGroup("위치를 숫자로 다듬기", isExpanded: $inputs.positionDetails) { positionFields.padding(.top, 6) } }
                    if model.positionDirty { Text("위치 입력을 적용하거나 취소한 뒤 다른 배치를 선택하세요.").font(.caption).foregroundStyle(.secondary) }
                    if !model.viewportController.sharedFurnishingVisualsSupported { FurnishingCatalogView(model: model, viewport: model.viewportController, ready: ready, mode: mode) }
                    SharedFurnishingCatalogView(model: model, viewport: model.viewportController, ready: ready)
                    if model.frameImageFitAvailable {
                        GroupBox("액자 맞춤") {
                            Button("이미지에 맞추기") { Task { await model.reconcileSelectedFrameImageFit() } }
                                .disabled(!editable || !model.canReconcileFrameImageFit)
                                .accessibilityHint("사진 전체를 유지하며 액자 안쪽 경계를 맞춥니다.")
                                .padding(4)
                        }
                    }
                    InstallationSettingsView(buffer: $model.installation, inspector: $inputs, mode: mode, placementId: model.placementId, dirty: model.installationDirty,
                        editable: editable && model.installationEditable, viewport: model.viewportController, canAlign: !model.formDirty && !model.positionDirty,
                        status: model.installationStatus, floors: surfaces("floor", placement: selected.placement), walls: surfaces("wall", placement: selected.placement),
                        apply: { Task { await model.applyInstallation() } }, discard: { model.discardInstallation() }, reset: { model.resetInstallation() },
                        align: { command in Task { await model.viewportController.perform(.installation(command), model: model) } })
                    if mode == .expert {
                        GroupBox("배치 참조값") {
                            VStack(alignment: .leading, spacing: 8) {
                                LabeledContent("회전 · xyzw", value: vector(selected.placement["transform"]["rotation"]))
                                LabeledContent("배치 배율", value: vector(selected.placement["transform"]["scale"]))
                                Text("회전과 배율은 공간의 편집 조작에서 변경합니다. 작품에 기록된 크기와 배치 배율은 별도로 표시합니다.").font(.caption).foregroundStyle(.secondary)
                                Text("배치 ID · " + model.placementId).font(.caption).textSelection(.enabled)
                            }.padding(4)
                        }
                    }
                }.padding(16)
            } else {
                ContentUnavailableView("작품 배치를 선택하세요", systemImage: "cursorarrow.rays", description: Text("작품 목록이나 공간에서 선택한 배치의 속성이 여기에 표시됩니다."))
            }
        }.inspectorColumnWidth(min: 280, ideal: 320, max: 420)
    }
}
