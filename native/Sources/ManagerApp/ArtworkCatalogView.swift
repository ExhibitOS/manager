// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct ArtworkCatalogView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var controller: NativeViewportController
    let ready: Bool
    @State private var expanded = true
    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 10) {
                ViewThatFits(in: .horizontal) {
                    HStack { search; filters }
                    VStack(alignment: .leading) { search; filters }
                }
                if model.library.isEmpty {
                    ContentUnavailableView("승인한 작품이 없습니다", systemImage: "photo.on.rectangle.angled", description: Text("Studio 제작에서 작품을 가져오고 확인한 뒤 배치하세요."))
                } else if model.visibleCatalog.isEmpty {
                    Text("조건에 맞는 작품이 없습니다. 검색어나 필터를 바꿔 보세요.").foregroundStyle(.secondary)
                } else {
                    ScrollView(.horizontal) {
                        LazyHStack(alignment: .top, spacing: 12) {
                            ForEach(model.visibleCatalog) { entry in
                                catalogCard(entry)
                                    .task(id: model.viewportAccessID.uuidString + ":" + String(controller.acceptedSequence) + ":" + model.catalogImageKey(entry)) { await model.loadCatalogImage(entry) }
                            }
                        }.padding(4)
                    }.frame(minHeight: 200, idealHeight: 222)
                }
                HStack(spacing: 12) {
                    if !model.catalogArtworkId.isEmpty {
                        Text("원본 작품 선택").font(.caption).foregroundStyle(.secondary)
                        Button("3D에서 배치", systemImage: "plus.viewfinder") { Task { await model.beginVisualArtworkPlacement() } }
                            .disabled(!ready || !model.canPlaceVisually)
                            .help(controller.placementSupported ? "벽이나 바닥을 클릭해 미리 보고 명시적으로 배치합니다." : "이 실행 환경에서는 숫자로 배치할 수 있습니다.")
                        Button("숫자로 배치", systemImage: "number") { model.prepareCatalogPlacement() }
                            .disabled(!ready || !model.canSelectCatalog || model.immutable)
                        Button("선택한 배치 찾기", systemImage: "scope") { Task { await controller.findSelection() } }
                            .disabled(!ready || !model.canSelectCatalog || model.immutable || model.viewportSelection?.kind != "placement" || !controller.findSelectionSupported || model.viewportPresentation == .plan)
                            .help(controller.findSelectionSupported ? "선택한 배치가 보이도록 시점을 맞춥니다. 원본과 배치 위치는 바뀌지 않습니다." : "이 실행 환경에서는 찾기를 사용할 수 없습니다.")
                    }
                    Spacer(minLength: 0)
                    if model.libraryCursor != nil { Button("작품 더 보기") { Task { await model.loadMoreCatalog() } }.disabled(!ready || model.busy) }
                }
                if !model.catalogFailures.isEmpty {
                    HStack {
                        Text("일부 작품 정보를 읽지 못했습니다. 작품 선택은 계속할 수 있습니다.").font(.callout).foregroundStyle(.secondary)
                        Button("정보 다시 확인") { model.retryCatalogMetadata() }.disabled(!ready || model.busy)
                    }
                }
                if let intent = model.artworkPlacement {
                    HStack {
                        Label(intent.targetKind == .wall ? "벽에 배치 미리보기" : "바닥에 배치 미리보기", systemImage: "viewfinder")
                        if model.placementApplying { ProgressView().controlSize(.small).accessibilityLabel("배치를 확인하고 있습니다") }
                        Spacer(minLength: 0)
                        Button("취소") { Task { await model.cancelVisualArtworkPlacement() } }
                            .disabled(model.placementApplying)
                        Button("여기에 배치") { Task { await model.commitVisualArtworkPlacement() } }
                            .buttonStyle(.borderedProminent)
                            .disabled(model.placementApplying || model.artworkPlacementTarget?.valid != true)
                            .accessibilityHint("미리 보는 위치에 작품 한 개를 추가합니다. 되돌리기로 취소할 수 있습니다.")
                    }
                } else if !model.artworkId.isEmpty {
                    HStack {
                        Text("배치할 위치를 확인하세요.").foregroundStyle(.secondary)
                        Button("배치 선택 취소") { model.cancelCatalogPlacement() }
                    }
                }
                if !model.catalogStatus.isEmpty { Text(model.catalogStatus).font(.callout).foregroundStyle(.secondary) }
                if !model.canSelectCatalog && !model.placingArtwork { Text("미적용 입력은 보존됩니다. 적용하거나 취소한 뒤 다른 작품을 고르세요.").font(.callout).foregroundStyle(.secondary) }
            }.padding(.top, 8)
        } label: {
            Label("작품 카탈로그", systemImage: "square.grid.2x2")
                .font(.headline)
        }
        .padding(12)
        .background(.background, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color(nsColor: .separatorColor)))
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)) { _ in
            Task { await model.cancelVisualArtworkPlacement() }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didResignKeyNotification)) { _ in
            Task { await model.cancelVisualArtworkPlacement() }
        }
    }
    private var search: some View {
        TextField("작품명·작가 검색", text: $model.catalogQuery).textFieldStyle(.roundedBorder).frame(minWidth: 180)
    }
    private var filters: some View {
        Picker("작품 필터", selection: $model.catalogFilter) {
            ForEach(ArtworkCatalogFilter.allCases, id: \.self) { Text($0.label).tag($0) }
        }.pickerStyle(.segmented).frame(minWidth: 280)
    }
    private func catalogCard(_ entry: ArtworkCatalogEntry) -> some View {
        let selected = model.catalogArtworkId == entry.id
        let count = model.draft?.catalogPlacementIDs(for: entry.id).count ?? 0
        let key = model.catalogImageKey(entry)
        let dimensions = entry.dimensions.isEmpty ? "치수 확인 중" : entry.dimensions.map { $0.formatted(.number.precision(.fractionLength(0...3))) }.joined(separator: " × ") + " m"
        return Button { model.selectCatalog(entry.id) } label: {
            VStack(alignment: .leading, spacing: 5) {
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
                }.frame(height: 98).clipShape(RoundedRectangle(cornerRadius: 4)).accessibilityHidden(true)
                Text(entry.title).font(.body.weight(.medium)).lineLimit(2).multilineTextAlignment(.leading)
                Text(entry.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Text(entry.typeLabel + " · " + dimensions)
                    .font(.caption).foregroundStyle(.secondary).lineLimit(2)
                Label(count == 0 ? "미배치" : "배치 \(count)개", systemImage: selected ? "checkmark.circle.fill" : count == 0 ? "circle" : "square.on.square")
                    .font(.caption).foregroundStyle(selected ? Color.accentColor : Color.secondary)
            }
            .padding(8).frame(width: 180, alignment: .topLeading)
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
