// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct SurfaceMaterialSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    private var editable: Bool { ready && !model.busy && !model.pending && !model.immutable && !model.formDirty && !model.positionDirty && !model.architectureGeometryDirty && !model.lightDirty && !model.benchDirty && !model.plaqueDirty && !model.spaceLightingDirty && !model.architectureApplying }
    private func field<Value>(_ key: WritableKeyPath<SurfaceMaterialFormBuffer, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { model.surfaceMaterial?[keyPath: key] ?? fallback }, set: { value in
            guard var fields = model.surfaceMaterial else { return }
            fields[keyPath: key] = value; model.surfaceMaterial = fields
        })
    }
    private var color: Binding<Color> {
        Binding(get: {
            let text = model.surfaceMaterial?.color ?? "#f4f1e8"
            guard text.range(of: "^#[0-9a-fA-F]{6}$", options: .regularExpression) != nil,
                  let rgb = UInt32(text.dropFirst(), radix: 16) else { return Color(nsColor: .controlBackgroundColor) }
            return Color(.sRGB, red: Double((rgb >> 16) & 255) / 255, green: Double((rgb >> 8) & 255) / 255, blue: Double(rgb & 255) / 255, opacity: 1)
        }, set: { value in
            guard let rgb = NSColor(value).usingColorSpace(.sRGB), var fields = model.surfaceMaterial else { return }
            let components = [rgb.redComponent, rgb.greenComponent, rgb.blueComponent].map { Int((min(1, max(0, $0)) * 255).rounded()) }
            fields.color = String(format: "#%02x%02x%02x", components[0], components[1], components[2])
            model.surfaceMaterial = fields
        })
    }
    var body: some View {
        if model.viewportSelection?.kind == "surface" {
            Section("선택한 표면의 재질") {
                if let fields = model.surfaceMaterial {
                    Toggle("이 표면에 재질 지정", isOn: field(\.usesOverride, fallback: false)).disabled(!editable)
                    if fields.usesOverride {
                        Group {
                            ColorPicker("색상", selection: color, supportsOpacity: false)
                            TextField("색상 코드 · #rrggbb", text: field(\.color, fallback: ""))
                            TextField("거칠기 · 0–1", text: field(\.roughness, fallback: ""))
                            TextField("금속성 · 0–1", text: field(\.metalness, fallback: ""))
                        }.disabled(!editable)
                    } else {
                        Text("표면 지정값을 사용하지 않고 기존 기본 재질로 표시합니다.").foregroundStyle(.secondary)
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        Button("재질 미리보기") { Task { await model.previewSurfaceMaterial() } }.disabled(!ready || !model.canPreviewSurfaceMaterial)
                        Button("재질 적용") { Task { await model.applySurfaceMaterial() } }.disabled(!ready || !model.canApplySurfaceMaterial)
                        Button("재질 입력 취소") { model.discardSurfaceMaterial() }.disabled(!model.surfaceMaterialDirty || model.architectureApplying || model.busy)
                    }
                    if model.surfaceMaterialDirty && !model.surfaceMaterialPreviewing {
                        Text("미리보기 후 적용하세요. 적용 전 입력은 아직 보관되지 않았습니다.").foregroundStyle(.secondary)
                    }
                }
                if !model.surfaceMaterialStatus.isEmpty { Text(model.surfaceMaterialStatus).foregroundStyle(.secondary) }
            }
        }
    }
}
