// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

/// Presentation of shared, validated catalogue records. The caller owns the
/// existing authenticated image loading/cache and the model's staged command.
struct NativeMaterialCatalogRow: Identifiable {
    let catalogId: String
    let name: String
    let recommendedRepeat: Double
    var id: String { catalogId }
}

struct SurfaceMaterialCatalogView: View {
    let entries: [NativeMaterialCatalogRow]
    let images: [String: NSImage]
    @Binding var fields: SurfaceMaterialTextureFormBuffer
    let mode: Mode
    let supported: Bool
    let editable: Bool
    let loading: Bool
    let status: String
    let select: (String) -> Void
    let remove: () -> Void
    let useRecommendedValues: () -> Void
    let retry: () -> Void
    @State private var details = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("재질 고르기").font(.headline)
            if !supported {
                Text("이 제작 서비스에서는 무늬가 있는 재질을 지원하지 않습니다. 기존 색상과 표면 설정은 유지됩니다.").foregroundStyle(.secondary)
            } else {
                if loading { ProgressView("재질을 불러오고 있습니다…").controlSize(.small) }
                ForEach(entries) { entry in
                    Button { select(entry.catalogId) } label: {
                        HStack(spacing: 10) {
                            Group {
                                if let image = images[entry.id] { Image(nsImage: image).resizable().scaledToFit() }
                                else { Image(systemName: "photo").foregroundStyle(.secondary) }
                            }.frame(width: 48, height: 48).accessibilityHidden(true)
                            VStack(alignment: .leading, spacing: 4) {
                                Text(entry.name).font(.body).foregroundStyle(.primary)
                                Text("권장 반복 \(entry.recommendedRepeat.formatted()) m").font(.caption).foregroundStyle(.secondary)
                            }.frame(maxWidth: .infinity, alignment: .leading)
                            if fields.catalogId == entry.id { Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.accentColor).accessibilityHidden(true) }
                        }.padding(8).contentShape(Rectangle())
                            .background(fields.catalogId == entry.id ? Color.accentColor.opacity(0.12) : Color.clear)
                    }.buttonStyle(.plain).disabled(!editable)
                        .accessibilityLabel(entry.name)
                        .accessibilityValue(fields.catalogId == entry.id ? "선택됨" : "선택 안 됨")
                }
                if entries.isEmpty && !loading { Button("재질 다시 불러오기", action: retry).disabled(!editable) }
                if !entries.isEmpty {
                    Text("재질을 고른 뒤 ‘재질 미리보기’로 공간에서 확인하세요.").font(.caption).foregroundStyle(.secondary)
                }
                if fields.catalogId != nil {
                    Button("권장값 사용", action: useRecommendedValues).disabled(!editable)
                    DisclosureGroup("무늬 반복·방향", isExpanded: $details) {
                        VStack(alignment: .leading, spacing: 8) {
                            TextField("반복 가로 · m", text: $fields.repeatU)
                            TextField("반복 세로 · m", text: $fields.repeatV)
                            TextField("무늬 회전 · °", text: $fields.rotationDegrees)
                            TextField("요철 강도 · 0–2", text: $fields.normalScale)
                            Text("요철은 빛 반응으로 표시하며 실제 표면 형상을 바꾸지 않습니다.").font(.caption).foregroundStyle(.secondary)
                        }.disabled(!editable)
                    }
                    Button("무늬 재질 해제", action: remove).disabled(!editable)
                }
            }
            if !status.isEmpty { Text(status).font(.callout).foregroundStyle(.secondary) }
        }
        .onAppear { if mode == .expert { details = true } }
        .onChange(of: mode) { _, value in if value == .expert { details = true } }
    }
}
