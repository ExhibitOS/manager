// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// Both modes share the same buffers and existing central scene.
struct NativeFloorSettingsInspector: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var viewport: NativeViewportController
    let ready: Bool
    private var targets: [(key: String, name: String)] {
        model.floorTargets.enumerated().map { index, target in
            let room = model.draft?.rooms.first { $0["id"].string?.lowercased() == target.id.lowercased() }?["name"].string ?? "공간"
            return (target.key, target.kind == .surface ? "바닥 \(index + 1)" : room + " · 기본 바닥")
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            FloorSettingsView(buffer: $model.floor, targets: targets, targetKey: model.floorTargetKey,
                select: model.selectFloor, customized: model.floorCustomized, currentValues: model.floorCurrentValues,
                dirty: model.floorDirty, canApply: ready && model.canApplyFloor, images: model.floorImages,
                preview: model.previewFloor, canReset: model.floorCanReset, editable: ready && model.canEditFloor,
                inactive: model.floorTarget.flatMap { target in model.draft.map { !FloorSettings.isActive(target, in: $0.candidate) } } ?? false,
                status: model.floorStatus,
                media: (model.draft?.candidate["mediaAssets"].items ?? []).filter { $0["mime"].string == "audio/wav" }.compactMap { row in row["id"].string.map { ($0, row["path"].string ?? "WAV") } },
                stage: model.stageFloorPreset, reset: model.resetFloorField,
                apply: { Task { await model.applyFloor() } }, discard: model.discardFloor,
                preset: $model.floorPreset, scope: $model.floorScope)
            if !model.floorRuntimeAvailable {
                Text("바닥 재질을 지원하는 제작 서비스를 준비하세요. 기존 설정과 입력은 유지됩니다.").font(.caption).foregroundStyle(.secondary)
            }
        }.task(id: "\(model.viewportAccessID)-\(model.floorRuntimeAvailable)") {
            if ready { await model.loadFloorImages() }
        }
    }
}
