// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import CoreGraphics
import ManagerCore

extension NativeLightType {
    var title: String {
        switch self { case .point: "점광원"; case .spot: "스폿"; case .directional: "방향광"; case .area: "면광원" }
    }
}

struct NativeLightPicker: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    private var lights: [DocumentValue] { model.draft?.candidate["lights"].items ?? [] }
    private func title(_ row: DocumentValue, index: Int) -> String {
        let kind = row["type"].string.flatMap(NativeLightType.init(rawValue:))?.title ?? "조명"
        let room = model.draft?.rooms.first { $0["id"] == row["roomId"] }?["name"].string ?? "공간"
        return "\(room) · \(kind) \(index + 1)"
    }
    var body: some View {
        if !lights.isEmpty {
            Picker("조명 선택", selection: Binding(get: { model.selectedLightId }, set: { model.selectLight($0) })) {
                Text("조명을 선택하세요").tag("")
                ForEach(lights.indices, id: \.self) { index in Text(title(lights[index], index: index)).tag(lights[index]["id"].string ?? "") }
            }.disabled(!ready || !model.canSelectLight)
                .accessibilityHint("선택한 조명의 설정을 속성 패널에 표시합니다. 입력을 적용하거나 취소한 뒤 다른 대상을 선택하세요.")
        }
    }
}

/// Text belongs to ExhibitionModel; hiding the panel or changing modes cannot
/// reset it. Only disclosure visibility is local presentation state.
struct SelectedLightInspector: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let ready: Bool
    let mode: Mode
    @State private var positionDetails = false
    @State private var colorDetails = false
    private var editable: Bool {
        ready && controller.connected && controller.lightInspectorSupported && !model.busy && !model.pending && !model.immutable && !model.placingArtwork && !model.formDirty && !model.positionDirty && !model.architectureGeometryDirty && !model.surfaceMaterialDirty && !model.benchDirty && !model.plaqueDirty && !model.architectureApplying
    }
    private var selected: DocumentValue? {
        model.draft?.candidate["lights"].items?.first { $0["id"].string == model.selectedLightId }
    }
    private func field(_ key: WritableKeyPath<LightFormBuffer, String>) -> Binding<String> {
        Binding(get: { model.light?[keyPath: key] ?? "" }, set: { value in
            guard var fields = model.light else { return }
            fields[keyPath: key] = value; model.light = fields
        })
    }
    private var color: Binding<Color> {
        Binding(get: {
            guard let fields = model.light, let r = Double(fields.red), let g = Double(fields.green), let b = Double(fields.blue),
                  [r, g, b].allSatisfy({ $0.isFinite && (0...1).contains($0) }) else { return Color(nsColor: .controlBackgroundColor) }
            return Color(.sRGBLinear, red: r, green: g, blue: b, opacity: 1)
        }, set: { value in
            guard let linear = CGColorSpace(name: CGColorSpace.extendedLinearSRGB),
                  let space = NSColorSpace(cgColorSpace: linear),
                  let rgb = NSColor(value).usingColorSpace(space), var fields = model.light else { return }
            fields.red = String(Double(min(1, max(0, rgb.redComponent))))
            fields.green = String(Double(min(1, max(0, rgb.greenComponent))))
            fields.blue = String(Double(min(1, max(0, rgb.blueComponent))))
            model.light = fields
        })
    }
    private var positionFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("조명이 속한 공간 기준 · 미터").font(.caption).foregroundStyle(.secondary)
            TextField("가로 위치 X · m", text: field(\.x))
            TextField("높이 위치 Y · m", text: field(\.y))
            TextField("깊이 위치 Z · m", text: field(\.z))
        }
    }
    private var colorFields: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("저장된 색상은 선형 RGB입니다. 각 값은 0–1입니다.").font(.caption).foregroundStyle(.secondary)
            TextField("선형 빨강 R", text: field(\.red))
            TextField("선형 초록 G", text: field(\.green))
            TextField("선형 파랑 B", text: field(\.blue))
        }
    }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("선택한 조명").font(.headline).accessibilityAddTraits(.isHeader)
                if let row = selected {
                    let room = model.draft?.rooms.first { $0["id"] == row["roomId"] }?["name"].string ?? "공간"
                    Text(room).font(.title3.weight(.semibold))
                    if let fields = model.light {
                        VStack(alignment: .leading, spacing: 12) {
                            Picker("빛의 종류", selection: Binding(get: { fields.type }, set: { value in
                                guard var next = model.light else { return }; next.changeType(value); model.light = next
                            })) { ForEach(NativeLightType.allCases, id: \.self) { Text($0.title).tag($0) } }
                            Text("빛의 계산 방식을 선택합니다. 종류를 바꿔 단위가 달라지면 밝기는 기본값 100으로 바뀝니다.").font(.caption).foregroundStyle(.secondary)
                            ColorPicker("빛 색상", selection: color, supportsOpacity: false)
                            TextField("밝기 · 렌더러 값", text: field(\.intensity))
                            if fields.type == .spot { TextField("빔 전체각 · °", text: field(\.beamDegrees)) }
                            if fields.type == .area {
                                TextField("광원 가로 · m", text: field(\.width))
                                TextField("광원 세로 · m", text: field(\.height))
                            }
                            if mode == .expert {
                                GroupBox("정확한 위치") { positionFields.padding(4) }
                                GroupBox("색상 수치") { colorFields.padding(4) }
                            } else {
                                DisclosureGroup("위치를 숫자로 다듬기", isExpanded: $positionDetails) { positionFields.padding(.top, 6) }
                                DisclosureGroup("색상 수치", isExpanded: $colorDetails) { colorFields.padding(.top, 6) }
                            }
                        }.textFieldStyle(.roundedBorder).disabled(!editable)
                        if mode == .expert { LabeledContent("저장 단위", value: fields.type.unit) }
                        Text("밝기와 색상은 화면 표현을 위한 입력입니다. 실제 조도 측정값은 아닙니다.").font(.caption).foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 8) {
                            Button("조명 미리보기") { Task { await model.previewLight() } }.disabled(!ready || !model.canPreviewLight)
                            Button("조명 적용") { Task { await model.applyLight() } }.disabled(!ready || !model.canApplyLight)
                            Button("조명 입력 취소") { model.discardLight() }.disabled(!model.lightDirty || model.busy || model.pending || model.architectureApplying)
                        }
                        if model.lightDirty && !model.lightPreviewing { Text("미리보기 후 적용하세요. 적용 전 입력은 아직 보관되지 않았습니다.").font(.caption).foregroundStyle(.secondary) }
                    }
                    if !controller.lightInspectorSupported { Text("현재 연결에서는 조명을 확인할 수 있습니다. 편집 가능한 제작 서비스에 다시 연결하세요.").font(.caption).foregroundStyle(.secondary) }
                } else { Text("조명 목록에서 대상을 선택하세요.").foregroundStyle(.secondary) }
                if !model.lightStatus.isEmpty { Text(model.lightStatus).foregroundStyle(.secondary) }
            }.padding(16)
        }.inspectorColumnWidth(min: 280, ideal: 320, max: 420)
    }
}
