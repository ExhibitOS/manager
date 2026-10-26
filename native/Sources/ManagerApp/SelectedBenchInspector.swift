// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct NativeBenchPicker: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    var body: some View {
        HStack {
            Picker("벤치 선택", selection: Binding(get: { model.selectedBenchId }, set: { model.selectBench($0) })) {
                Text("벤치를 선택하세요").tag("")
                ForEach(model.benches, id: \.id) { Text($0.name).tag($0.id) }
                if model.benchAdding { Text("새 벤치 입력 중").tag(model.selectedBenchId) }
            }.disabled(!ready || !model.canSelectBench)
                .accessibilityHint("입력을 적용하거나 취소한 뒤 다른 벤치를 선택하세요.")
            Button("벤치 추가", systemImage: "plus") { model.beginBench() }.disabled(!ready || !model.canAddBench)
        }
        if !model.benchMetadataEditable {
            Text("이 전시의 벤치·감상 시점 설정은 현재 편집할 수 없습니다. 저장된 정보는 유지됩니다.").font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// Only disclosure visibility is local. All text, even incomplete numbers, lives
/// in the same model buffer across mode changes and hidden inspector panels.
struct SelectedBenchInspector: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let ready: Bool
    let mode: Mode
    @State private var positionDetails = false
    @State private var materialDetails = false
    @State private var cameraDetails = false
    private var editable: Bool { ready && model.canEditBench && !model.benchRemoving }
    private func field<Value>(_ key: WritableKeyPath<BenchFormBuffer, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { model.bench?[keyPath: key] ?? fallback }, set: { value in
            guard var fields = model.bench else { return }
            fields[keyPath: key] = value; model.bench = fields
        })
    }
    private func text(_ label: String, _ key: WritableKeyPath<BenchFormBuffer, String>) -> some View {
        TextField(label, text: field(key, fallback: ""))
    }
    private func presetName(_ preset: InstallationSettings.Preset) -> String {
        switch preset { case .paint: "도장"; case .wood: "목재"; case .stone: "석재"; case .metal: "금속" }
    }
    private var color: Binding<Color> {
        Binding(get: {
            let value = model.bench?.color ?? ""
            guard value.range(of: "^#[0-9a-fA-F]{6}$", options: .regularExpression) != nil,
                  let rgb = UInt32(value.dropFirst(), radix: 16) else { return Color(nsColor: .controlBackgroundColor) }
            return Color(.sRGB, red: Double((rgb >> 16) & 255) / 255, green: Double((rgb >> 8) & 255) / 255, blue: Double(rgb & 255) / 255, opacity: 1)
        }, set: { value in
            guard let rgb = NSColor(value).usingColorSpace(.sRGB), var fields = model.bench else { return }
            let components = [rgb.redComponent, rgb.greenComponent, rgb.blueComponent].map { Int((min(1, max(0, $0)) * 255).rounded()) }
            fields.color = String(format: "#%02x%02x%02x", components[0], components[1], components[2]); model.bench = fields
        })
    }
    private var positionFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("공간 안의 바닥 접촉 중심 · 미터").font(.caption).foregroundStyle(.secondary)
            text("가로 위치 X · m", \.x)
            text("높이 위치 Y · m", \.y)
            text("깊이 위치 Z · m", \.z)
            text("방향 · −180–180°", \.yawDegrees)
        }
    }
    private var materialFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            text("색상 코드 · sRGB #rrggbb", \.color)
            text("거칠기 · 0–1", \.roughness)
            text("금속성 · 0–1", \.metalness)
        }
    }
    private var cameraFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("벤치가 속한 공간 기준 · 미터").font(.caption).foregroundStyle(.secondary)
            text("감상 시점 이름", \.viewpointName)
            text("눈 위치 X · m", \.viewX)
            text("눈 위치 Y · m", \.viewY)
            text("눈 위치 Z · m", \.viewZ)
            text("바라볼 위치 X · m", \.targetX)
            text("바라볼 위치 Y · m", \.targetY)
            text("바라볼 위치 Z · m", \.targetZ)
            text("시야각 · 10–120°", \.fov)
        }
    }
    @ViewBuilder private func viewpoint(_ fields: BenchFormBuffer) -> some View {
        Toggle("새 감상 시점 만들기", isOn: field(\.usesFreshViewpoint, fallback: true))
        if fields.usesFreshViewpoint {
            Text("벤치를 적용할 때 새 시점도 함께 만듭니다. 기존 시점은 그대로 남습니다.").font(.caption).foregroundStyle(.secondary)
            Button("벤치 기준으로 시점 맞추기") { model.matchBenchViewpoint() }
                .help("눈을 좌석 위로 맞추고 벤치 방향을 바라보게 합니다. 미리보기에서 확인한 뒤 적용하세요.")
            if mode == .expert { GroupBox("감상 시점 수치") { cameraFields.padding(4) } }
            else { DisclosureGroup("감상 시점 다듬기", isExpanded: $cameraDetails) { cameraFields.padding(.top, 6) } }
        } else {
            let views = model.draft.flatMap { try? BenchViewpoint.list(in: $0) } ?? []
            Picker("기존 감상 시점", selection: field(\.viewpointId, fallback: "")) {
                Text("같은 공간의 시점을 선택하세요").tag("")
                ForEach(views.filter { $0.roomId.lowercased() == fields.roomId.lowercased() }, id: \.id) { Text($0.name).tag($0.id) }
            }
            Text("기존 시점의 위치·방향은 바꾸지 않습니다. 변경하려면 새 시점을 만들어 연결하세요.").font(.caption).foregroundStyle(.secondary)
        }
    }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(model.benchAdding ? "새 벤치" : "선택한 벤치").font(.headline).accessibilityAddTraits(.isHeader)
                if let fields = model.bench {
                    VStack(alignment: .leading, spacing: 12) {
                        text("벤치 이름", \.name)
                        Picker("공간", selection: field(\.roomId, fallback: "")) {
                            let rooms = model.draft?.rooms ?? []
                            ForEach(rooms.indices, id: \.self) { index in Text(rooms[index]["name"].string ?? "공간").tag(rooms[index]["id"].string ?? "") }
                        }
                        Picker("형태", selection: field(\.kind, fallback: .slab)) {
                            Text("등받이 없음").tag(BenchSettings.Kind.slab)
                            Text("등받이 있음").tag(BenchSettings.Kind.backed)
                        }
                        text("가로 · 0.6–3 m", \.width)
                        text("깊이 · 0.3–1 m", \.depth)
                        text("좌석 높이 · 0.3–0.65 m", \.height)
                        Text("높이는 좌석 윗면 기준입니다. 등받이는 좌석보다 0.4 m 더 높습니다.").font(.caption).foregroundStyle(.secondary)
                        Picker("재질", selection: Binding(get: { fields.materialPreset }, set: { preset in
                            guard var next = model.bench else { return }; next.applyMaterialPreset(preset); model.bench = next
                        })) { ForEach(InstallationSettings.Preset.allCases, id: \.self) { Text(presetName($0)).tag($0) } }
                        Text("프리셋은 색상·거칠기·금속성의 기본값을 함께 적용합니다.").font(.caption).foregroundStyle(.secondary)
                        ColorPicker("벤치 색상", selection: color, supportsOpacity: false)
                        if mode == .expert {
                            GroupBox("위치와 방향") { positionFields.padding(4) }
                            GroupBox("재질 수치") { materialFields.padding(4) }
                        } else {
                            DisclosureGroup("위치와 방향", isExpanded: $positionDetails) { positionFields.padding(.top, 6) }
                            DisclosureGroup("재질 수치", isExpanded: $materialDetails) { materialFields.padding(.top, 6) }
                        }
                        Divider()
                        Text("앉아서 보는 시점").font(.headline)
                        viewpoint(fields)
                    }.textFieldStyle(.roundedBorder).disabled(!editable)
                    Text("눈은 좌석 안쪽 위에 있어야 합니다. 미리보기는 저장 형식을 확인하며, 관람 중 앉기는 바닥·장애물·머리 공간을 다시 확인합니다.").font(.caption).foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 8) {
                        Button(model.benchRemoving ? "삭제 미리보기 다시 확인" : "벤치 미리보기") { Task { await model.previewBench() } }.disabled(!ready || !model.canPreviewBench)
                        Button(model.benchRemoving ? "벤치 삭제 적용" : "벤치 적용") { Task { await model.applyBench() } }.disabled(!ready || !model.canApplyBench)
                        Button("벤치 입력 취소") { model.discardBench() }.disabled(!model.benchDirty || model.busy || model.pending || model.architectureApplying)
                        if !model.benchAdding {
                            Button("벤치 삭제 미리보기", role: .destructive) { Task { await model.previewBenchRemoval() } }.disabled(!ready || !model.canRemoveBench)
                            Text("벤치를 삭제해도 감상 시점은 남습니다. 적용한 삭제는 되돌리기로 취소할 수 있습니다.").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    if model.benchDirty && !model.benchPreviewing { Text("입력은 아직 보관되지 않았습니다. 미리보기 후 적용하세요.").font(.caption).foregroundStyle(.secondary) }
                }
                if !controller.benchInspectorSupported { Text("벤치 편집을 지원하는 제작 서비스에 연결하세요. 저장된 설정은 유지됩니다.").font(.caption).foregroundStyle(.secondary) }
                if !model.benchStatus.isEmpty { Text(model.benchStatus).foregroundStyle(.secondary) }
            }.padding(16)
        }.inspectorColumnWidth(min: 280, ideal: 340, max: 440)
    }
}
