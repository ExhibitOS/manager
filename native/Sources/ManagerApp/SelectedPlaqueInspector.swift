// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct NativePlaquePicker: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    var body: some View {
        HStack {
            Picker("명판 선택", selection: Binding(get: { model.selectedPlaqueId }, set: { model.selectPlaque($0) })) {
                Text("명판을 선택하세요").tag("")
                let rows = model.plaques
                ForEach(rows.indices, id: \.self) { index in Text("\(model.plaqueSourceTitle(rows[index])) · 명판 \(index + 1)").tag(rows[index].id) }
                if model.plaqueAdding { Text("새 명판 입력 중").tag(model.selectedPlaqueId) }
            }.disabled(!ready || !model.canSelectPlaque)
                .accessibilityHint("입력을 적용하거나 취소한 뒤 다른 명판을 선택하세요.")
            Button("명판 추가", systemImage: "plus") { model.beginPlaque() }.disabled(!ready || !model.canAddPlaque)
        }
        if !model.plaqueMetadataEditable {
            Text("이 전시의 명판 설정은 현재 편집할 수 없습니다. 저장된 정보는 유지됩니다.").font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// All source references and raw text are model-owned. Only disclosures are
/// local presentation state; changing mode or hiding this panel preserves input.
struct SelectedPlaqueInspector: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let ready: Bool
    let mode: Mode
    @State private var positionDetails = false
    @State private var rotationDetails = false
    private var editable: Bool { ready && model.canEditPlaque && !model.plaqueRemoving }
    private func field<Value>(_ key: WritableKeyPath<PlaqueFormBuffer, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { model.plaque?[keyPath: key] ?? fallback }, set: { value in
            guard var fields = model.plaque else { return }; fields[keyPath: key] = value; model.plaque = fields
        })
    }
    private func text(_ label: String, _ key: WritableKeyPath<PlaqueFormBuffer, String>) -> some View {
        TextField(label, text: field(key, fallback: ""))
    }
    private var roomSelection: Binding<String> {
        Binding(get: {
            let id = model.plaque?.roomId ?? ""
            return model.draft?.rooms.first(where: { $0["id"].string?.lowercased() == id.lowercased() })?["id"].string ?? id
        }, set: { value in guard var fields = model.plaque else { return }; fields.roomId = value; model.plaque = fields })
    }
    @ViewBuilder private func sourceFields(_ fields: PlaqueFormBuffer) -> some View {
        Picker("표시할 내용", selection: field(\.sourceKind, fallback: .exhibition)) {
            Text("전시 제목·소개").tag(PlaqueFormBuffer.SourceKind.exhibition)
            Text("공간 이름").tag(PlaqueFormBuffer.SourceKind.room)
            Text("작품 캡션").tag(PlaqueFormBuffer.SourceKind.artwork)
        }
        if fields.sourceKind == .artwork {
            let rows = (model.draft?.placements ?? []).filter { $0["roomId"].string?.lowercased() == fields.roomId.lowercased() }
            Picker("작품 배치", selection: Binding(get: {
                rows.first(where: { $0["id"].string?.lowercased() == fields.placementId.lowercased() })?["id"].string ?? fields.placementId
            }, set: { value in guard var next = model.plaque else { return }; next.placementId = value; model.plaque = next })) {
                Text("같은 공간의 작품 배치를 선택하세요").tag("")
                ForEach(rows.indices, id: \.self) { index in
                    let id = rows[index]["id"].string ?? ""
                    Text("\(model.plaquePlacementTitle(id)) · 배치 \(index + 1)").tag(id)
                }
                if !fields.placementId.isEmpty && !rows.contains(where: { $0["id"].string?.lowercased() == fields.placementId.lowercased() }) {
                    Text("이전 배치 · 같은 공간의 대상을 다시 선택하세요").tag(fields.placementId)
                }
            }
            if rows.isEmpty { Text("이 공간에 배치한 작품이 없습니다. 기존 작품 배치를 만든 뒤 선택하세요.").font(.caption).foregroundStyle(.secondary) }
        }
        Text("내용은 기존 전시·공간·작품 정보에서 가져옵니다. 문안은 원래 편집 화면에서 수정하세요.").font(.caption).foregroundStyle(.secondary)
    }
    private var positionFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("명판 중심 · 선택한 공간 기준 미터").font(.caption).foregroundStyle(.secondary)
            text("가로 위치 X · m", \.x)
            text("높이 위치 Y · m", \.y)
            text("깊이 위치 Z · m", \.z)
        }
    }
    private func facingTitle(_ direction: PlaqueFormBuffer.Facing) -> String {
        switch direction { case .forward: "정면 · +Z"; case .back: "뒤쪽 · −Z"; case .left: "왼쪽 · −X"; case .right: "오른쪽 · +X" }
    }
    private var rotationFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("명판 앞면은 +Z 방향입니다. 단위 쿼터니언의 네 성분을 입력합니다. 길이는 1이어야 합니다.").font(.caption).foregroundStyle(.secondary)
            text("회전 X", \.rotationX)
            text("회전 Y", \.rotationY)
            text("회전 Z", \.rotationZ)
            text("회전 W", \.rotationW)
        }
    }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(model.plaqueAdding ? "새 명판" : "선택한 명판").font(.headline).accessibilityAddTraits(.isHeader)
                if let fields = model.plaque {
                    VStack(alignment: .leading, spacing: 12) {
                        Picker("명판이 놓일 공간", selection: roomSelection) {
                            let rooms = model.draft?.rooms ?? []
                            ForEach(rooms.indices, id: \.self) { index in Text(rooms[index]["name"].string ?? "공간").tag(rooms[index]["id"].string ?? "") }
                        }
                        sourceFields(fields)
                        Toggle("명판 표시", isOn: field(\.visible, fallback: true))
                        Text("숨긴 명판도 설정과 참조를 보관합니다.").font(.caption).foregroundStyle(.secondary)
                        text("가로 · 0.2–2 m", \.width)
                        text("세로 · 0.15–1.2 m", \.height)
                        Menu("명판 앞면 방향") {
                            ForEach(PlaqueFormBuffer.Facing.allCases, id: \.self) { direction in
                                Button(facingTitle(direction)) {
                                    guard var next = model.plaque else { return }; next.face(direction); model.plaque = next
                                }
                            }
                        }
                        if mode == .expert {
                            GroupBox("위치") { positionFields.padding(4) }
                            GroupBox("방향 수치") { rotationFields.padding(4) }
                        } else {
                            DisclosureGroup("위치 다듬기", isExpanded: $positionDetails) { positionFields.padding(.top, 6) }
                            DisclosureGroup("방향 수치", isExpanded: $rotationDetails) { rotationFields.padding(.top, 6) }
                        }
                    }.textFieldStyle(.roundedBorder).disabled(!editable)
                    Text("명판 전체가 공간 안에 있어야 합니다. 미리보기에서 위치·방향을 확인하고 적용하세요.").font(.caption).foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 8) {
                        Button(model.plaqueRemoving ? "삭제 미리보기 다시 확인" : "명판 미리보기") { Task { await model.previewPlaque() } }.disabled(!ready || !model.canPreviewPlaque)
                        Button(model.plaqueRemoving ? "명판 삭제 적용" : "명판 적용") { Task { await model.applyPlaque() } }.disabled(!ready || !model.canApplyPlaque)
                        Button("명판 입력 취소") { model.discardPlaque() }.disabled(!model.plaqueDirty || model.busy || model.pending || model.architectureApplying)
                        if !model.plaqueAdding {
                            Button("명판 삭제 미리보기", role: .destructive) { Task { await model.previewPlaqueRemoval() } }.disabled(!ready || !model.canRemovePlaque)
                            Text("명판 삭제는 원래 전시·공간·작품의 내용을 바꾸지 않습니다. 적용한 삭제는 되돌리기로 취소할 수 있습니다.").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    if model.plaqueDirty && !model.plaquePreviewing { Text("입력은 아직 보관되지 않았습니다. 미리보기 후 적용하세요.").font(.caption).foregroundStyle(.secondary) }
                }
                if !controller.plaqueInspectorSupported { Text("명판 편집을 지원하는 제작 서비스에 연결하세요. 저장된 설정은 유지됩니다.").font(.caption).foregroundStyle(.secondary) }
                if !model.plaqueStatus.isEmpty { Text(model.plaqueStatus).foregroundStyle(.secondary) }
            }.padding(16)
        }.inspectorColumnWidth(min: 280, ideal: 330, max: 440)
    }
}
