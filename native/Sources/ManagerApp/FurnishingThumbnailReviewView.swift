// SPDX-License-Identifier: Apache-2.0
import AppKit
import ManagerCore
import SwiftUI

@MainActor
final class FurnishingThumbnailReviewModel: ObservableObject, Identifiable {
    let id = UUID()
    @Published private(set) var name: String
    private let furnishingId: String
    private var api: AuthoringAPI
    private var document: () -> NativeDocumentState?
    private var capture: () async throws -> WorkspaceFurnishingThumbnailSubmission
    private var refreshed: () async -> Void
    @Published private(set) var candidates: [WorkspaceFurnishingThumbnailCandidate] = []
    @Published private(set) var candidate: WorkspaceFurnishingThumbnailCandidate?
    @Published private(set) var image: NSImage?
    @Published private(set) var selection: WorkspaceFurnishingThumbnailSelection?
    @Published private(set) var busy = false
    @Published private(set) var message = ""
    private var expiryTask: Task<Void, Never>?
    private let ownerScope: [String]
    private var pendingCapture: WorkspaceFurnishingThumbnailSubmission?
    private struct SelectionIntent {
        let candidate: WorkspaceFurnishingThumbnailCandidate
        let expectedVersion: Int64
        let requestId: String
    }
    private var pendingSelection: SelectionIntent?

    init(name: String, furnishingId: String, owner: NativeDocumentState, api: AuthoringAPI, document: @escaping () -> NativeDocumentState?,
         capture: @escaping () async throws -> WorkspaceFurnishingThumbnailSubmission, refreshed: @escaping () async -> Void) {
        self.name = name; self.furnishingId = furnishingId; self.api = api
        ownerScope = [owner.workspaceId, owner.tenantId, owner.principalId]
        self.document = document; self.capture = capture; self.refreshed = refreshed
    }
    var hasUnresolvedIntent: Bool { pendingCapture != nil || pendingSelection != nil }
    func belongs(to key: FurnishingThumbnailReviewSessions.Key) -> Bool {
        ownerScope == [key.workspaceId, key.tenantId, key.principalId] && furnishingId == key.furnishingId
    }
    /// Reopening a sheet may refresh its connection, never its pending nonce,
    /// CAS, candidate or captured bytes. Authority is checked by each API call.
    func rebind(name: String, api: AuthoringAPI, document: @escaping () -> NativeDocumentState?,
                capture: @escaping () async throws -> WorkspaceFurnishingThumbnailSubmission,
                refreshed: @escaping () async -> Void) -> Bool {
        guard !busy, let owner = document(), [owner.workspaceId, owner.tenantId, owner.principalId] == ownerScope else { return false }
        self.name = name; self.api = api; self.document = document; self.capture = capture; self.refreshed = refreshed
        return true
    }
    var preparationTitle: String { pendingCapture == nil ? "새 이미지 준비" : "준비 결과 다시 확인" }
    var selectionTitle: String { pendingSelection == nil ? "대표 이미지로 선택" : "선택 결과 다시 확인" }
    var isCurrent: Bool { candidate.map { selection?.matches($0) == true } ?? false }
    var canChoose: Bool { !busy && pendingSelection == nil && pendingCapture == nil }
    var canPrepare: Bool { !busy && pendingSelection == nil }
    var canSelect: Bool { !busy && pendingCapture == nil && image != nil && candidate != nil && selection != nil && (!isCurrent || pendingSelection != nil) }
    private func current() throws -> NativeDocumentState {
        guard let value = document(), [value.workspaceId, value.tenantId, value.principalId] == ownerScope else {
            throw AuthoringFailure("ACCOUNT_CHANGED")
        }; return value
    }
    private func loadRecords() async throws {
        let doc = try current()
        let page = try await api.workspaceFurnishingThumbnailCandidates(furnishingId, version: 1, in: doc)
        let selected = try await api.workspaceFurnishingThumbnailSelection(furnishingId, version: 1, in: doc)
        _ = try current(); candidates = page.items; selection = selected
    }
    private func show(_ value: WorkspaceFurnishingThumbnailCandidate) async throws {
        let bytes = try await api.workspaceFurnishingThumbnailImage(value, in: current())
        let item = try await api.workspaceFurnishing(furnishingId, version: 1)
        _ = try WorkspaceFurnishingThumbnailSource(value: value.source.value, furnishing: item)
        _ = try current()
        guard let image = NSImage(data: bytes) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        expiryTask?.cancel(); expiryTask = nil
        if let text = item.details.rights.expiresAt {
            let formatter = ISO8601DateFormatter()
            var deadline = formatter.date(from: text)
            if deadline == nil { formatter.formatOptions.insert(.withFractionalSeconds); deadline = formatter.date(from: text) }
            guard let deadline, deadline > .now else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
            expiryTask = Task { [weak self] in
                do { try await Task.sleep(for: .seconds(deadline.timeIntervalSinceNow)) } catch { return }
                self?.image = nil; self?.message = "표시 기한이 지났습니다. 권한을 확인한 뒤 다시 불러오세요."
            }
        }
        candidate = value; self.image = image
    }
    func closeDisplay() { expiryTask?.cancel(); expiryTask = nil; image = nil }
    func load() async {
        guard !busy else { return }; busy = true; defer { busy = false }
        do {
            try await loadRecords()
            if let value = pendingSelection?.candidate ?? candidate ?? candidates.first(where: { $0.candidateId == selection?.selected?.candidateId }) ?? candidates.first {
                try await show(value)
            }
            message = candidates.isEmpty ? "아직 준비한 이미지가 없습니다. 새 이미지를 준비한 뒤 확인하고 선택하세요." : "이미지를 확인한 뒤 대표 이미지로 선택하세요."
        } catch { image = nil; message = "이미지를 확인하지 못했습니다. 작업 공간과 표시 권한을 확인한 뒤 다시 시도하세요." }
    }
    func review(_ value: WorkspaceFurnishingThumbnailCandidate) async {
        guard canChoose else { return }; busy = true; defer { busy = false }
        do { try await show(value); message = "이미지 준비와 대표 이미지 선택은 별도입니다." }
        catch { image = nil; message = "이 이미지를 확인하지 못했습니다. 다시 불러오세요." }
    }
    func prepare() async {
        guard !busy, pendingSelection == nil else { return }; busy = true; defer { busy = false }
        do {
            if pendingCapture == nil { pendingCapture = try await capture() }
            guard let intent = pendingCapture else { return }
            let value = try await api.prepareWorkspaceFurnishingThumbnail(intent, in: current())
            _ = try current(); pendingCapture = nil
            try await loadRecords(); try await show(value)
            message = "이미지를 준비했습니다. 아직 대표 이미지로 선택하지 않았습니다."
        } catch { image = nil; message = pendingCapture == nil ? "3D 미리보기와 표시 권한을 확인한 뒤 다시 준비하세요." : "이미지 준비를 확인하지 못했습니다. 다시 확인하면 같은 준비 요청을 사용합니다." }
    }
    func select() async {
        guard canSelect, let candidate, let selection else { return }; busy = true; defer { busy = false }
        let wasRetry = pendingSelection != nil
        do {
            if pendingSelection == nil {
                pendingSelection = .init(candidate: candidate, expectedVersion: selection.selectionVersion, requestId: UUID().uuidString.lowercased())
            }
            guard let intent = pendingSelection else { return }
            let result = try await api.selectWorkspaceFurnishingThumbnail(intent.candidate, expectedSelectionVersion: intent.expectedVersion,
                                                                         requestId: intent.requestId, in: current())
            _ = try current(); self.selection = result.current; pendingSelection = nil
            await refreshed()
            message = result.current.matches(intent.candidate) ? "대표 이미지를 선택했습니다." : "선택 요청을 확인했습니다. 현재는 다른 대표 이미지가 선택되어 있습니다."
        } catch let rejected as WorkspaceFurnishingThumbnailSelectionRejection where !wasRetry && rejected.requestId == pendingSelection?.requestId {
            // Only the first, definitely rejected POST can start a fresh intent.
            // A rejection while replaying an uncertain nonce cannot erase it.
            pendingSelection = nil; self.selection = nil; image = nil
            do { try await loadRecords(); message = "선택이 적용되지 않았습니다. 최신 대표 이미지를 확인한 뒤 다시 선택하세요." }
            catch { message = "선택이 적용되지 않았습니다. 최신 상태를 다시 불러온 뒤 선택하세요." }
        } catch { image = nil; message = "선택 결과를 확인하지 못했습니다. 다시 확인하면 같은 선택 요청을 사용합니다." }
    }
}

