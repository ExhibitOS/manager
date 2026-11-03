// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import UniformTypeIdentifiers
import ManagerCore

@MainActor
final class OEXImportModel: ObservableObject {
    @Published private(set) var state: NativeOEXImportState?
    @Published private(set) var busy = false
    @Published private(set) var message = ""
    private var api: AuthoringAPI?
    private var save: ((NativeOEXImportState) async throws -> NativeOEXImportState)?
    private var file: NativeOEXImportFile?
    private var generation = UUID()
    private var freshBounded = false
    private var bridge: NativeBridge?
    private var context: Context?
    private var mayBeginBounded: (() -> Bool)?
    private var acceptBoundedPending: ((NativeDocumentState) -> Bool)?
    private var acceptBoundedSaved: ((NativeDocumentState, String) -> Bool)?
    private(set) var uncertainBounded: NativeBoundedOEXUncertain?
    private var boundedSaveRequestId: String?

    func connect(api: AuthoringAPI, session: AuthoringSession,
                 read: @escaping () async throws -> NativeOEXImportState,
                 save: @escaping (NativeOEXImportState) async throws -> NativeOEXImportState,
                 freshBounded: Bool = false, bridge: NativeBridge? = nil, context: Context? = nil,
                 mayBeginBounded: @escaping () -> Bool = { false },
                 acceptBoundedPending: @escaping (NativeDocumentState) -> Bool = { _ in false },
                 acceptBoundedSaved: @escaping (NativeDocumentState, String) -> Bool = { _, _ in false }) async {
        guard !busy else { return }
        let retainedFile = file, retainedState = state, retainedContext = self.context
        let retainedAdmission = self.freshBounded
        generation = UUID(); let current = generation
        // An active operation retains its scope; an idle reconnect invalidates
        // the old scope before binding the new one.
        self.api = nil; self.save = nil
        while busy && generation == current {
            do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
        }
        guard generation == current else { return }
        if retainedFile == nil { state = nil }
        do {
            let restored = try await read()
            guard generation == current, restored.tenantId.lowercased() == session.tenantId.lowercased(),
                  restored.principalId.lowercased() == session.userId.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            if retainedFile != nil {
                guard let retainedState, retainedAdmission, freshBounded,
                      retainedState.workspaceId == restored.workspaceId,
                      retainedState.tenantId == restored.tenantId, retainedState.principalId == restored.principalId,
                      let retainedContext, let context,
                      retainedContext.activeId == context.activeId,
                      retainedContext.selectionToken == context.selectionToken else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            }
            self.api = api; self.save = save; self.freshBounded = freshBounded
            self.bridge = bridge; self.context = context; self.mayBeginBounded = mayBeginBounded; self.acceptBoundedPending = acceptBoundedPending; self.acceptBoundedSaved = acceptBoundedSaved
            state = restored; file = retainedFile
            if let uncertain = uncertainBounded {
                message = "서버 작업 번호 \(uncertain.jobId ?? "미확인"), 요청 번호 \(uncertain.requestId)를 보관하고 다시 시작하지 마세요."
            } else if let intent = restored.intent {
                message = intent.profile == "bounded-file-v1" && intent.oexFormat == "1.0.0-draft.9"
                    ? "진행 중인 가져오기 기록을 보관했습니다. 같은 요청을 다시 시작하지 마세요."
                    : "이전 방식의 가져오기 기록을 보관했습니다. 현재 앱에서는 이어가기·재시도·취소를 지원하지 않습니다."
            } else if !freshBounded {
                message = "OEX 가져오기는 새 작업 공간에서 시작합니다. 현재 전시와 저장된 작업은 보관됩니다."
            }
        } catch {
            guard generation == current else { return }
            message = retainedFile != nil
                ? "선택한 파일을 보관했습니다. 작업 공간 연결을 확인하거나 파일 선택을 취소하세요."
                : ((error as? AuthoringFailure)?.errorDescription ?? "가져오기 상태를 확인하지 못했습니다.")
        }
    }
    func invalidate() {
        generation = UUID(); api = nil; save = nil; bridge = nil
        // Keep input and its original scope as data, never as live authority.
        // Reconnect checks the same root/token before reusing a selected file.
        if !hasPendingWork { state = nil; context = nil; freshBounded = false }
        mayBeginBounded = nil; acceptBoundedPending = nil; acceptBoundedSaved = nil
        if let uncertain = uncertainBounded {
            message = "서버 작업 번호 \(uncertain.jobId ?? "미확인"), 요청 번호 \(uncertain.requestId)를 보관하고 다시 시작하지 마세요."
        } else {
            message = "작업 공간이 중지됐습니다. 같은 공간에서 가져오기를 다시 확인하세요."
        }
    }
    func choose() async {
        guard !busy, api != nil, permitsSelection else { return }
        let current = generation
        busy = true; defer { busy = false }
        let panel = NSOpenPanel(); panel.title = "OEX 가져오기"; panel.message = "전시를 새 사본으로 가져옵니다. 기존 전시는 변경하지 않습니다."
        panel.allowedContentTypes = [UTType(filenameExtension: "oex") ?? .data]; panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard generation == current, api != nil, permitsSelection else { return }
        do {
            let selected = try await NativeOEXImportFile.loadForBoundedProfile(url)
            guard generation == current, api != nil, permitsSelection else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            if let intent = state?.intent, !intent.matches(selected.archive) { throw AuthoringFailure("OEX_IMPORT_FILE_CHANGED") }
            file = selected; message = "파일을 확인했습니다. 가져오기를 시작하거나 이어가세요."
        } catch {
            guard generation == current else { return }
            if let failure = error as? AuthoringFailure, failure.code == "OEX_LIMIT" {
                message = "OEX 파일은 1바이트부터 512 MiB까지 가져올 수 있습니다. 파일 크기를 확인하세요."
            } else {
                message = (error as? AuthoringFailure)?.errorDescription ?? "OEX 파일을 읽지 못했습니다."
            }
        }
    }
    func run() async {
        guard !busy, let initial = state, let api, let save else { return }
        busy = true; defer { busy = false }; let current = generation
        do {
            if freshBounded {
                guard initial.intent == nil, let file, let bridge, let context, mayBeginBounded?() == true else { throw AuthoringFailure("OEX_IMPORT_PROFILE_UNAVAILABLE") }
                let result = try await NativeOEXImportTransfer.runBoundedOnce(state: initial, file: file, api: api, persist: { [weak self] value in
                    let saved = try await save(value); guard let self, self.generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                    self.state = saved; return saved
                }, select: { try await bridge.selectBoundedOEX(context) }, isCurrent: { [weak self] in self?.generation == current })
                guard generation == current, result.0.jobId == result.1.id else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                state = result.0
                _ = try await bridge.runBoundedOEX(context)
                try await finishBounded(api: api, bridge: bridge, context: context, original: initial, current: current)
                return
            }
            throw AuthoringFailure("OEX_IMPORT_PROFILE_UNAVAILABLE")
        } catch let uncertain as NativeBoundedOEXUncertain {
            uncertainBounded = uncertain
            message = "서버 작업 번호 \(uncertain.jobId ?? "미확인"), 요청 번호 \(uncertain.requestId)를 기록하지 못했습니다. 이 번호를 보관하고 다시 시작하지 마세요."
        } catch {

            if freshBounded, let requestId = boundedSaveRequestId, let bridge, let context, generation == current {
                // A cancelled parent cannot itself perform the read. This
                // detached read uses only the original selected token; scope
                // is checked again before adopting anything into the editor.
                let read = Task.detached { try await bridge.documentState(context) }
                if let document = try? await read.value, generation == current, document.pending?.requestId == requestId {
                    _ = acceptBoundedPending?(document)
                }
            }
            message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 결과가 확실하지 않습니다. 현재 기록을 보관하세요."
        }
    }
    /// Wait for the private owner offer. The existing document must be empty;
    /// never replace an existing or in-memory unsaved draft for this test.
    private func finishBounded(api: AuthoringAPI, bridge: NativeBridge, context: Context,
                               original: NativeOEXImportState, current: UUID) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(600))
        var offered: NativeOEXSaveOffer?
        var reopened: NativeStudioResponse?
        while ContinuousClock.now < deadline {
            try Task.checkCancellation()
            guard generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            let status = try await bridge.nativeStatus(context)
            if let offer = status.oexSaveOffer, offered == nil {
                guard mayBeginBounded?() == true else { throw AuthoringFailure("EXHIBITION_PENDING") }
                offered = offer
                boundedSaveRequestId = offer.requestId
                let result = try await api.saveAndReopenSelectedOEX(exhibitionId: offer.exhibitionId, expectedETag: offer.etag, requestId: offer.requestId) { [weak self] source, requestId in
                    guard await MainActor.run(body: { self?.generation == current && self?.mayBeginBounded?() == true }) else { throw AuthoringFailure("EXHIBITION_PENDING") }
                    var document = try await bridge.documentState(context)
                    guard document.workspaceId == original.workspaceId, document.tenantId == original.tenantId,
                          document.principalId == original.principalId, document.draft == nil, document.pending == nil,
                          document.etag == nil, document.savedEditVersion == nil,
                          let version = source.draft.document["editVersion"].integer else { throw AuthoringFailure("EXHIBITION_PENDING") }
                    document.draft = source.draft; document.etag = source.etag; document.savedEditVersion = version
                    document.pending = .init(requestId: requestId, etag: source.etag, draft: source.draft, origin: "bounded-import-v1")

                    let stored = try await bridge.saveDocumentState(context, state: document)

                    guard stored.workspaceId == original.workspaceId, stored.tenantId == original.tenantId,
                          stored.principalId == original.principalId, stored.pending?.requestId == requestId, stored.pending?.origin == "bounded-import-v1",
                          stored.pending?.etag == source.etag, stored.pending?.draft == source.draft,
                          stored.draft == source.draft else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                    guard await MainActor.run(body: { self?.generation == current && self?.mayBeginBounded?() == true }) else { throw AuthoringFailure("EXHIBITION_PENDING") }
                    let checked = try await bridge.documentState(context)
                    guard checked.revision == stored.revision, checked.pending?.requestId == requestId, checked.pending?.origin == "bounded-import-v1",
                          checked.pending?.etag == source.etag, checked.pending?.draft == source.draft,
                          checked.draft == source.draft else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                    try await bridge.authorizeBoundedOEXSave(context, offer: offer)
                    guard await MainActor.run(body: { self?.generation == current && self?.mayBeginBounded?() == true }) else { throw AuthoringFailure("EXHIBITION_PENDING") }
                }
                guard generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                reopened = result

                try await bridge.reportBoundedOEXSave(context, offer: offer, etag: result.etag)

            }
            if !status.busy {
                guard status.errorCode == nil, let offer = offered, let result = reopened else { throw AuthoringFailure(status.errorCode ?? "OEX_IMPORT_INVALID") }
                guard generation == current, let id = state?.jobId else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                let completed = try await api.oexImportJob(id)
                guard generation == current, completed.id.lowercased() == id.lowercased(), completed.state == "complete",
                      completed.result?.exhibitionId.lowercased() == offer.exhibitionId.lowercased() else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
                var document = try await bridge.documentState(context)
                guard generation == current, document.workspaceId == original.workspaceId, document.tenantId == original.tenantId,
                      document.principalId == original.principalId, document.pending?.requestId == offer.requestId,
                      document.pending?.etag == offer.etag, document.pending?.draft == result.draft,
                      document.draft == result.draft, let version = result.draft.document["editVersion"].integer else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                guard mayBeginBounded?() == true, acceptBoundedPending?(document) == true else { throw AuthoringFailure("EXHIBITION_PENDING") }
                document.etag = result.etag; document.savedEditVersion = version; document.pending = nil
                let stored = try await bridge.saveDocumentState(context, state: document)
                guard generation == current, stored.pending == nil, stored.draft == result.draft, stored.etag == result.etag,
                      acceptBoundedSaved?(stored, offer.requestId) == true else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                let cleared = try await bridge.oexImportState(context)
                guard generation == current, cleared.intent == nil, cleared.jobId == nil else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                state = cleared
                file = nil; boundedSaveRequestId = nil
                message = "가져온 전시를 저장하고 다시 열어 확인했습니다. 작업 기록은 보관됩니다."
                return
            }
            try await Task.sleep(for: .milliseconds(250))
        }
        throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED")
    }
    var isAdmittedWorkspace: Bool { freshBounded }
    var permitsSelection: Bool { api != nil && freshBounded && state?.intent == nil && state?.jobId == nil && mayBeginBounded?() == true }
    var hasFile: Bool { file != nil }
    func clearSelection() {
        guard !busy, state?.intent == nil, state?.jobId == nil, uncertainBounded == nil else { return }
        generation = UUID()
        file = nil; message = "파일 선택을 취소했습니다. 원본 파일은 보관됩니다."
    }
    var hasPendingWork: Bool {
        file != nil || state?.intent != nil || state?.jobId != nil || uncertainBounded != nil
    }
}

