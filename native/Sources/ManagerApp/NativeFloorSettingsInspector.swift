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
            if let selection = model.floor.recordedFootsteps {
                let name = selection == .quietOak ? "목재 발소리" : "콘크리트 발소리"
                Text("선택한 바닥의 " + name + " 적용 전 확인입니다. 바닥 외관과 다른 소리는 유지합니다.")
                Text("관람객의 소리 끄기·음량 설정이 우선합니다. 여기서는 소리를 자동 재생하지 않습니다.")
                    .font(.caption).foregroundStyle(.secondary)
                Button(name + " 미리보기") { model.previewFloor() }.disabled(!ready || !model.canEditFloor)
                Button(name + " 적용") { Task { await model.applyFloor() } }.disabled(!ready || !model.canApplyFloor)
                Button("발소리 입력 취소") { model.discardFloor() }.disabled(model.busy || model.pending || model.architectureApplying)
                if !model.floorStatus.isEmpty { Text(model.floorStatus).font(.callout).foregroundStyle(.secondary) }
            } else {
            FloorSettingsView(buffer: $model.floor, targets: targets, targetKey: model.floorTargetKey,
                select: model.selectFloor, customized: model.floorCustomized, currentValues: model.floorCurrentValues, soundStatus: model.floorSoundStatus,
                dirty: model.floorDirty, canApply: ready && model.canApplyFloor, audioResourcesAvailable: model.floorAudioRuntimeAvailable, images: model.floorImages,
                preview: model.previewFloor, canReset: model.floorCanReset, editable: ready && model.canEditFloor,
                inactive: model.floorTarget.flatMap { target in model.draft.map { !FloorSettings.isActive(target, in: $0.candidate) } } ?? false,
                status: model.floorStatus,
                media: (model.draft?.candidate["mediaAssets"].items ?? []).filter { $0["mime"].string == "audio/wav" }.compactMap { row in row["id"].string.map { ($0, row["path"].string ?? "WAV") } },
                stage: model.stageFloorPreset, reset: model.resetFloorField,
                apply: { Task { await model.applyFloor() } }, discard: model.discardFloor,
                preset: $model.floorPreset, scope: $model.floorScope)
            if model.privateFloorCatalog != nil {
                Button(action: model.stagePrivateFloor) {
                    HStack(spacing: 10) {
                        if let image = model.privateFloorImage {
                            Image(nsImage: image).resizable().scaledToFit().frame(width: 48, height: 48)
                                .accessibilityHidden(true)
                        }
                        Text(NativePrivateFloorCatalog.name + " 기본값 입력")
                    }
                }.disabled(!ready || !model.canStagePrivateFloor)
            } else if model.privateFloorLoadFailed {
                Button("헤링본 재질 목록 다시 확인") { Task { await model.loadPrivateFloorCatalog() } }
            }
            if ready && model.canStageRecordedFootsteps(.quietOak) {
                Button("승인된 목재 발소리 입력") { model.stageRecordedFootsteps(.quietOak) }
            }
            Button("승인된 콘크리트 발소리 입력") { model.stageRecordedFootsteps(.concrete) }
                .disabled(!ready || !model.canStageRecordedFootsteps(.concrete))
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
        }.task(id: "\(model.viewportAccessID)-\(model.privateFloorRuntimeAvailable)") {
            if ready { await model.loadPrivateFloorCatalog() }
        }
    }
}
