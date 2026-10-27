// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct SurfaceMaterialSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var viewport: NativeViewportController
    let ready: Bool
    let mode: Mode
    private var editable: Bool { ready && model.canEditSurfaceMaterial }
    private var texture: Binding<SurfaceMaterialTextureFormBuffer> {
        Binding(get: { model.surfaceMaterialTexture ?? .init(binding: nil) }, set: { value in
            guard editable, model.surfaceMaterialTexturesSupported else { return }
            model.surfaceMaterialTexture = value
        })
    }
    private func field<Value>(_ key: WritableKeyPath<SurfaceMaterialFormBuffer, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { model.surfaceMaterial?[keyPath: key] ?? fallback }, set: { value in
            guard editable, var fields = model.surfaceMaterial else { return }
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
            guard editable, let rgb = NSColor(value).usingColorSpace(.sRGB), var fields = model.surfaceMaterial else { return }
            let components = [rgb.redComponent, rgb.greenComponent, rgb.blueComponent].map { Int((min(1, max(0, $0)) * 255).rounded()) }
            fields.color = String(format: "#%02x%02x%02x", components[0], components[1], components[2])
            model.surfaceMaterial = fields
        })
    }
    var body: some View {
        if model.viewportSelection?.kind == "surface" {
            Section("선택한 표면의 재질") {
                if let fields = model.surfaceMaterial {
                    if model.surfaceMaterialTexture != nil {
                        SurfaceMaterialCatalogView(entries: model.surfaceMaterialCatalog?.items.map {
                            .init(catalogId: $0.id, name: $0.name, recommendedRepeat: $0.texture.repeatMeters[0])
                        } ?? [], images: [:], fields: texture, mode: mode, supported: model.surfaceMaterialTexturesSupported,
                           editable: editable, loading: model.surfaceMaterialCatalogLoading, status: model.surfaceMaterialCatalogStatus,
                           select: model.selectSurfaceMaterial, remove: model.removeSurfaceMaterialTexture,
                           useRecommendedValues: model.resetSurfaceMaterialTextureValues,
                           retry: { Task { await model.retrySurfaceMaterialCatalog() } })
                    }
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
            .task(id: "\(model.viewportAccessID)-\(ready)-\(viewport.connected)-\(viewport.surfaceMaterialTexturesSupported)-\(model.surfaceMaterial?.surfaceId ?? "")") {
                if ready { await model.loadSurfaceMaterialCatalog() }
            }
        }
    }
}
