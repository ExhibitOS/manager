// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// Reuses the current inspector and the existing shared 3D viewport.
struct SharedFurnishingCatalogView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var viewport: NativeViewportController
    let ready: Bool
    private func field(_ key: WritableKeyPath<SharedFurnishingFormBuffer, String>) -> Binding<String> {
        Binding(get: { model.sharedFurnishing?[keyPath: key] ?? "" }, set: { value in
            guard var form = model.sharedFurnishing else { return }; form[keyPath: key] = value; model.sharedFurnishing = form
        })
    }
    private var selected: Binding<Bool> {
        Binding(get: { model.sharedFurnishing?.selected ?? false }, set: { value in
            guard var form = model.sharedFurnishing else { return }; form.selected = value; model.sharedFurnishing = form
        })
    }
    private func title(_ host: SharedFurnishingCommand.Host) -> String {
        switch host { case .bench: "등받이 없는 참나무 벤치"; case .light: "전시용 트랙 스포트"; case .opening: "열린 전시 통로"; case .pedestal: "사각 전시 받침" }
    }
    var body: some View {
        if viewport.sharedFurnishingVisualsSupported, let target = model.sharedFurnishingTarget {
            GroupBox("비품 외형") {
                VStack(alignment: .leading, spacing: 8) {
                    if model.sharedFurnishingCatalogReady, let form = model.sharedFurnishing, form.host == target.0, form.hostId == target.1 {
                        Toggle(title(target.0), isOn: selected).disabled(!ready || !model.canEditSharedFurnishing)
                        Text("CC0 1.0 · 전시에 사용한 모델과 고지문을 함께 보관합니다.").font(.caption).foregroundStyle(.secondary)
                        if form.selected {
                            VStack(alignment: .leading, spacing: 8) {
                            if target.0 == .pedestal || target.0 == .bench {
                                Text("공간 기준 좌표 · 미터").font(.caption).foregroundStyle(.secondary)
                                TextField("가로 위치 X · m", text: field(\.x)); TextField("높이 Y · m", text: field(\.y)); TextField("깊이 Z · m", text: field(\.z))
                                TextField("방향 · −180–180°", text: field(\.yawDegrees))
                            }
                            TextField("비례 크기 · 0.1–4", text: field(\.scale))
                            if target.0 == .light { Text("스폿 조명의 발광 위치에 맞춰 외형을 놓습니다.").font(.caption).foregroundStyle(.secondary) }
                            if target.0 == .opening { Text("문의 위치에 맞춰 외형을 놓습니다. 연결된 문의 크기가 달라지면 적용되지 않습니다.").font(.caption).foregroundStyle(.secondary) }
                            }.disabled(!ready || !model.canEditSharedFurnishing)
                        }
                        HStack {
                            Button("비품 미리보기") { Task { await model.previewSharedFurnishing() } }.disabled(!ready || !model.canPreviewSharedFurnishing)
                            Button("비품 적용") { Task { await model.applySharedFurnishing() } }.disabled(!ready || !model.canApplySharedFurnishing)
                        }
                        Button("비품 입력 취소") { model.discardSharedFurnishing() }.disabled(!model.sharedFurnishingDirty || model.busy || model.architectureApplying)
                        if target.0 == .pedestal, form.selected, model.selectedHasFurnishing {
                            Button("크기를 확인하고 작품을 받침 위에 놓기") {
                                Task { await viewport.perform(.installation(.alignOnVerifiedPlinth(placementId: target.1)), model: model) }
                            }.disabled(!ready || !model.canSelectCatalog || model.immutable || !viewport.connected || !viewport.plinthEnvelopeAlignSupported)
                            Text("작품의 크기 범위가 받침 영역에 들어오는지만 확인합니다. 실제 바닥 접촉과 안정성은 확인되지 않았습니다.").font(.caption).foregroundStyle(.secondary)
                            if !viewport.plinthEnvelopeAlignSupported { Text("현재 연결에서는 이 배치 조작을 지원하지 않습니다.").font(.caption).foregroundStyle(.secondary) }
                        }
                    } else if !model.sharedFurnishingCatalogReady {
                        if model.sharedFurnishingStatus.isEmpty { Text("비품 목록을 확인하고 있습니다.").font(.caption).foregroundStyle(.secondary) }
                        Button("비품 목록 다시 확인") { Task { await model.retrySharedFurnishingCatalog() } }.disabled(!ready || model.busy)
                    }
                    if !model.sharedFurnishingStatus.isEmpty { Text(model.sharedFurnishingStatus).font(.callout).foregroundStyle(.secondary) }
                }.textFieldStyle(.roundedBorder).padding(4)
            }
            .task(id: "\(target.0.rawValue)-\(target.1)-\(model.viewportAccessID)-\(viewport.connected)-\(model.busy)") {
                model.syncSharedFurnishing(); if ready { await model.loadSharedFurnishingCatalog() }
            }
        }
    }
}
