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
            let roomId = target.kind == .surface
                ? model.draft?.candidate["surfaces"].items?.first { $0["id"].string?.lowercased() == target.id.lowercased() }?["roomId"].string
                : target.id
            let room = model.draft?.rooms.first { $0["id"].string?.lowercased() == roomId?.lowercased() }?["name"].string ?? "공간"
            return (target.key, target.kind == .surface ? room + " · 바닥 \(index + 1)" : room + " · 기본 바닥")
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if model.floor.concreteFootstepsRequested {
                Text("선택한 바닥의 콘크리트 발소리를 미리 확인하고 있습니다. 바닥 외관·다른 소리는 유지합니다.")
                Text("관람객의 소리 끄기·음량 설정이 우선합니다. 여기서는 소리를 자동 재생하지 않습니다.")
                    .font(.caption).foregroundStyle(.secondary)
                Button("콘크리트 발소리 미리보기") { model.previewFloor() }.disabled(!ready || !model.canEditFloor)
                Button("콘크리트 발소리 적용") { Task { await model.applyFloor() } }.disabled(!ready || !model.canApplyFloor)
                Button("발소리 입력 취소") { model.discardFloor() }.disabled(model.busy || model.pending || model.architectureApplying)
                if !model.floorStatus.isEmpty { Text(model.floorStatus).font(.callout).foregroundStyle(.secondary) }
            } else {
            FloorSettingsView(buffer: $model.floor, targets: targets, targetKey: model.floorTargetKey,
                select: model.selectFloor, customized: model.floorCustomized, currentValues: model.floorCurrentValues,
                dirty: model.floorDirty, canApply: ready && model.canApplyFloor, audioResourcesAvailable: model.floorAudioRuntimeAvailable, images: model.floorImages,
                preview: model.previewFloor, canReset: model.floorCanReset, editable: ready && model.canEditFloor,
                inactive: model.floorTarget.flatMap { target in model.draft.map { !FloorSettings.isActive(target, in: $0.candidate) } } ?? false,
                status: model.floorStatus,
                media: (model.draft?.candidate["mediaAssets"].items ?? []).filter { $0["mime"].string == "audio/wav" }.compactMap { row in row["id"].string.map { ($0, row["path"].string ?? "WAV") } },
                stage: model.stageFloorPreset, reset: model.resetFloorField,
                apply: { Task { await model.applyFloor() } }, discard: model.discardFloor,
                preset: $model.floorPreset, scope: $model.floorScope)
            Button("승인된 콘크리트 발소리 입력") { model.stageConcreteFootsteps() }
                .disabled(!ready || !model.canStageConcreteFootsteps)
            Text("선택한 바닥의 발소리를 승인된 녹음으로 바꿉니다. 바닥 외관은 유지합니다. 바닥 재질의 소리 설정이 있으면 변경할 수 없습니다.")
                .font(.caption).foregroundStyle(.secondary)
            }
            if !model.floorRuntimeAvailable {
                Text("바닥 재질을 지원하는 제작 서비스를 준비하세요. 기존 설정과 입력은 유지됩니다.").font(.caption).foregroundStyle(.secondary)
            }
            if model.floorDirty && !model.canEditFloor {
                Button("바닥 입력 되돌리기") { model.discardFloor() }
                    .disabled(model.busy || model.pending || model.architectureApplying || model.placingArtwork)
            }
        }.task(id: "\(model.viewportAccessID)-\(model.floorRuntimeAvailable)") {
            if ready { await model.loadFloorImages() }
        }
    }
}
