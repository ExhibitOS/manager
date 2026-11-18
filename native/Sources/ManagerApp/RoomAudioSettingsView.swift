// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore
import UniformTypeIdentifiers

struct RoomAudioSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    @State private var settings = RoomAudioSettings()
    @State private var baseline = RoomAudioSettings()
    @State private var tracks: [ApprovedRoomAudioTrack] = []
    @State private var loaded: Target?
    @State private var loading = false
    @State private var message = ""
    @State private var choosingAudio = false
    @State private var readingAudio = false
    @State private var audioFile: ImportFileSnapshot?
    @State private var audioTarget: Target?
    @State private var audioRequestId: String?
    @State private var audioReceipt: NativeRoomAudioReceipt?
    @State private var audioRights = ArtworkMetadata().rights


    private struct Target: Equatable {
        let exhibitionId: String
        let workspaceId: String
        let tenantId: String
        let principalId: String
        let roomId: String
        let revision: UInt64
    }
    private struct LoadKey: Equatable {
        let target: Target?
        let canEdit: Bool
    }

    private var target: Target? {
        guard ready, let state = model.state, let draft = state.draft,
              draft.rooms.contains(where: { $0["id"].string == model.roomId }) else { return nil }
        return Target(exhibitionId: draft.id, workspaceId: state.workspaceId,
                      tenantId: state.tenantId, principalId: state.principalId,
                      roomId: model.roomId, revision: state.revision)
    }

    private var trackAvailable: Bool {
        guard case .asset(let id) = settings.bgm else { return true }
        return tracks.contains { $0.id.lowercased() == id }
    }

    private var loadKey: LoadKey { LoadKey(target: target, canEdit: model.canEditRoomAudio) }

    var body: some View {
        Form {
            audioIntake
            Section("공간 소리") {
                Picker("공간", selection: $model.roomId) {
                    ForEach(model.draft?.rooms.compactMap(\.roomAudioChoice) ?? []) { room in
                        Text(room.name).tag(room.id)
                    }
                }
                .disabled(!ready || model.busy || model.formDirty || model.architectureDirty || settings != baseline)

                if loading { ProgressView("승인한 오디오 확인 중") }
                if !message.isEmpty { Text(message).foregroundStyle(.secondary) }
                if !model.canEditRoomAudio && !model.busy {
                    Text("현재 전시를 저장한 뒤 공간 소리를 적용할 수 있습니다.")
                        .foregroundStyle(.secondary)
                }
                Group {
                    Picker("배경 음악", selection: $settings.bgm) {
                        Text("기본값").tag(RoomAudioSettings.Track.inherit)
                        Text("없음").tag(RoomAudioSettings.Track.noTrack)
                        if case .asset(let id) = settings.bgm, !trackAvailable {
                            Text("현재 트랙 · 선택 불가").tag(RoomAudioSettings.Track.asset(id))
                        }
                        ForEach(tracks) { track in
                            Text("\(track.creditLine) · \(track.id.prefix(8))").tag(RoomAudioSettings.Track.asset(track.id.lowercased()))
                        }
                    }
                    defaultPicker("첫 방문 배경 음악", selection: $settings.bgmOn)
                    defaultPicker("첫 방문 발소리", selection: $settings.footstepsOn)
                    defaultPicker("첫 방문 공간 잔향", selection: $settings.reverbOn)
                    Button("공간 소리 적용") {
                        guard let target else { return }
                        let input = settings
                        Task { await model.applyRoomAudio(input, roomId: target.roomId, revision: target.revision) }
                    }
                    .disabled(settings == baseline || !trackAvailable)
                    if settings != baseline {
                        Button("입력 취소") { settings = baseline }
                    }
                }
                .disabled(!model.canEditRoomAudio || loading || loaded == nil || loaded != target)
            }
        }
        .formStyle(.grouped)
        .fixedSize(horizontal: false, vertical: true)
        .task(id: loadKey) { await load(loadKey) }
    }

    @ViewBuilder private var audioIntake: some View {
        if model.canIntakeRoomAudio || audioFile != nil || readingAudio {
            Section("오디오 가져오기") {
                Button("MP3 또는 WAV 선택") { choosingAudio = true }
                    .disabled(!model.canIntakeRoomAudio || readingAudio || audioRequestId != nil)
                if readingAudio { ProgressView("오디오 파일 읽는 중") }
                if let audioFile {
                    Text(audioFile.name)
                    Group {
                        TextField("권리자", text: $audioRights.holder)
                        TextField("라이선스", text: Binding(
                            get: { audioRights.licenseId ?? "" },
                            set: { audioRights.licenseId = $0.isEmpty ? nil : $0 }))
                        TextField("크레디트", text: $audioRights.creditLine)
                        Toggle("전시·내보내기 권한 확인", isOn: $audioRights.permissions.export)
                            .toggleStyle(.checkbox)
                    }
                    .disabled(audioRequestId != nil || !model.canIntakeRoomAudio)
                    if let audioReceipt {
                        LabeledContent("오디오 ID", value: audioReceipt.id)
                        if let seconds = audioReceipt.value["durationSeconds"].double {
                            LabeledContent("길이", value: "\(seconds.formatted(.number.precision(.fractionLength(2))))초")
                        }
                        Button("이 오디오 승인") {
                            guard let audioTarget else { return }
                            Task {
                                if await model.approveRoomAudio(audioReceipt, revision: audioTarget.revision) {
                                    self.audioFile = nil
                                    self.audioTarget = nil
                                    audioRequestId = nil
                                    self.audioReceipt = nil
                                    await load(loadKey)
                                    message = "오디오를 승인했습니다."
                                }
                            }
                        }
                        .disabled(!model.canIntakeRoomAudio || audioTarget != target)
                    } else {
                        Button(audioRequestId == nil ? "오디오 가져오기" : "같은 요청 다시 확인") {
                            guard let audioTarget else { return }
                            let isFirstAttempt = audioRequestId == nil
                            let id = audioRequestId ?? UUID().uuidString.lowercased()
                            audioRequestId = id
                            let rights = audioRights
                            Task {
                                audioReceipt = await model.intakeRoomAudio(audioFile, requestId: id, rights: rights,
                                                                          revision: audioTarget.revision)
                                if isFirstAttempt, audioReceipt == nil,
                                   ["AUDIO_SOURCE_INVALID", "AUDIO_DECLARATION_INVALID"].contains(model.failure?.code ?? "") {
                                    message = model.failure?.localizedDescription ?? ""
                                    audioRequestId = nil
                                }
                            }
                        }
                        .disabled(!model.canIntakeRoomAudio || audioTarget != target
                                  || audioRights.holder.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                                  || audioRights.licenseId?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty != false
                                  || !audioRights.permissions.export)
                    }
                    if audioTarget != target {
                        Text("파일을 선택한 전시의 같은 저장본에서 이 요청을 이어갈 수 있습니다.")
                        Button("오디오 입력 닫기") {
                            self.audioFile = nil
                            audioTarget = nil
                            audioRequestId = nil
                            audioReceipt = nil
                        }
                        .disabled(model.busy)
                    }
                }
            }
            .fileImporter(isPresented: $choosingAudio, allowedContentTypes: [.mp3, .wav]) { result in
                guard let target, model.canIntakeRoomAudio else { return }
                readingAudio = true
                Task {
                    defer { readingAudio = false }
                    do {
                        let file = try await ImportFileSnapshot.load(result.get(), byteLimit: 12 * 1024 * 1024,
                                                                     allowWAV: true, allowMP3: true)
                        guard self.target == target else { return }
                        audioFile = file
                        audioTarget = target
                        audioRights = ArtworkMetadata().rights
                        audioRights.licenseText = nil
                    } catch {
                        message = "선택한 MP3 또는 WAV 파일을 읽을 수 없습니다."
                    }
                }
            }
        }
    }

    private func defaultPicker(_ title: String, selection: Binding<Bool?>) -> some View {
        Picker(title, selection: selection) {
            Text("기본값").tag(nil as Bool?)
            Text("켬").tag(true as Bool?)
            Text("끔").tag(false as Bool?)
        }
    }

    private func load(_ key: LoadKey) async {
        guard !Task.isCancelled, key == loadKey else { return }
        message = ""
        loading = false
        guard let target = key.target, let draft = model.draft else {
            loaded = nil
            tracks = []
            return
        }
        do {
            if loaded != target {
                loaded = nil
                tracks = []
                let current = try RoomAudioSettings.read(in: draft, roomId: target.roomId)
                settings = current
                baseline = current
                loaded = target
            }
            guard key.canEdit else { return }
            loading = true
            let approved = try await model.roomAudioTracks()
            guard !Task.isCancelled, key == loadKey else { return }
            tracks = approved
        } catch {
            guard !Task.isCancelled, key == loadKey else { return }
            message = (error as? AuthoringFailure)?.errorDescription ?? "공간 소리를 불러오지 못했습니다."
        }
        if !Task.isCancelled, key == loadKey { loading = false }
    }
}

private struct RoomAudioChoice: Identifiable {
    let id: String
    let name: String
}

private extension DocumentValue {
    var roomAudioChoice: RoomAudioChoice? {
        guard let id = self["id"].string else { return nil }
        return RoomAudioChoice(id: id, name: self["name"].string ?? "공간")
    }
}
