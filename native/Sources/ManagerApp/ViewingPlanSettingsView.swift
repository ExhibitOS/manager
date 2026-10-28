// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// General and Expert mount this same inspector and model-owned text buffer.
/// Choosing a stop/viewpoint never moves the editing or visitor camera.
struct ViewingPlanSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let ready: Bool
    private var editable: Bool { ready && model.canEditViewingPlan }
    private var views: [BenchViewpoint] { model.draft.flatMap { try? BenchViewpoint.list(in: $0) } ?? [] }
    private func title(_ placementID: String) -> String {
        guard let draft = model.draft,
              let placement = draft.placements.first(where: { $0["id"].string?.lowercased() == placementID.lowercased() }),
              let artwork = draft.artworks.first(where: { $0["revisionId"].string?.lowercased() == placement["artworkRevisionId"].string?.lowercased() }) else { return "배치한 작품" }
        return artwork["metadata"]["title"].string ?? "제목 없는 작품"
    }
    private func sculpture(_ placementID: String) -> Bool {
        guard let draft = model.draft,
              let placement = draft.placements.first(where: { $0["id"].string?.lowercased() == placementID.lowercased() }) else { return false }
        return draft.artworks.first(where: { $0["revisionId"].string?.lowercased() == placement["artworkRevisionId"].string?.lowercased() })?["artworkType"].string == "sculpture"
    }
    private func field<Value>(_ placementID: String, _ key: WritableKeyPath<ViewingPlanFormBuffer.Stop, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { model.viewingPlan?.stops.first(where: { $0.placementId == placementID })?[keyPath: key] ?? fallback }, set: { value in
            guard editable, var fields = model.viewingPlan, let index = fields.stops.firstIndex(where: { $0.placementId == placementID }) else { return }
            fields.stops[index][keyPath: key] = value; model.viewingPlan = fields
        })
    }
    private func move(_ placementID: String, by offset: Int) {
        guard editable, var fields = model.viewingPlan else { return }
        fields.move(placementID, by: offset); model.viewingPlan = fields
    }
    private func stop(_ value: ViewingPlanFormBuffer.Stop, index: Int, count: Int) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("\(index + 1). \(title(value.placementId))").font(.headline)
            Picker("감상 시점", selection: field(value.placementId, \.viewpointId, fallback: "")) {
                Text("기존 시점을 선택하세요").tag("")
                ForEach(views, id: \.id) { view in
                    Text(view.name).tag(view.id)
                }
            }
            HStack {
                Button("위로", systemImage: "arrow.up") { move(value.placementId, by: -1) }.disabled(index == 0)
                Button("아래로", systemImage: "arrow.down") { move(value.placementId, by: 1) }.disabled(index + 1 == count)
                Button("순서에서 빼기") {
                    guard editable, var fields = model.viewingPlan else { return }
                    fields.stops.removeAll { $0.placementId == value.placementId }; model.viewingPlan = fields
                }.help("전시의 작품 배치와 원본은 그대로 유지됩니다.")
            }
            if sculpture(value.placementId) {
                Toggle("둘러볼 범위 지정", isOn: field(value.placementId, \.usesOrbit, fallback: false))
                if value.usesOrbit {
                    TextField("왼쪽 한도 · −180–0°", text: field(value.placementId, \.minimumDegrees, fallback: ""))
                    TextField("오른쪽 한도 · 0–180°", text: field(value.placementId, \.maximumDegrees, fallback: ""))
                    Text("선택한 감상 시점을 기준으로 합니다. 관람 중에는 안전을 확인한 위치만 사용합니다.").font(.caption).foregroundStyle(.secondary)
                }
            }
        }.textFieldStyle(.roundedBorder)
    }
    var body: some View {
        Form {
            Section("관람 순서") {
                if let fields = model.viewingPlan {
                    Toggle("관람 순서 지정", isOn: Binding(get: { model.viewingPlan?.usesPlan ?? false }, set: { value in
                        guard editable, var next = model.viewingPlan else { return }; next.usesPlan = value; model.viewingPlan = next
                    })).disabled(!editable)
                    Text("관람자가 자유롭게 관람하거나 지정한 순서로 볼 수 있습니다. 관람 중 위치와 진행은 전시에 저장하지 않습니다.").font(.caption).foregroundStyle(.secondary)
                    if fields.usesPlan {
                        ForEach(fields.stops) { value in
                            if let index = fields.stops.firstIndex(where: { $0.placementId == value.placementId }) {
                                stop(value, index: index, count: fields.stops.count).disabled(!editable)
                            }
                        }
                        Menu("작품 추가", systemImage: "plus") {
                            let placements = model.draft?.placements ?? []
                            ForEach(placements.indices, id: \.self) { index in
                                if let id = placements[index]["id"].string,
                                   !fields.stops.contains(where: { $0.placementId.lowercased() == id.lowercased() }) {
                                    Button("\(index + 1). \(title(id))") {
                                        guard editable, var next = model.viewingPlan else { return }
                                        next.append(placementId: id); model.viewingPlan = next
                                    }
                                }
                            }
                        }.disabled(!editable || fields.stops.count >= 128 || views.isEmpty)
                    }
                    if !fields.usesPlan || fields.stops.isEmpty {
                        Text("지정한 순서가 없으면 기존 입장 방식과 작품 목록 순서를 사용합니다.").font(.caption).foregroundStyle(.secondary)
                    }
                    if views.isEmpty { Text("연결할 감상 시점이 없습니다. 기존 감상 시점을 작성한 뒤 순서에 작품을 추가하세요.").font(.caption).foregroundStyle(.secondary) }
                    Button("관람 계획 확인") { Task { await model.previewViewingPlan() } }.disabled(!ready || !model.canPreviewViewingPlan)
                    Button("관람 계획 적용") { Task { await model.applyViewingPlan() } }.disabled(!ready || !model.canApplyViewingPlan)
                    Button("관람 계획 입력 취소") { model.discardViewingPlan() }.disabled(!model.viewingPlanDirty || model.busy || model.pending || model.architectureApplying)
                    Text("적용한 뒤 실행 취소로 되돌리거나 전시를 저장할 수 있습니다.").font(.caption).foregroundStyle(.secondary)
                }
                if !controller.viewingPlanSupported {
                    Text("관람 계획 편집을 지원하는 제작 서비스에 연결하세요. 저장된 설정과 입력은 유지됩니다.").font(.caption).foregroundStyle(.secondary)
                }
                if !model.viewingPlanStatus.isEmpty { Text(model.viewingPlanStatus).foregroundStyle(.secondary) }
            }
        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true)
    }
}
