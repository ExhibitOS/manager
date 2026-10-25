// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct ArchitectureSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    private struct Target: Identifiable {
        let selection: ViewportSelection
        let title: String
        var id: String { selection.kind + ":" + selection.id }
    }
    private var targets: [Target] {
        guard let draft = model.draft else { return [] }
        return [("room", "rooms"), ("surface", "surfaces"), ("opening", "openings")].flatMap { kind, key in
            (draft.candidate[key].items ?? []).enumerated().compactMap { index, row -> Target? in
                guard let id = row["id"].string else { return nil }
                let prefix: String
                if kind == "room" { prefix = row["name"].string ?? "공간 \(index + 1)" }
                else {
                    let names = ["wall": "벽", "floor": "바닥", "ceiling": "천장", "door": "문", "window": "창문"]
                    prefix = (names[row["type"].string ?? ""] ?? (kind == "surface" ? "표면" : "개구부")) + " \(index + 1)"
                }
                return Target(selection: ViewportSelection(kind: kind, id: id), title: prefix)
            }
        }
    }
    private var editable: Bool { ready && !model.busy && !model.pending && !model.immutable && !model.formDirty && !model.positionDirty && !model.architectureApplying }
    private func field(_ key: WritableKeyPath<ArchitectureFormBuffer, String>) -> Binding<String> {
        Binding(get: { model.architecture?[keyPath: key] ?? "" }, set: { value in
            guard var fields = model.architecture else { return }
            fields[keyPath: key] = value; model.architecture = fields
        })
    }
    var body: some View {
        Form {
            Section("공간·벽·문 편집") {
                Picker("편집 대상", selection: Binding<String?>(get: {
                    guard let selected = model.viewportSelection else { return nil }
                    return targets.first(where: { $0.selection == selected })?.id
                }, set: { id in
                    if let target = targets.first(where: { $0.id == id }) { model.selectViewport(target.selection) }
                })) {
                    Text("대상을 선택하세요").tag(nil as String?)
                    ForEach(targets) { target in Text(target.title).tag(Optional(target.id)) }
                }.disabled(!editable || model.architectureDirty)
                if let fields = model.architecture {
                    Group {
                        if fields.selection.kind == "room" { TextField("공간 이름", text: field(\.name)) }
                        TextField("가로 · m", text: field(\.width))
                        TextField("높이 · m", text: field(\.height))
                        if fields.selection.kind == "room" { TextField("깊이 · m", text: field(\.depth)) }
                        if fields.selection.kind == "opening" {
                            TextField("벽 기준 가로 위치 · m", text: field(\.x))
                            TextField("벽 기준 세로 위치 · m", text: field(\.y))
                        } else {
                            Text(fields.selection.kind == "room" ? "공간 위치 · 전시 좌표" : "표면 위치 · 공간 좌표").foregroundStyle(.secondary)
                            TextField("가로 위치 · m", text: field(\.x))
                            TextField("높이 위치 · m", text: field(\.y))
                            TextField("깊이 위치 · m", text: field(\.z))
                        }
                    }.disabled(!editable)
                    if fields.selection.kind == "room" {
                        Text("공간의 기준 치수만 바뀝니다. 벽과 작품은 자동으로 늘어나거나 이동하지 않습니다.").foregroundStyle(.secondary)
                    }
                    HStack {
                        Button("공간 변경 적용") { Task { await model.applyArchitecture() } }.disabled(!ready || !model.canApplyArchitecture)
                        Button("입력 취소") { model.discardArchitecture() }.disabled(!model.architectureDirty || model.architectureApplying)
                    }
                    if model.architectureDirty { Text("적용하거나 입력을 취소한 뒤 다른 대상을 선택하세요. 적용 전 입력은 아직 보관되지 않았습니다.").foregroundStyle(.secondary) }
                    Text("회전·배율·연결 정보와 재질은 현재 설정을 유지합니다. 적용한 변경은 되돌리기로 취소할 수 있습니다.").foregroundStyle(.secondary)
                }
                if !model.architectureStatus.isEmpty { Text(model.architectureStatus).foregroundStyle(.secondary) }
            }
        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true)
    }
}
