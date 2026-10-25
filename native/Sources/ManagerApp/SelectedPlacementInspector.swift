// SPDX-License-Identifier: Apache-2.0
import SwiftUI
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
    let ready: Bool
    let mode: Mode
    @Binding var inputs: InstallationInspectorState

    private var selected: (placement: DocumentValue, artwork: DocumentValue)? {
        guard let draft = model.draft, let selection = model.viewportSelection, selection.kind == "placement",
              selection.id == model.placementId,
              let placement = draft.placements.first(where: { $0["id"].string == selection.id }),
              let artwork = draft.artworks.first(where: { $0["revisionId"] == placement["artworkRevisionId"] }) else { return nil }
        return (placement, artwork)
    }
    private var editable: Bool { ready && !model.busy && !model.pending && !model.immutable && !model.architectureDirty && !model.architectureApplying && !model.placingArtwork }
    private var canAlign: Bool { editable && !model.formDirty && !model.positionDirty && model.viewportController.connected }
    private func surfaces(_ kind: String, placement: DocumentValue) -> [(id: String, name: String)] {
        (model.draft?.candidate["surfaces"].items ?? []).filter { $0["roomId"] == placement["roomId"] && $0["type"].string == kind }
            .enumerated().compactMap { index, row in row["id"].string.map { ($0, (kind == "floor" ? "바닥 " : "벽 ") + String(index + 1)) } }
    }
    private func number(_ value: Double) -> String { value.formatted(.number.precision(.fractionLength(0...4))) }
    private func sourceDimensions(_ artwork: DocumentValue) -> String {
        [("width", "폭"), ("height", "높이"), ("depth", "깊이")].compactMap { key, label in
            artwork["dimensions"][key].double.map { label + " " + number($0) + " m" }
        }.joined(separator: " · ")
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
                VStack(alignment: .leading, spacing: 16) {
                    Text("선택한 작품 배치").font(.headline).accessibilityAddTraits(.isHeader)
                    Text(selected.artwork["metadata"]["title"].string ?? "제목 없는 작품").font(.title3.weight(.semibold))
                    Text(selected.artwork["metadata"]["artist"].string ?? "작가 미상").foregroundStyle(.secondary)
                    LabeledContent("원본 실측", value: sourceDimensions(selected.artwork))
                    Text("이 속성은 선택한 배치에 적용됩니다. 작품 원본과 실측 치수는 유지됩니다.").font(.caption).foregroundStyle(.secondary)
                    Button("공간에서 찾기", systemImage: "scope") { Task { await model.viewportController.findSelection() } }
                        .disabled(!editable || !model.viewportController.findSelectionSupported || !model.canSelectCatalog || model.viewportPresentation == .plan)
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
                                Text("회전과 배율은 공간의 편집 조작에서 변경합니다. 원본 실측과 구분해 표시합니다.").font(.caption).foregroundStyle(.secondary)
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
