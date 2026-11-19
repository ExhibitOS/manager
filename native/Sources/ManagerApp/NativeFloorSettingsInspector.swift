// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// Both modes share the same buffers and existing central scene.
struct NativeFloorSettingsInspector: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var viewport: NativeViewportController
    let ready: Bool
    @State private var confirmSoundReset = false
    @State private var resetTarget: FloorSettings.Target?

    private func requestSoundReset() {
        guard let target = model.floorTarget else { return }
        resetTarget = target
        confirmSoundReset = true
    }
    private var targets: [(key: String, name: String)] {
        model.floorTargets.enumerated().map { index, target in
            let roomId = target.kind == .surface
                ? model.draft?.candidate["surfaces"].items?.first { $0["id"].string?.lowercased() == target.id.lowercased() }?["roomId"].string
                : target.id
            let room = model.draft?.rooms.first { $0["id"].string?.lowercased() == roomId?.lowercased() }?["name"].string ?? "공간"
            return (target.key, target.kind == .surface ? room + " · 바닥 \(index + 1)" : room + " · 기본 바닥")
        }
    }
    private func recordingLabel(_ recording: RecordedFloorStatus) -> String {
        let room = model.draft?.rooms.first {
            $0["id"].string?.lowercased() == recording.roomId.lowercased()
        }?["name"].string ?? "공간"
        let material = recording.material.map { ["wood": "목재", "marble": "대리석", "tile": "타일", "carpet": "카펫", "concrete": "콘크리트"][$0] ?? $0 }
        let subject = [room, material].compactMap { $0 }.joined(separator: " · ")
        let status: String
        switch recording.state {
        case .recorded:
            status = recording.source == .explicit ? "선택한 녹음 연결" : "승인된 재질 녹음 연결"
        case .muted: status = "발소리 끔"
        case .unassigned: status = "녹음 미지정 · 발소리 없음"
        case .missing: status = "연결된 녹음을 확인할 수 없음 · 발소리 없음"
        }
        return subject + " · " + status
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
            } else if let recording = model.recordedFloorPreview?.recording {
                GroupBox("바닥 재질과 소리") {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(recordingLabel(recording))
                        Text("적용 전에 공간의 미리보기를 확인하세요. 녹음 파일은 내보내기 준비에서 검증합니다.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Button("바닥 설정 적용") { Task { await model.applyFloor() } }
                            .disabled(!ready || !model.canApplyFloor)
                        Button("입력 되돌리기") { model.discardFloor() }
                            .disabled(model.busy || model.pending)
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
            FloorSettingsView(buffer: $model.floor, targets: targets, targetKey: model.floorTargetKey,
                select: model.selectFloor, customized: model.floorCustomized, currentValues: model.floorCurrentValues,
                dirty: model.floorDirty, canApply: ready && model.canApplyFloor, audioResourcesAvailable: model.floorAudioRuntimeAvailable, images: model.floorImages,
                recordedSelectionAvailable: model.recordedFloorSupported,
                canStageRecorded: ready && model.canStageRecordedFloor,
                preview: model.previewFloor, canReset: model.floorCanReset, editable: ready && model.canEditFloor,
                inactive: model.floorTarget.flatMap { target in model.draft.map { !FloorSettings.isActive(target, in: $0.candidate) } } ?? false,
                status: model.floorStatus,
                media: (model.draft?.candidate["mediaAssets"].items ?? []).filter { $0["mime"].string == "audio/wav" }.compactMap { row in row["id"].string.map { ($0, row["path"].string ?? "WAV") } },
                stage: model.stageFloorPreset, reset: { field in
                    if field == .sound && model.recordedFloorSupported { requestSoundReset() }
                    else { model.resetFloorField(field) }
                },
                chooseMaterial: {
                    guard let target = model.floorTarget else { return }
                    Task { await model.stageRecordedFloor(.select(target, model.floorPreset)) }
                },
                apply: { Task { await model.applyFloor() } }, discard: model.discardFloor,
                preset: $model.floorPreset, scope: $model.floorScope)
            if model.recordedFloorSupported {
                if model.floor.settings == nil {
                    Button("현재 재질의 기본 발소리로 재설정", action: requestSoundReset)
                        .disabled(!ready || !model.canStageRecordedFloor)
                }
                Button(model.recordedFloorReadbackPending ? "녹음 확인 중…" : "저장된 발소리 녹음 확인") {
                    Task { await model.checkRecordedFloorReadiness() }
                }.disabled(!ready || !model.canStageRecordedFloor)
                if let status = model.selectedRecordedFloorStatus { Text(recordingLabel(status)) }
            }
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
        }.confirmationDialog("이 바닥의 발소리를 재설정할까요?", isPresented: $confirmSoundReset, titleVisibility: .visible) {
            Button("발소리 재설정") {
                guard let resetTarget else { return }
                Task { await model.stageRecordedFloor(.reset(resetTarget)) }
            }
            Button("취소", role: .cancel) {}
        } message: {
            Text("이 바닥의 개별 녹음과 끄기 설정을 해제합니다. 현재 재질에 승인된 녹음이 연결되어 있으면 사용하고, 없으면 발소리가 나지 않습니다.")
        }.task(id: "\(model.viewportAccessID)-\(model.floorRuntimeAvailable)") {
            if ready { await model.loadFloorImages() }
        }.task(id: "\(model.viewportAccessID)-\(model.privateFloorRuntimeAvailable)") {
            if ready { await model.loadPrivateFloorCatalog() }
        }
    }
}
