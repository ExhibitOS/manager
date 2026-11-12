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
    private var readCheckpoint: () async throws -> NativeFurnishingThumbnailState
    private var writeCheckpoint: (NativeFurnishingThumbnailState) async throws -> NativeFurnishingThumbnailState
    @Published private(set) var candidates: [WorkspaceFurnishingThumbnailCandidate] = []
    @Published private(set) var candidate: WorkspaceFurnishingThumbnailCandidate?
    @Published private(set) var image: NSImage?
    @Published private(set) var selection: WorkspaceFurnishingThumbnailSelection?
    @Published private(set) var busy = false
    @Published private(set) var message = ""
    @Published private var checkpointReady = false
    private var checkpoint: NativeFurnishingThumbnailState?
    private var expiryTask: Task<Void, Never>?
    private let ownerScope: [String]
    private var pendingCapture: WorkspaceFurnishingThumbnailSubmission?
    private struct SelectionIntent {
        let candidate: WorkspaceFurnishingThumbnailCandidate
        let expectedVersion: Int64
        let requestId: String
    }
    private var pendingSelection: SelectionIntent?
    private enum Terminal { case capture(String), selection(String), rejected(String)
        var requestId: String { switch self { case .capture(let id), .selection(let id), .rejected(let id): id } }
    }
    private var knownTerminal: Terminal?

    init(name: String, furnishingId: String, owner: NativeDocumentState, api: AuthoringAPI,
         document: @escaping () -> NativeDocumentState?, capture: @escaping () async throws -> WorkspaceFurnishingThumbnailSubmission,
         refreshed: @escaping () async -> Void, readCheckpoint: @escaping () async throws -> NativeFurnishingThumbnailState,
         writeCheckpoint: @escaping (NativeFurnishingThumbnailState) async throws -> NativeFurnishingThumbnailState) {
        self.name = name; self.furnishingId = furnishingId; self.api = api
        ownerScope = [owner.workspaceId, owner.tenantId, owner.principalId]
        self.document = document; self.capture = capture; self.refreshed = refreshed
        self.readCheckpoint = readCheckpoint; self.writeCheckpoint = writeCheckpoint
    }
    var hasUnresolvedIntent: Bool {
        pendingCapture != nil || pendingSelection != nil || checkpoint?.pending?.furnishingId == furnishingId
    }
    func belongs(to key: FurnishingThumbnailReviewSessions.Key) -> Bool {
        ownerScope == [key.workspaceId, key.tenantId, key.principalId] && furnishingId == key.furnishingId
    }
    func rebind(name: String, api: AuthoringAPI, document: @escaping () -> NativeDocumentState?,
                capture: @escaping () async throws -> WorkspaceFurnishingThumbnailSubmission, refreshed: @escaping () async -> Void,
                readCheckpoint: @escaping () async throws -> NativeFurnishingThumbnailState,
                writeCheckpoint: @escaping (NativeFurnishingThumbnailState) async throws -> NativeFurnishingThumbnailState) -> Bool {
        guard !busy, let owner = document(), [owner.workspaceId, owner.tenantId, owner.principalId] == ownerScope else { return false }
        if self.api !== api { knownTerminal = nil }
        self.name = name; self.api = api; self.document = document; self.capture = capture; self.refreshed = refreshed
        self.readCheckpoint = readCheckpoint; self.writeCheckpoint = writeCheckpoint; checkpointReady = false
        return true
    }
    var preparationTitle: String { pendingCapture == nil ? "새 이미지 준비" : "준비 결과 다시 확인" }
    var selectionTitle: String { pendingSelection == nil ? "대표 이미지로 선택" : "선택 결과 다시 확인" }
    var isCurrent: Bool { candidate.map { selection?.matches($0) == true } ?? false }
    var canChoose: Bool { checkpointReady && !busy && !hasUnresolvedIntent }
    var canPrepare: Bool { checkpointReady && !busy && pendingSelection == nil }
    var canSelect: Bool { checkpointReady && !busy && pendingCapture == nil && image != nil && candidate != nil && selection != nil && (!isCurrent || pendingSelection != nil) }
    private func current() throws -> NativeDocumentState {
        guard let value = document(), [value.workspaceId, value.tenantId, value.principalId] == ownerScope else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        return value
    }
    private func memoryIntent() throws -> NativeFurnishingThumbnailPending? {
        if let pendingCapture { return try .init(capture: pendingCapture) }
        if let pendingSelection { return try .init(candidate: pendingSelection.candidate, requestId: pendingSelection.requestId, expectedSelectionVersion: pendingSelection.expectedVersion) }
        return nil
    }
    private func synchronizeCheckpoint() async throws {
        checkpointReady = false
        let owner = try current(), saved = try await readCheckpoint()
        try saved.requireScope(owner); _ = try current(); checkpoint = saved
        if let pending = saved.pending {
            guard pending.furnishingId == furnishingId else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_PENDING") }
            if let memory = try memoryIntent() {
                guard memory == pending else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_STATE_CHANGED") }
            } else {
                let item = try await api.workspaceFurnishing(furnishingId, version: 1)
                _ = try current()
                switch pending.kind {
                case .capture: pendingCapture = try pending.submission(in: owner, furnishing: item)
                case .selection:
                    let original = try pending.candidate(in: owner, furnishing: item)
                    guard let version = pending.expectedSelectionVersion else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_INVALID") }
                    pendingSelection = .init(candidate: original, expectedVersion: version, requestId: pending.requestId)
                }
            }
        }
        checkpointReady = true
    }
    private func persistIntent(_ intent: NativeFurnishingThumbnailPending) async throws {
        let owner = try current(); var saved = try await readCheckpoint()
        try saved.requireScope(owner); _ = try current()
        if let existing = saved.pending {
            guard existing == intent else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_PENDING") }
            checkpoint = saved; return
        }
        saved.pending = intent
        let acknowledged = try await writeCheckpoint(saved)
        try acknowledged.requireScope(owner); _ = try current()
        guard acknowledged.pending == intent else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_STATE_CHANGED") }
        checkpoint = acknowledged
    }
    private func finishKnownTerminal() async throws {
        guard let terminal = knownTerminal else { return }
        let owner = try current(); var saved = try await readCheckpoint()
        try saved.requireScope(owner); _ = try current()
        if let pending = saved.pending {
            guard pending.requestId == terminal.requestId, pending.furnishingId == furnishingId else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_PENDING") }
            saved.pending = nil
            saved = try await writeCheckpoint(saved)
            try saved.requireScope(owner); _ = try current()
            guard saved.pending == nil else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_STATE_CHANGED") }
        }
        checkpoint = saved; pendingCapture = nil; pendingSelection = nil; knownTerminal = nil
        checkpointReady = true
    }
    private func loadRecords() async throws {
        let owner = try current()
        let page = try await api.workspaceFurnishingThumbnailCandidates(furnishingId, version: 1, in: owner)
        let selected = try await api.workspaceFurnishingThumbnailSelection(furnishingId, version: 1, in: owner)
        _ = try current(); candidates = page.items; selection = selected
    }
    private func reconcileReadyCapture() async throws -> Bool {
        guard let pendingCapture, let found = candidates.first(where: { $0.request.requestId == pendingCapture.request.requestId }) else { return false }
        let owner = try current(), item = try await api.workspaceFurnishing(furnishingId, version: 1)
        let original = try WorkspaceFurnishingThumbnailCandidate(value: found.value, document: owner, furnishing: item, submission: pendingCapture)
        _ = try current(); candidate = original; knownTerminal = .capture(pendingCapture.request.requestId)
        try await finishKnownTerminal(); return true
    }
    private func show(_ value: WorkspaceFurnishingThumbnailCandidate) async throws {
        let bytes = try await api.workspaceFurnishingThumbnailImage(value, in: current())
        let item = try await api.workspaceFurnishing(furnishingId, version: 1)
        _ = try WorkspaceFurnishingThumbnailSource(value: value.source.value, furnishing: item); _ = try current()
        guard let image = NSImage(data: bytes) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        expiryTask?.cancel(); expiryTask = nil
        if let text = item.details.rights.expiresAt {
            let formatter = ISO8601DateFormatter(); var deadline = formatter.date(from: text)
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
            try await synchronizeCheckpoint(); try await finishKnownTerminal(); try await loadRecords()
            let reconciled = try await reconcileReadyCapture()
            if let value = pendingSelection?.candidate ?? candidate ?? candidates.first(where: { $0.candidateId == selection?.selected?.candidateId }) ?? candidates.first { try await show(value) }
            if reconciled { message = "이전에 준비한 이미지를 확인했습니다. 다시 만들지 않았습니다." }
            else if hasUnresolvedIntent { message = "보관한 요청을 다시 확인하세요. 같은 이미지와 선택 요청을 이어갑니다." }
            else { message = candidates.isEmpty ? "아직 준비한 이미지가 없습니다. 새 이미지를 준비한 뒤 확인하고 선택하세요." : "이미지를 확인한 뒤 대표 이미지로 선택하세요." }
        } catch {
            image = nil
            if (error as? AuthoringFailure)?.code == "NATIVE_FURNISHING_THUMBNAIL_PENDING" { message = "먼저 다른 비품에서 보관한 이미지 요청을 확인하세요." }
            else { message = "이미지와 보관한 요청을 확인하지 못했습니다. 작업 공간과 권한을 확인한 뒤 다시 불러오세요." }
        }
    }
    func review(_ value: WorkspaceFurnishingThumbnailCandidate) async {
        guard canChoose else { return }; busy = true; defer { busy = false }
        do { try await show(value); message = "이미지 준비와 대표 이미지 선택은 별도입니다." }
        catch { image = nil; message = "이 이미지를 확인하지 못했습니다. 다시 불러오세요." }
    }
    func prepare() async {
        guard !busy, pendingSelection == nil else { return }; busy = true; defer { busy = false }
        var wasRetry = pendingCapture != nil
        do {
            try await synchronizeCheckpoint(); wasRetry = pendingCapture != nil
            guard pendingSelection == nil else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_PENDING") }
            if knownTerminal != nil { try await finishKnownTerminal(); try await loadRecords(); message = "준비 결과를 확인했습니다."; return }
            if pendingCapture != nil {
                try await loadRecords()
                if try await reconcileReadyCapture() { if let candidate { try await show(candidate) }; message = "보관한 준비 결과를 확인했습니다."; return }
            }
            if pendingCapture == nil { pendingCapture = try await capture() }
            guard let intent = pendingCapture else { return }
            try await persistIntent(.init(capture: intent))
            let value = try await api.prepareWorkspaceFurnishingThumbnail(intent, in: current())
            _ = try current(); candidate = value; knownTerminal = .capture(intent.request.requestId)
            try await finishKnownTerminal(); try await loadRecords(); try await show(value)
            message = "이미지를 준비했습니다. 아직 대표 이미지로 선택하지 않았습니다."
        } catch let rejected as WorkspaceFurnishingThumbnailPreparationRejection where !wasRetry && rejected.requestId == pendingCapture?.request.requestId {
            knownTerminal = .rejected(rejected.requestId); image = nil
            do { try await finishKnownTerminal(); try await loadRecords(); message = "이미지가 준비되지 않았습니다. 최신 권한과 비품 상태를 확인한 뒤 다시 준비하세요." }
            catch { message = "거부된 준비 요청의 보관 상태를 확인하지 못했습니다. 다시 확인하세요." }
        } catch { image = nil; message = hasUnresolvedIntent ? "보관한 준비 요청을 확인하지 못했습니다. 다시 확인하면 같은 요청을 사용합니다." : "3D 미리보기와 표시 권한을 확인한 뒤 다시 준비하세요." }
    }
    func select() async {
        guard canSelect, let candidate, let selection else { return }; busy = true; defer { busy = false }
        var wasRetry = pendingSelection != nil
        do {
            try await synchronizeCheckpoint(); wasRetry = pendingSelection != nil
            if knownTerminal != nil { try await finishKnownTerminal(); try await loadRecords(); await refreshed(); message = "보관한 선택 결과를 확인했습니다."; return }
            if pendingSelection == nil { pendingSelection = .init(candidate: candidate, expectedVersion: selection.selectionVersion, requestId: UUID().uuidString.lowercased()) }
            guard let intent = pendingSelection else { return }
            try await persistIntent(.init(candidate: intent.candidate, requestId: intent.requestId, expectedSelectionVersion: intent.expectedVersion))
            let result = try await api.selectWorkspaceFurnishingThumbnail(intent.candidate, expectedSelectionVersion: intent.expectedVersion, requestId: intent.requestId, in: current())
            _ = try current(); self.selection = result.current; knownTerminal = .selection(intent.requestId)
            try await finishKnownTerminal(); await refreshed()
            message = result.current.matches(intent.candidate) ? "대표 이미지를 선택했습니다." : "선택 요청을 확인했습니다. 현재는 다른 대표 이미지가 선택되어 있습니다."
        } catch let rejected as WorkspaceFurnishingThumbnailSelectionRejection where !wasRetry && rejected.requestId == pendingSelection?.requestId {
            knownTerminal = .rejected(rejected.requestId); image = nil
            do { try await finishKnownTerminal(); self.selection = nil; try await loadRecords(); message = "선택이 적용되지 않았습니다. 최신 상태를 확인한 뒤 다시 선택하세요." }
            catch { message = "거부된 선택 요청의 보관 상태를 확인하지 못했습니다. 다시 확인하세요." }
        } catch { image = nil; message = "보관한 선택 결과를 확인하지 못했습니다. 다시 확인하면 같은 요청을 사용합니다." }
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