/// Session-owned intent retention. Sheet dismissal only drops displayed pixels;
/// it cannot create a new request while the same owner's intent is unresolved.
@MainActor
final class FurnishingThumbnailReviewSessions {
    struct Key: Hashable {
        let workspaceId: String, tenantId: String, principalId: String, furnishingId: String
        init(owner: NativeDocumentState, furnishingId: String) {
            workspaceId = owner.workspaceId; tenantId = owner.tenantId; principalId = owner.principalId; self.furnishingId = furnishingId
        }
    }
    private var values: [Key: FurnishingThumbnailReviewModel] = [:]
    func existing(_ key: Key) -> FurnishingThumbnailReviewModel? { values[key] }
    func retain(_ value: FurnishingThumbnailReviewModel, for key: Key) -> Bool {
        guard value.belongs(to: key) else { return false }
        if values[key] != nil { return values[key] === value }
        values = values.filter { $0.value.hasUnresolvedIntent }
        guard values.count < 16 else { return false }
        values[key] = value; return true
    }
}

struct FurnishingThumbnailReviewView: View {
    @ObservedObject var model: FurnishingThumbnailReviewModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("비품 대표 이미지").font(.title2.weight(.semibold))
            Text(model.name).font(.headline)
            HStack(alignment: .top, spacing: 16) {
                Group {
                    if let image = model.image { Image(nsImage: image).resizable().scaledToFit() }
                    else { ContentUnavailableView("이미지 확인", systemImage: "photo") }
                }.frame(width: 256, height: 256).background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(Array(model.candidates.enumerated()), id: \.element.candidateId) { index, value in
                        Button("이미지 \(index + 1)" + (model.selection?.matches(value) == true ? " · 현재 선택" : "")) {
                            Task { await model.review(value) }
                        }.disabled(!model.canChoose)
                    }
                    Button(model.preparationTitle) { Task { await model.prepare() } }.disabled(!model.canPrepare)
                    Button("다시 불러오기", systemImage: "arrow.clockwise") { Task { await model.load() } }.disabled(model.busy)
                }
            }
            if model.busy { ProgressView().controlSize(.small) }
            Text(model.message).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                Button("닫기") { dismiss() }.disabled(model.busy).keyboardShortcut(.cancelAction)
                Spacer()
                Text(model.isCurrent ? "현재 대표 이미지" : "").font(.caption).foregroundStyle(.secondary)
                Button(model.selectionTitle) { Task { await model.select() } }.disabled(!model.canSelect).keyboardShortcut(.defaultAction)
            }
        }.padding(24).frame(width: 580).task { await model.load() }.onDisappear { model.closeDisplay() }
            .interactiveDismissDisabled(model.busy)
    }
}
