// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct WalkingCollisionInspector: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool

    private struct LoadKey: Equatable {
        let access: UUID
        let revision: UInt64?
        let available: Bool
        let retry: UUID
    }

    @State private var targets: [FurnishingWalkingCollisionState] = []
    @State private var loaded: LoadKey?
    @State private var error: String?
    @State private var retry = UUID()

    private var key: LoadKey {
        .init(access: model.viewportAccessID, revision: model.state?.revision,
              available: model.walkingCollisionAvailable, retry: retry)
    }

    var body: some View {
        let request = key
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                Text("걷기 충돌").font(.headline)
                Text("관람자가 걸을 때 배치별로 통과를 막을지 정합니다. 전시를 저장하면 내보내기에 반영됩니다.")
                    .font(.callout).foregroundStyle(.secondary)
                if !request.available {
                    Text("이 작업 공간에서 걷기 충돌 설정을 사용할 수 없습니다.").foregroundStyle(.secondary)
                } else if loaded != request {
                    ProgressView("배치 확인 중…").controlSize(.small)
                } else if let error {
                    Text(error).font(.callout)
                    Button("다시 확인") { retry = UUID() }
                } else if targets.isEmpty {
                    Text("충돌을 설정할 배치가 없습니다.").foregroundStyle(.secondary)
                } else if let revision = request.revision, let draft = model.draft {
                    ForEach(targets) { row in
                        let label = label(for: row.target, in: draft)
                        VStack(alignment: .leading, spacing: 4) {
                            Picker(label.title, selection: Binding(
                                get: { row.setting },
                                set: { setting in
                                    guard setting != row.setting else { return }
                                    Task { await model.setWalkingCollision(row.target, setting: setting, revision: revision) }
                                }
                            )) {
                                Text(row.defaultEnabled ? "기본값 (켜짐)" : "기본값 (꺼짐)").tag(FurnishingWalkingCollisionSetting.inherit)
                                Text("켜기").tag(FurnishingWalkingCollisionSetting.on)
                                Text("끄기").tag(FurnishingWalkingCollisionSetting.off)
                            }
                            .accessibilityIdentifier("walkingCollision." + row.target.kind.rawValue + "." + row.target.id.uuidString.lowercased())
                            .disabled(!ready || !model.canSetWalkingCollision)
                            Text(label.detail).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }.frame(maxWidth: .infinity, alignment: .leading).padding(12)
        }.task(id: request) {
            guard request.available, let revision = request.revision else { return }
            do {
                let rows = try await model.walkingCollisionTargets(revision: revision)
                guard !Task.isCancelled, request == key else { return }
                targets = rows
                error = nil
                loaded = request
            } catch {
                guard !Task.isCancelled, request == key else { return }
                targets = []
                self.error = (error as? AuthoringFailure)?.localizedDescription ?? "배치를 확인하지 못했습니다."
                loaded = request
            }
        }
    }

    private func label(for target: FurnishingWalkingCollisionTarget, in draft: NativeExhibitionDraft) -> (title: String, detail: String) {
        let extensions = draft.candidate["extensions"]
        func find(_ rows: [DocumentValue]?) -> DocumentValue {
            rows?.first(where: { $0["id"].string.flatMap(UUID.init(uuidString:)) == target.id }) ?? .null
        }
        let kind: String
        let row: DocumentValue
        let name: String?
        switch target.kind {
        case .workspace:
            kind = "비품"
            let workspace = extensions[WorkspaceFurnishingPlacement.namespace]
            row = find(workspace["placements"].items)
            name = workspace["furnishings"].items?.first {
                $0["id"] == row["furnishingId"] && $0["version"] == row["furnishingVersion"]
            }?["name"].string
        case .bench:
            kind = "벤치"
            row = find(extensions[BenchSettings.namespace]["benches"].items)
            name = row["name"].string
        case .plinth, .frame:
            kind = target.kind == .plinth ? "받침대" : "프레임"
            row = find(draft.placements)
            name = draft.artworks.first { $0["revisionId"] == row["artworkRevisionId"] }?["title"].string
        case .light:
            kind = "조명"
            row = find(draft.candidate["lights"].items)
            name = row["name"].string
        case .opening:
            kind = "개구부"
            row = find(draft.candidate["openings"].items)
            name = row["name"].string
        case .baseboard:
            kind = "걸레받이"
            row = find(draft.candidate["surfaces"].items)
            name = row["name"].string
        case .sign:
            kind = "안내판"
            row = find(extensions[PlaqueSettings.namespace]["plaques"].items)
            name = row["text"].string
        }
        let room = draft.rooms.first { $0["id"] == row["roomId"] }?["name"].string
        let coordinates = (row["position"].items ?? row["transform"]["position"].items)?.compactMap(\.double)
        let position = coordinates.flatMap { values in
            values.count == 3 ? "(" + values.map { $0.formatted(.number.precision(.fractionLength(0...2))) }.joined(separator: ", ") + ") m" : nil
        }
        let detail = [room, position, String(target.id.uuidString.prefix(8)).lowercased()].compactMap { $0 }.joined(separator: " · ")
        return (name.map { $0.isEmpty ? kind : "\(kind) · \($0)" } ?? kind, detail)
    }
}
