// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct RoomAudioSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    @State private var settings = RoomAudioSettings()
    @State private var baseline = RoomAudioSettings()
    @State private var tracks: [ApprovedRoomAudioTrack] = []
    @State private var loaded: Target?
    @State private var loading = false
    @State private var message = ""

    private struct Target: Equatable {
        let exhibitionId: String
        let workspaceId: String
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
                      roomId: model.roomId, revision: state.revision)
    }

    private var trackAvailable: Bool {
        guard case .asset(let id) = settings.bgm else { return true }
        return tracks.contains { $0.id.lowercased() == id }
    }

    private var loadKey: LoadKey { LoadKey(target: target, canEdit: model.canEditRoomAudio) }

    var body: some View {
        Form {
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
