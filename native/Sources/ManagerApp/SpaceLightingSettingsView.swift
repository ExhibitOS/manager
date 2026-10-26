// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

extension NativeSpaceLightingPreset {
    var title: String { self == .calm ? "차분하게" : "밝게" }
}

struct SpaceLightingSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    let mode: Mode
    @State private var reflectionDetails = false
    private func field<Value>(_ key: WritableKeyPath<SpaceLightingFormBuffer, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { model.spaceLighting?[keyPath: key] ?? fallback }, set: { value in
            guard var fields = model.spaceLighting else { return }
            fields[keyPath: key] = value; model.spaceLighting = fields
        })
    }
    private var reflection: some View {
        VStack(alignment: .leading, spacing: 8) {
            TextField("환경 반사 · 0–1", text: field(\.reflectionIntensity, fallback: ""))
            Text("표면의 환경 반사 강도입니다. 실제 간접광 계산은 아닙니다.").font(.caption).foregroundStyle(.secondary)
        }
    }
    var body: some View {
        Section("전시 조명") {
            if let fields = model.spaceLighting {
                Group {
                    HStack {
                        ForEach(NativeSpaceLightingPreset.allCases, id: \.self) { preset in
                            Button(preset.title) {
                                guard var fields = model.spaceLighting else { return }
                                fields.selectPreset(preset); model.spaceLighting = fields
                            }
                        }
                    }
                    LabeledContent("설정", value: fields.usesOverride ? (fields.preset?.title ?? "직접 설정") : "기존 표시 유지")
                    Toggle("기본 밝기 직접 설정", isOn: field(\.usesOverride, fallback: false))
                    if fields.usesOverride {
                        TextField("전시 기본 밝기 · 0–1", text: field(\.baselineIntensity, fallback: ""))
                        Text("전시 전체의 약한 기본광을 조절합니다. 개별 조명과 채광은 별도로 설정합니다.").font(.caption).foregroundStyle(.secondary)
                        if mode == .expert { reflection }
                        else { DisclosureGroup("환경 반사", isExpanded: $reflectionDetails) { reflection } }
                        if fields.baselineIntensity.isEmpty || fields.reflectionIntensity.isEmpty {
                            Text("처음 설정할 때는 기본 밝기와 환경 반사를 모두 입력하세요. 자동으로 값을 지정하지 않습니다.").font(.caption).foregroundStyle(.secondary)
                        }
                    } else {
                        Text("기존 표시 유지").foregroundStyle(.secondary)
                        Text("적용하면 직접 설정한 두 값을 해제하고 기존 조명 표시를 사용합니다.").font(.caption).foregroundStyle(.secondary)
                    }
                }.disabled(!ready || !model.canEditSpaceLighting)
                HStack {
                    Button("전시 조명 미리보기") { Task { await model.previewSpaceLighting() } }.disabled(!ready || !model.canPreviewSpaceLighting)
                    Button("전시 조명 적용") { Task { await model.applySpaceLighting() } }.disabled(!ready || !model.canApplySpaceLighting)
                    Button("전시 조명 입력 취소") { model.discardSpaceLighting() }.disabled(!model.spaceLightingDirty || model.busy || model.pending || model.architectureApplying)
                }
            }
            if !model.viewportController.spaceSettingsSupported {
                Text("현재 편집 화면은 전시 기본 밝기 설정을 지원하지 않습니다. 저장된 설정은 유지됩니다.").font(.caption).foregroundStyle(.secondary)
            }
            if !model.spaceLightingStatus.isEmpty { Text(model.spaceLightingStatus).foregroundStyle(.secondary) }
        }
    }
}
