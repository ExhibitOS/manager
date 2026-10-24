// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// The owner retains this buffer across previews. Apply creates one native undo
/// step; alignment is a separate explicit command after configuration is applied.
struct InstallationSettingsView: View {
    @Binding var buffer: InstallationFormBuffer
    let placementId: String
    let dirty: Bool
    let editable: Bool
    @ObservedObject var viewport: NativeViewportController
    let canAlign: Bool
    let status: String
    let floors: [(id: String, name: String)]
    let walls: [(id: String, name: String)]
    let apply: () -> Void
    let discard: () -> Void
    let reset: () -> Void
    let align: (InstallationCommand) -> Void
    @State private var advancedPedestal = false
    @State private var advancedFrame = false
    @State private var floor = ""
    @State private var wall = ""
    @State private var installHeight = "1.5"
    @State private var wallGap = "0.02"

    private func field(_ title: String, _ key: InstallationFormBuffer.Field, unit: String = "m") -> some View {
        HStack {
            TextField(title, text: Binding(get: { buffer.fields[key] ?? "" }, set: { buffer.fields[key] = $0 }))
                .accessibilityLabel(title + " · " + (unit == "m" ? "미터" : unit))
            if !unit.isEmpty { Text(unit).foregroundStyle(.secondary) }
        }
    }
    private func number(_ text: String) -> Double? {
        let value = text.replacingOccurrences(of: Locale.current.decimalSeparator ?? ".", with: ".")
        guard let parsed = Double(value.trimmingCharacters(in: .whitespacesAndNewlines)), parsed.isFinite else { return nil }
        return parsed
    }
    private func presetName(_ value: InstallationSettings.Preset) -> String {
        switch value { case .paint: "도장"; case .wood: "목재"; case .stone: "석재"; case .metal: "금속" }
    }
    private func material(frame: Bool) -> some View {
        VStack(alignment: .leading) {
            Picker("재질", selection: Binding(get: { frame ? buffer.framePreset : buffer.pedestalPreset }, set: { buffer.applyMaterialPreset($0, frame: frame) })) {
                ForEach(InstallationSettings.Preset.allCases.filter { !frame || $0 != .stone }, id: \.self) { preset in Text(presetName(preset)).tag(preset) }
            }
            TextField("색상 · #RRGGBB", text: frame ? $buffer.frameColor : $buffer.pedestalColor)
            Text("프리셋은 색과 표면 반사 기본값을 적용합니다.").font(.caption).foregroundStyle(.secondary)
            DisclosureGroup("상세 재질", isExpanded: frame ? $advancedFrame : $advancedPedestal) {
                field("거칠기 · 0–1", frame ? .frameRoughness : .pedestalRoughness, unit: "")
                field("금속성 · 0–1", frame ? .frameMetalness : .pedestalMetalness, unit: "")
            }
        }
    }
    var body: some View {
        GroupBox("전시 설치") {
            VStack(alignment: .leading, spacing: 12) {
                Text("작품 원본과 실측 치수는 유지됩니다. 받침이 포함된 모델에는 중복으로 적용하지 마세요.")
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                GroupBox("조각 받침") {
                    VStack(alignment: .leading) {
                        Picker("형태", selection: $buffer.pedestalKind) {
                            Text("없음").tag(InstallationSettings.Pedestal.Kind.none)
                            Text("직육면체").tag(InstallationSettings.Pedestal.Kind.box)
                            Text("원기둥").tag(InstallationSettings.Pedestal.Kind.cylinder)
                        }
                        if buffer.pedestalKind != .none {
                            if buffer.pedestalKind == .box { field("폭", .width); field("깊이", .depth) }
                            else { field("지름", .diameter) }
                            field("높이", .height)
                            Text("받침 바닥 중심 위치 · 방 좌표").font(.caption)
                            field("X", .x); field("Y", .y); field("Z", .z)
                            field("회전 · −180–180", .yawDegrees, unit: "°")
                            material(frame: false)
                            HStack {
                                Button("작품 중앙 정렬") { align(.centerArtwork(placementId: placementId)) }
                                Button("받침 위에 작품 배치") { align(.onTop(placementId: placementId)) }
                            }.disabled(!viewport.connected || !canAlign || dirty)
                        }
                    }.padding(4)
                }
                GroupBox("회화 액자") {
                    VStack(alignment: .leading) {
                        Picker("형태", selection: $buffer.frameKind) {
                            Text("없음").tag(InstallationSettings.Frame.Kind.none)
                            Text("평면형").tag(InstallationSettings.Frame.Kind.flat)
                            Text("단차형").tag(InstallationSettings.Frame.Kind.stepped)
                            Text("플로팅형").tag(InstallationSettings.Frame.Kind.floating)
                        }
                        if buffer.frameKind != .none {
                            field("테두리 폭", .borderWidth); field("두께", .thickness); field("깊이", .frameDepth)
                            if buffer.frameKind == .floating { field("작품과 액자 간격", .gap) }
                            field("매트 폭 · 0이면 없음", .matWidth)
                            TextField("매트 색상 · #RRGGBB", text: $buffer.matColor)
                            material(frame: true)
                        }
                    }.padding(4)
                }
                if !status.isEmpty { Text(status).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                HStack {
                    Button("설정 적용", action: apply).disabled(!dirty)
                    Button("입력 되돌리기", action: discard).disabled(!dirty)
                    Button("설치 설정 초기화", action: reset)
                }
                Text("설정을 먼저 적용하면 작품 정렬을 사용할 수 있습니다. 각 조작은 실행 취소할 수 있습니다.")
                    .font(.caption).foregroundStyle(.secondary)
                DisclosureGroup("설치 높이와 벽 간격") {
                    VStack(alignment: .leading) {
                        Picker("기준 바닥", selection: $floor) {
                            Text("작품 아래의 유일한 바닥").tag("")
                            ForEach(floors, id: \.id) { Text($0.name).tag($0.id) }
                        }
                        TextField("작품 중심 높이 · m", text: $installHeight)
                        Button("설치 높이 적용") {
                            if let height = number(installHeight) { align(.installHeight(placementId: placementId, height: height, floorSurfaceId: floor.isEmpty ? nil : floor)) }
                        }.disabled(!viewport.connected || !canAlign || dirty || number(installHeight).map { !(0...10).contains($0) } != false)
                        Picker("기준 벽", selection: $wall) {
                            Text("벽 선택 필요").tag("")
                            ForEach(walls, id: \.id) { Text($0.name).tag($0.id) }
                        }
                        TextField("벽과의 간격 · m", text: $wallGap)
                        Button("벽 간격 적용") {
                            if let gap = number(wallGap) { align(.wallGap(placementId: placementId, gap: gap, wallSurfaceId: wall)) }
                        }.disabled(!viewport.connected || !canAlign || dirty || !walls.contains(where: { $0.id == wall }) || number(wallGap).map { !(0...1).contains($0) } != false)
                        Text("높이는 작품 중심 기준입니다. 벽 간격은 선택한 벽과 평행한 작품에 적용됩니다.").font(.caption).foregroundStyle(.secondary)
                    }
                }
            }.padding(6).disabled(!editable)
        }
    }
}
