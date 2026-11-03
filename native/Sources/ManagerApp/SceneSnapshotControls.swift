// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// Uses an existing authored viewpoint. This picker is transient UI state, never
/// an exhibition edit or an implicit camera/selection change.
struct SceneSnapshotControls: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let placement: DocumentValue
    let ready: Bool
    @State private var viewpointId = ""
    private var viewpoints: [(id: String, name: String)] {
        model.draft?.candidate["extensions"]["org.exhibitos.studio/presentation"]["viewpoints"].items?
            .filter { $0["roomId"] == placement["roomId"] }.compactMap { view in
                view["id"].string.map { ($0, view["name"].string ?? "감상 시점") }
            } ?? []
    }
    var body: some View {
        GroupBox("감상 시점 이미지") {
            VStack(alignment: .leading, spacing: 8) {
                if controller.connected && !controller.sceneSnapshotSupported {
                    Text("이 실행 환경은 감상 시점 이미지를 지원하지 않습니다.").font(.caption).foregroundStyle(.secondary)
                }
                if model.viewportTransparent {
                    Text("‘공간을 투명하게 보기’를 끈 뒤 이미지를 저장하세요.").font(.caption).foregroundStyle(.secondary)
                }
                if viewpoints.isEmpty {
                    Text("이 공간에 저장된 감상 시점이 필요합니다.").font(.caption).foregroundStyle(.secondary)
                } else {
                    Picker("감상 시점", selection: $viewpointId) {
                        Text("시점 선택").tag("")
                        ForEach(viewpoints, id: \.id) { view in
                            Text(view.name).tag(view.id)
                        }
                    }
                    Button("시점 이미지 저장…", systemImage: "camera") {
                        guard let placementId = placement["id"].string else { return }
                        Task { await model.saveSceneSnapshot(viewpointId: viewpointId, placementId: placementId) }
                    }.disabled(!ready || !model.canCaptureSceneSnapshot || !viewpoints.contains(where: { $0.id == viewpointId }))
                    Text("저장된 전시를 640 × 360 PNG로 저장합니다. 편집 중인 화면의 시점은 유지됩니다.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if model.sceneSnapshotting { ProgressView().controlSize(.small).accessibilityLabel("감상 시점 이미지 준비 중") }
                if !model.sceneSnapshotStatus.isEmpty { Text(model.sceneSnapshotStatus).font(.caption).textSelection(.enabled) }
            }.padding(4)
        }
    }
}
