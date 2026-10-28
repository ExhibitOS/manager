// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct ArtworkCatalogView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let ready: Bool
    var compact = false
    var condensed = false
    @State private var expanded = true
    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 6 : 10) {
            HStack(spacing: 8) {
                Button { expanded.toggle() } label: {
                    Label(compact ? "작품" : "작품 카탈로그", systemImage: expanded ? "chevron.down" : "chevron.right")
                }.buttonStyle(.plain).font(.headline).accessibilityLabel("작품 카탈로그").accessibilityValue(expanded ? "펼침" : "접힘")
                if expanded {
                    search
                    if compact {
                        Menu {
                            Picker("작품 필터", selection: $model.catalogFilter) {
                                ForEach(ArtworkCatalogFilter.allCases, id: \.self) { Text($0.label).tag($0) }
                            }.pickerStyle(.inline)
                        } label: { Label(model.catalogFilter.label, systemImage: "line.3.horizontal.decrease") }
                        .menuStyle(.borderlessButton).fixedSize()
                    } else { filters }
                }
            }
            if expanded {
                if model.library.isEmpty {
                    Text("작품 관리에서 작품 정보·파일·권리를 확인하고 승인한 뒤 배치하세요.").foregroundStyle(.secondary)
                } else if model.visibleCatalog.isEmpty {
                    Text("조건에 맞는 작품이 없습니다. 검색어나 필터를 바꿔 보세요.").foregroundStyle(.secondary)
                } else {
                    ScrollView(.horizontal) {
                        LazyHStack(alignment: .top, spacing: 8) {
                            ForEach(model.visibleCatalog) { entry in
                                catalogCard(entry)
                                    .task(id: model.viewportAccessID.uuidString + ":" + String(controller.acceptedSequence) + ":" + model.catalogImageKey(entry)) { await model.loadCatalogImage(entry) }
                            }
                        }.padding(2)
                    }.frame(height: compact ? (condensed ? 88 : 110) : 222)
                }
            }
            if expanded || model.placingArtwork {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 8) { placementActions }
                    VStack(alignment: .leading, spacing: 4) { placementActions }
                }
            }
            if let intent = model.artworkPlacement {
                HStack {
                    Text(intent.targetKind == .wall ? "벽에 배치 미리보기" : "바닥에 배치 미리보기")
                    if model.placementApplying { ProgressView().controlSize(.small) }
                    Spacer(minLength: 0)
                    Button("취소") { Task { await model.cancelVisualArtworkPlacement() } }.disabled(model.placementApplying)
                    Button("여기에 배치") { Task { await model.commitVisualArtworkPlacement() } }.buttonStyle(.borderedProminent)
                        .disabled(model.placementApplying || model.artworkPlacementTarget?.valid != true)
                }
            } else if !model.artworkId.isEmpty {
                HStack { Text("배치할 위치를 확인하세요.").foregroundStyle(.secondary); Button("배치 선택 취소") { model.cancelCatalogPlacement() } }
            }
            if !model.catalogFailures.isEmpty {
                HStack { Text("일부 작품 정보를 읽지 못했습니다.").foregroundStyle(.secondary); Button("정보 다시 확인") { model.retryCatalogMetadata() }.disabled(!ready || model.busy) }
            }
            if !model.catalogStatus.isEmpty { Text(model.catalogStatus).font(.caption).foregroundStyle(.secondary).lineLimit(compact ? 1 : nil) }
            if !model.canSelectCatalog && !model.placingArtwork { Text("입력을 적용하거나 취소한 뒤 다른 작품을 고르세요.").font(.caption).foregroundStyle(.secondary) }
        }
        .controlSize(compact ? .small : .regular)
        .padding(compact ? 8 : 12)
        .background(Color(nsColor: .controlBackgroundColor))
        .onAppear { if condensed { expanded = false } }
        .onChange(of: condensed) { _, value in if value && !model.placingArtwork { expanded = false } }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)) { _ in
            Task { await model.cancelVisualArtworkPlacement() }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didResignKeyNotification)) { _ in
            Task { await model.cancelVisualArtworkPlacement() }
        }
    }
    @ViewBuilder private var placementActions: some View {
        if !model.catalogArtworkId.isEmpty {
            Button(compact ? "장면에 배치" : "3D에서 배치", systemImage: "plus.viewfinder") { Task { await model.beginVisualArtworkPlacement() } }
                .disabled(!ready || !model.canPlaceVisually)
                .help(controller.placementSupported ? "벽이나 바닥을 클릭해 미리 보고 명시적으로 배치합니다." : "이 실행 환경에서는 숫자로 배치할 수 있습니다.")
            Button("숫자로 배치", systemImage: "number") { model.prepareCatalogPlacement() }
                .disabled(!ready || !model.canSelectCatalog || model.immutable)
            Button { Task { await controller.findSelection() } } label: { Label("선택한 배치 찾기", systemImage: "scope") }
                .labelStyle(.iconOnly).disabled(!ready || !model.canSelectCatalog || model.immutable || model.viewportSelection?.kind != "placement" || !controller.findSelectionSupported || model.viewportPresentation == .plan)
        }
        Spacer(minLength: 0)
        if model.libraryCursor != nil { Button("작품 더 보기") { Task { await model.loadMoreCatalog() } }.disabled(!ready || model.busy) }
    }
    private var search: some View {
        TextField("작품명·작가 검색", text: $model.catalogQuery).textFieldStyle(.roundedBorder).frame(minWidth: compact ? 90 : 180)
    }
    private var filters: some View {
        Picker("작품 필터", selection: $model.catalogFilter) {
            ForEach(ArtworkCatalogFilter.allCases, id: \.self) { Text($0.label).tag($0) }
        }.pickerStyle(.segmented).frame(minWidth: compact ? 190 : 280)
    }
    private func catalogCard(_ entry: ArtworkCatalogEntry) -> some View {
        let selected = model.catalogArtworkId == entry.id
        let count = model.draft?.catalogPlacementIDs(for: entry.id).count ?? 0
        let key = model.catalogImageKey(entry)
        let dimensions = entry.dimensions.isEmpty ? "치수 확인 중" : entry.dimensions.map { $0.formatted(.number.precision(.fractionLength(0...3))) }.joined(separator: " × ") + " m"
        return Button { model.selectCatalog(entry.id) } label: {
            VStack(alignment: .leading, spacing: compact ? 2 : 5) {
                ZStack {
                    Color(nsColor: .textBackgroundColor)
                    if let image = model.catalogImages[key] {
                        Image(nsImage: image).resizable().scaledToFit().padding(4)
                    } else {
                        VStack(spacing: 4) {
                            Image(systemName: entry.kind == "sculpture" ? "cube" : "photo").font(.title2)
                            Text(model.catalogPreviewFailures.contains(key) || !controller.thumbnailSupported ? "미리보기 없음 · 배치 가능" : "미리보기 준비 중").font(.caption2)
                        }.foregroundStyle(.secondary)
                    }
                }.frame(height: compact ? (condensed ? 28 : 44) : 98).clipShape(RoundedRectangle(cornerRadius: 4)).accessibilityHidden(true)
                Text(entry.title).font(compact ? .caption.weight(.medium) : .body.weight(.medium)).lineLimit(compact ? 1 : 2).multilineTextAlignment(.leading)
                Text(entry.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                if !compact { Text(entry.typeLabel + " · " + dimensions).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                Label(count == 0 ? "미배치" : "배치 \(count)개", systemImage: selected ? "checkmark.circle.fill" : count == 0 ? "circle" : "square.on.square")
                    .font(.caption).foregroundStyle(selected ? Color.accentColor : Color.secondary)
            }
            .padding(compact ? 5 : 8).frame(width: compact ? 136 : 180, alignment: .topLeading)
            .background(selected ? Color.accentColor.opacity(0.10) : Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 7))
            .overlay(RoundedRectangle(cornerRadius: 7).stroke(selected ? Color.accentColor : Color(nsColor: .separatorColor), lineWidth: selected ? 2 : 1))
        }
        .buttonStyle(.plain)
        .disabled(!ready || !model.canSelectCatalog)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityLabel("\(entry.title), \(entry.artist), \(entry.typeLabel), \(dimensions), \(count == 0 ? "미배치" : "배치 \(count)개")")
        .accessibilityHint("원본 작품을 선택합니다. 배치된 작품이면 같은 배치가 공간과 목록에서 강조됩니다. 시점은 그대로 유지됩니다.")
    }
}
