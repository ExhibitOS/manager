// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// Existing native card/search and grouped inspector patterns, with a separate
/// catalog-source identity and no artwork import/thumbnail authority.
struct FurnishingCatalogView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var viewport: NativeViewportController
    let ready: Bool
    let mode: Mode
    @State private var sourceDetails = false
    private func field(_ key: WritableKeyPath<FurnishingFormBuffer, String>) -> Binding<String> {
        Binding(get: { model.furnishing?[keyPath: key] ?? "" }, set: { value in
            guard var fields = model.furnishing else { return }; fields[keyPath: key] = value; model.furnishing = fields
        })
    }
    private var matchesSearch: Bool {
        let q = model.furnishingQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        return q.isEmpty || (FurnishingCatalog.title + " 받침 전시 pedestal plinth").localizedCaseInsensitiveContains(q)
    }
    private func select(_ selected: Bool) {
        guard var fields = model.furnishing else { return }; fields.selected = selected; model.furnishing = fields
    }
    private func dimensions(_ values: [Double]) -> String { values.map { $0.formatted(.number.precision(.fractionLength(0...3))) }.joined(separator: " × ") + " m" }
    var body: some View {
        GroupBox("비품 · 받침") {
            VStack(alignment: .leading, spacing: 10) {
                if !viewport.furnishingVisualsSupported {
                    Text("현재 제작 서비스에서는 비품 카탈로그를 지원하지 않습니다. 기존 받침 설정을 사용할 수 있습니다.").font(.callout).foregroundStyle(.secondary)
                } else {
                    TextField("비품 이름 검색", text: $model.furnishingQuery).textFieldStyle(.roundedBorder)
                    Picker("비품 종류", selection: $model.furnishingCategory) { Text("전체").tag("all"); Text("받침").tag("pedestal") }.pickerStyle(.segmented)
                    if model.furnishingCatalogReady && matchesSearch {
                        Button { select(true) } label: {
                            VStack(alignment: .leading, spacing: 6) {
                                ZStack {
                                    Color(nsColor: .textBackgroundColor)
                                    if let image = model.furnishingImage { Image(nsImage: image).resizable().scaledToFit().padding(4) }
                                    else { Label("미리보기 없음", systemImage: "cube").font(.caption).foregroundStyle(.secondary) }
                                }.frame(height: 110).clipShape(RoundedRectangle(cornerRadius: 6))
                                Label(FurnishingCatalog.title, systemImage: model.furnishing?.selected == true ? "checkmark.circle.fill" : "circle")
                                Text("받침 · CC0 1.0").font(.caption).foregroundStyle(.secondary)
                            }.padding(8).frame(maxWidth: .infinity, alignment: .leading)
                        }.buttonStyle(.bordered).disabled(!ready || !model.canEditFurnishing)
                            .accessibilityLabel(FurnishingCatalog.title + (model.furnishing?.selected == true ? " · 선택됨" : " · 선택"))
                            .accessibilityHint("선택한 작품 배치의 받침 외형을 고릅니다. 미리보기와 적용 전에는 전시가 바뀌지 않습니다.")
                            .task(id: model.viewportAccessID) { await model.loadFurnishingImage() }
                    } else if model.furnishingCatalogReady { Text("검색 결과가 없습니다.").foregroundStyle(.secondary) }
                    if !model.furnishingCatalogStatus.isEmpty {
                        Text(model.furnishingCatalogStatus).font(.callout).foregroundStyle(.secondary)
                        Button("비품 목록 다시 확인") { Task { await model.retryFurnishingCatalog() } }.disabled(!ready || model.busy || model.architectureApplying)
                    }
                    if let fields = model.furnishing {
                        if fields.selected {
                            VStack(alignment: .leading, spacing: 8) {
                                Text("받침 원점의 위치 · 방 좌표").font(.caption).foregroundStyle(.secondary)
                                TextField("받침 X · m", text: field(\.x))
                                TextField("받침 Y · m", text: field(\.y))
                                TextField("받침 Z · m", text: field(\.z))
                                TextField("받침 회전 · −180–180°", text: field(\.yawDegrees))
                                TextField("비례 크기 · 0.1–4", text: field(\.scale))
                                if let command = try? fields.command(hostId: model.placementId), let pose = command.pose {
                                    LabeledContent("폭 × 높이 × 깊이", value: dimensions(pose.dimensions))
                                    LabeledContent("받침 위 기준점 · X/Y/Z", value: dimensions(pose.point(FurnishingCatalog.support)))
                                } else { Text("위치·회전·크기를 입력한 뒤 미리 보세요.").font(.caption).foregroundStyle(.secondary) }
                                Text("작품 위치는 자동으로 바뀌지 않습니다. 원본에 받침이 포함돼 있는지 확인하세요.").font(.caption).foregroundStyle(.secondary)
                                Button("받침 외형 빼기") { select(false) }
                            }.textFieldStyle(.roundedBorder).disabled(!ready || !model.canEditFurnishing)
                        }
                        HStack {
                            Button("비품 미리보기") { Task { await model.previewFurnishing() } }.disabled(!ready || !model.canPreviewFurnishing)
                            Button("비품 적용") { Task { await model.applyFurnishing() } }.disabled(!ready || !model.canApplyFurnishing)
                        }
                        Button("비품 입력 취소") { model.discardFurnishing() }.disabled(!model.furnishingDirty || model.architectureApplying || model.busy)
                    }
                    if model.selectedHasFurnishing {
                        Text("선택한 외형에 맞춰 보수적 받침 크기를 유지합니다. 외형을 빼고 적용하면 기본 받침 설정을 다시 편집할 수 있습니다.").font(.caption).foregroundStyle(.secondary)
                        HStack {
                            Button("작품 중앙 맞추기") { align(.centerArtwork(placementId: model.placementId)) }
                            Button("받침 위에 작품 놓기") { align(.onTop(placementId: model.placementId)) }
                        }.disabled(!ready || !model.canSelectCatalog || !viewport.connected)
                    }
                    if mode == .expert { sourceInformation }
                    else { DisclosureGroup("출처와 지원 범위", isExpanded: $sourceDetails) { sourceInformation } }
                }
                if !model.furnishingStatus.isEmpty { Text(model.furnishingStatus).font(.callout).foregroundStyle(.secondary) }
            }.padding(4)
        }
        .task(id: "\(model.viewportAccessID)-\(viewport.furnishingVisualsSupported)-\(viewport.connected)") {
            if ready { await model.loadFurnishingCatalog() }
        }
    }
    private var sourceInformation: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Innerscene 3D Library · CC0 1.0 · 제작자 미표기").font(.caption)
            Text("현재 받침 1종을 지원합니다. 크기는 전체 비율로 조절하며 작품 원본은 바꾸지 않습니다. 전시에는 배치한 비품과 라이선스만 포함합니다.").font(.caption).foregroundStyle(.secondary)
        }
    }
    private func align(_ command: InstallationCommand) { Task { await viewport.perform(.installation(command), model: model) } }
}
