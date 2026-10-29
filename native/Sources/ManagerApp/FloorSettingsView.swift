// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct FloorSettingsView: View {
    @Binding var buffer: FloorFormBuffer
    let targets: [(key: String, name: String)]
    let targetKey: String
    let select: (String) -> Void
    let customized: Set<FloorSettings.Field>
    let currentValues: [FloorSettings.Field: DocumentValue]
    let dirty: Bool
    let canApply: Bool
    let audioResourcesAvailable: Bool
    let images: [FloorSettings.Preset: NSImage]
    let preview: () -> Void
    let canReset: Bool
    let editable: Bool
    let inactive: Bool
    let status: String
    let media: [(id: String, name: String)]
    let stage: (FloorSettings.Preset, Set<FloorSettings.Field>) -> Void
    let reset: (FloorSettings.Field) -> Void
    let apply: () -> Void
    let discard: () -> Void
    @Binding var preset: FloorSettings.Preset
    @Binding var scope: Set<FloorSettings.Field>

    static func name(_ field: FloorSettings.Field) -> String {
        switch field {
        case .color: "색상"; case .roughness: "거칠기"; case .metalness: "금속성"
        case .repeatMeters: "무늬 반복 크기"; case .rotation: "무늬 회전"
        case .texturePack: "재질 이미지"; case .normalScale: "표면 요철"; case .aoStrength: "틈새 음영"; case .sound: "발소리"
        }
    }
    private func presetName(_ value: FloorSettings.Preset) -> String {
        switch value { case .wood: "목재"; case .marble: "대리석"; case .tile: "타일"; case .carpet: "카펫" }
    }
    private func describe(_ value: DocumentValue?, field: FloorSettings.Field, preset: FloorSettings.Preset?) -> String {
        guard let value, value != .null else { return field == .sound ? "기존 소리 유지" : "기존 값" }
        if field == .texturePack {
            let id = value["id"].string ?? ""
            return ["WoodFloor051": "목재", "Marble006": "대리석", "Tiles074": "타일", "Carpet004": "카펫"][id] ?? "기존 재질 이미지"
        }
        if field == .repeatMeters, let values = value.items { return values.compactMap(\.double).map { String($0) }.joined(separator: " × ") + " m" }
        if field == .sound {
            let gain = value["gain"].double.map { " · 크기 " + String($0) } ?? ""
            switch value["mode"].string {
            case "none": return "소리 없음" + gain
            case "explicit": return "WAV \(value["assetIds"].items?.count ?? 0)개" + gain
            case "material-default": return preset.map { presetName($0) + " 기본 소리" + gain } ?? "재질 기본 소리" + gain
            default:
                let family = value["material"].string ?? ""
                let name = ["wood": "목재", "stone": "석재", "concrete": "콘크리트", "carpet": "카펫", "metal": "금속"][family] ?? "기존"
                return name + " 발소리" + (value["assetId"].string == nil ? "" : " · 선택한 녹음")
            }
        }
        return value.string ?? value.double.map { String($0) } ?? "기존 값"
    }
    private func number(_ label: String, _ key: FloorFormBuffer.Numeric) -> some View {
        TextField(label, text: Binding(get: { buffer.numbers[key] ?? "" }, set: { buffer.numbers[key] = $0 }))
    }
    private func resetButton(_ field: FloorSettings.Field) -> some View {
        Button("현재 기준 재질 기본값", action: { reset(field) }).disabled(!canReset).help(Self.name(field) + "만 현재 바닥 재질의 기본값으로 바꿉니다.")
    }
    var body: some View {
        GroupBox("바닥 재질과 소리") {
            VStack(alignment: .leading, spacing: 12) {
                Picker("바닥", selection: Binding(get: { targetKey }, set: select)) {
                    ForEach(targets, id: \.key) { Text($0.name).tag($0.key) }
                }.disabled(dirty)
                if inactive { Text("이 공간에 직접 만든 바닥이 있어 공간 기본 바닥 설정은 보관만 됩니다.").font(.caption).foregroundStyle(.secondary) }
                ForEach(FloorSettings.Preset.allCases, id: \.self) { value in
                    Button { preset = value } label: {
                        HStack(spacing: 10) {
                            Group {
                                if let image = images[value] { Image(nsImage: image).resizable().scaledToFit() }
                                else { Image(systemName: "photo").foregroundStyle(.secondary) }
                            }.frame(width: 48, height: 48).accessibilityHidden(true)
                            Text(presetName(value)).foregroundStyle(.primary).frame(maxWidth: .infinity, alignment: .leading)
                            if preset == value { Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.accentColor).accessibilityHidden(true) }
                        }.padding(8).contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityLabel(presetName(value)).accessibilityValue(preset == value ? "선택됨" : "선택 안 됨")
                }
                Text("변경할 항목을 선택하세요. 선택만으로 설정을 바꾸거나 소리를 재생하지 않습니다.").font(.caption).foregroundStyle(.secondary)
                ForEach(FloorSettings.Field.allCases, id: \.self) { field in
                    Toggle(isOn: Binding(get: { scope.contains(field) }, set: { if $0 { scope.insert(field) } else { scope.remove(field) } })) {
                        Text(Self.name(field) + (customized.contains(field) ? " · 맞춤 설정" : " · 기본값"))
                    }
                }
                GroupBox("적용 전 변경 확인") {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(FloorSettings.Field.allCases.filter { scope.contains($0) }, id: \.self) { field in
                            Text(Self.name(field) + ": " + describe(currentValues[field], field: field, preset: buffer.settings?.preset)
                                + " → " + describe(FloorSettings.defaults(preset)[field], field: field, preset: preset))
                                .font(.caption).fixedSize(horizontal: false, vertical: true)
                        }
                        if scope.isEmpty { Text("변경할 항목을 선택하세요.").font(.caption) }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
                if buffer.settings != nil && !scope.contains(.sound) {
                    Text("선택하지 않은 소리는 유지합니다. 기본 소리 종류가 바뀌는 경우 기존 기준 재질을 유지하고 외관 값을 따로 지정합니다.").font(.caption).foregroundStyle(.secondary)
                }
                Button("선택 항목에 재질 기본값 입력") { stage(preset, scope) }.disabled(scope.isEmpty)
                Text("이미지는 무늬를 고르기 위한 미리보기입니다. 대리석·카펫의 1 m는 편집 시작값이며 실측 크기가 아닙니다.").font(.caption).foregroundStyle(.secondary)
                if let settings = buffer.settings, !buffer.restoreLegacy {
                    Text("현재 기준 재질: " + presetName(settings.preset)).font(.caption).foregroundStyle(.secondary)
                    if !canReset { Text("재질 선택을 먼저 적용하면 각 항목을 초기화할 수 있습니다.").font(.caption).foregroundStyle(.secondary) }
                    DisclosureGroup("외관 세부 조정") {
                        VStack(alignment: .leading) {
                            VStack(alignment: .leading) { TextField("색상 · #rrggbb", text: $buffer.color); resetButton(.color) }
                            VStack(alignment: .leading) { number("거칠기 · 0–1", .roughness); resetButton(.roughness) }
                            VStack(alignment: .leading) { number("금속성 · 0–1", .metalness); resetButton(.metalness) }
                            VStack(alignment: .leading) { number("가로 반복 · m", .repeatU); number("세로 반복 · m", .repeatV); resetButton(.repeatMeters) }
                            VStack(alignment: .leading) { number("회전 · 라디안", .rotation); resetButton(.rotation) }
                            VStack(alignment: .leading) { number("요철 · 0–2", .normalScale); resetButton(.normalScale) }
                            VStack(alignment: .leading) { number("음영 · 0–1", .aoStrength); resetButton(.aoStrength) }
                            Text("빈 항목은 기존 재질 값을 따릅니다. 편집한 항목에는 유효한 값을 입력하세요.").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Picker("발소리", selection: Binding(get: { buffer.soundMode ?? "preserve" }, set: {
                        buffer.selectSoundMode($0 == "preserve" ? nil : $0)
                    })) {
                        if buffer.settings?.value["sound"] == .null { Text("기존 소리 유지").tag("preserve") }
                        Text("재질 기본 소리").tag("material-default")
                        Text("WAV 직접 선택").tag("explicit")
                        Text("없음").tag("none")
                    }
                    if buffer.soundMode == "explicit" {
                        ForEach(media, id: \.id) { asset in
                            Toggle(asset.name, isOn: Binding(get: { buffer.assetIds.contains(asset.id) }, set: { selected in
                                buffer.assetIds.removeAll { $0 == asset.id }; if selected { buffer.assetIds.append(asset.id) }
                            }))
                        }
                        if media.isEmpty { Text("이 전시에 선택 가능한 WAV가 없습니다.").font(.caption) }
                    }
                    if buffer.soundMode == "material-default" {
                        Button("이 재질의 승인된 발소리 사용") { buffer.selectSoundMode("material-default") }
                            .disabled(!audioResourcesAvailable)
                        if !audioResourcesAvailable { Text("승인된 발소리를 지원하는 제작 서비스를 준비한 뒤 선택할 수 있습니다.").font(.caption).foregroundStyle(.secondary) }
                    }
                    if buffer.soundMode != nil { number("발소리 크기 · 0–1", .gain) }
                    resetButton(.sound)
                    Button("소리 들어보기") {}.disabled(true)
                    Text("관람객의 소리 끄기 설정이 우선합니다. 공간 잔향은 별도 설정입니다. 미리 듣기는 소리 묶음과 관람 동의가 준비된 뒤 사용할 수 있습니다.").font(.caption).foregroundStyle(.secondary)
                }
                if !status.isEmpty { Text(status).font(.caption).textSelection(.enabled) }
                VStack(alignment: .leading, spacing: 8) {
                    Button("미리보기 확인", action: preview).disabled(!dirty)
                    Button("바닥 설정 적용", action: apply).disabled(!canApply)
                    Button("입력 되돌리기", action: discard).disabled(!dirty)
                }
                Button("바닥 설정 제거 · 이전 설정으로 복귀") { buffer.restoreLegacy = true }
                    .disabled(buffer.settings == nil)
                if buffer.restoreLegacy { Text("적용하면 바닥 설정을 제거하고 기존 재질과 소리 설정으로 돌아갑니다.").font(.caption) }
            }.padding(6).disabled(!editable)
        }
    }
}