struct OEXImportControls: View {
    @ObservedObject var model: OEXImportModel
    let ready: Bool
    var body: some View {
        DisclosureGroup("OEX 가져오기") {
            Text("현재 OEX 형식의 전시를 새 사본으로 가져옵니다. 이전 형식은 지원하지 않으며 원본 파일과 저장된 작업은 보관됩니다.").font(.caption).foregroundStyle(.secondary)
            if let name = model.state?.fileName { Text(name).font(.caption).lineLimit(2) }
            if model.isAdmittedWorkspace && model.state?.intent == nil {
                Button("OEX 파일 선택…") { Task { await model.choose() } }
                    .disabled(!ready || model.busy || !model.permitsSelection)
                Button("가져오기 시작") { Task { await model.run() } }
                    .disabled(!ready || model.busy || !model.permitsSelection || !model.hasFile)
                if model.hasFile {
                    Button("파일 선택 취소") { model.clearSelection() }.disabled(model.busy)
                }
            }
            if let id = model.state?.jobId { Text("보관된 작업 번호 · \(id)").font(.caption).textSelection(.enabled) }
            if let id = model.state?.intent?.requestId { Text("보관된 요청 번호 · \(id)").font(.caption).textSelection(.enabled) }
            if !model.message.isEmpty { Text(model.message).font(.caption).textSelection(.enabled) }
        }
    }
}
