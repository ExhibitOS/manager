// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import UniformTypeIdentifiers
import ManagerCore

@MainActor
final class OEXImportModel: ObservableObject {
    @Published private(set) var state: NativeOEXImportState?
    @Published private(set) var job: NativeOEXImportJob?
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
        generation = UUID(); let current = generation
        // An active operation retains its scope; an idle reconnect invalidates
        // the old scope before binding the new one.
        self.api = nil; self.save = nil
        while busy && generation == current {
            do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
        }
        guard generation == current else { return }
        state = nil; file = nil; job = nil
        do {
            let restored = try await read()
            guard generation == current, restored.tenantId.lowercased() == session.tenantId.lowercased(),
                  restored.principalId.lowercased() == session.userId.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            self.api = api; self.save = save; self.freshBounded = freshBounded
            self.bridge = bridge; self.context = context; self.mayBeginBounded = mayBeginBounded; self.acceptBoundedPending = acceptBoundedPending; self.acceptBoundedSaved = acceptBoundedSaved
            state = restored; file = nil; job = nil
            if let id = restored.jobId {
                let observed = try await api.oexImportJob(id)
                guard generation == current, observed.id.lowercased() == id.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                job = observed
            }
            if let uncertain = uncertainBounded {
                message = "서버 작업 번호 \(uncertain.jobId ?? "미확인"), 요청 번호 \(uncertain.requestId)를 보관하고 다시 시작하지 마세요."
            } else if let intent = restored.intent {
                message = intent.permitsLegacyJobMutation ? "이 작업 공간에서 시작한 OEX 가져오기 상태를 유지했습니다." :
                    (AuthoringFailure("OEX_IMPORT_PROFILE_UNAVAILABLE").errorDescription ?? "가져오기 기록을 보관하세요.")
            }
        } catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 상태를 확인하지 못했습니다." }
    }
    func invalidate() {
        generation = UUID(); api = nil; save = nil; file = nil; job = nil; state = nil
        bridge = nil; context = nil; freshBounded = false; mayBeginBounded = nil; acceptBoundedPending = nil; acceptBoundedSaved = nil
        if let uncertain = uncertainBounded {
            message = "서버 작업 번호 \(uncertain.jobId ?? "미확인"), 요청 번호 \(uncertain.requestId)를 보관하고 다시 시작하지 마세요."
        } else {
            message = "작업 공간이 중지됐습니다. 같은 공간에서 가져오기를 다시 확인하세요."
        }
    }
    func choose() async {
        guard !busy, api != nil, permitsSelection else { return }
        let panel = NSOpenPanel(); panel.title = "OEX 가져오기"; panel.message = "전시를 새 사본으로 가져옵니다. 기존 전시는 변경하지 않습니다."
        panel.allowedContentTypes = [UTType(filenameExtension: "oex") ?? .data]; panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        busy = true; defer { busy = false }; let current = generation
        do {
            let selected: NativeOEXImportFile
            if freshBounded { selected = try await NativeOEXImportFile.loadForBoundedProfile(url) }
            else { selected = try await NativeOEXImportFile.load(url) }
            guard generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            if let intent = state?.intent, !intent.matches(selected.archive) { throw AuthoringFailure("OEX_IMPORT_FILE_CHANGED") }
            file = selected; message = "파일을 확인했습니다. 가져오기를 시작하거나 이어가세요."
        } catch { message = (error as? AuthoringFailure)?.errorDescription ?? "OEX 파일을 읽지 못했습니다." }
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
            let result = try await NativeOEXImportTransfer.run(state: initial, file: file, transport: api, persist: { [weak self] value in
                let saved = try await save(value); guard let self, self.generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                self.state = saved; return saved
            }, isCurrent: { [weak self] in self?.generation == current })
            guard generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            state = result.0; job = result.1; message = "가져오기 요청을 확인했습니다. 상태를 다시 확인하세요."
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
                    document.pending = .init(requestId: requestId, etag: source.etag, draft: source.draft)

                    let stored = try await bridge.saveDocumentState(context, state: document)

                    guard stored.workspaceId == original.workspaceId, stored.tenantId == original.tenantId,
                          stored.principalId == original.principalId, stored.pending?.requestId == requestId,
                          stored.pending?.etag == source.etag, stored.pending?.draft == source.draft,
                          stored.draft == source.draft else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
                    guard await MainActor.run(body: { self?.generation == current && self?.mayBeginBounded?() == true }) else { throw AuthoringFailure("EXHIBITION_PENDING") }
                    let checked = try await bridge.documentState(context)
                    guard checked.revision == stored.revision, checked.pending?.requestId == requestId,
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
                job = completed; boundedSaveRequestId = nil
                message = "가져온 전시를 저장하고 다시 열어 확인했습니다. 작업 기록은 보관됩니다."
                return
            }
            try await Task.sleep(for: .milliseconds(250))
        }
        throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED")
    }
    var permitsSelection: Bool { freshBounded ? state?.intent == nil : permitsLegacySelection }
    func refresh() async {
        guard !busy, let api, let id = state?.jobId else { return }
        busy = true; defer { busy = false }; let current = generation
        do { let result = try await api.oexImportJob(id); guard generation == current, result.id.lowercased() == id.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }; job = result; message = "가져오기 상태를 확인했습니다." }
        catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 상태를 확인하지 못했습니다." }
    }
    func action(_ action: String) async {
        guard !busy, let api, let id = state?.jobId else { return }
        guard permitsLegacyJobMutation else {
            message = AuthoringFailure("OEX_IMPORT_PROFILE_UNAVAILABLE").errorDescription ?? "가져오기 기록을 보관하세요."
            return
        }
        busy = true; defer { busy = false }; let current = generation
        do { let result = try await api.oexImportAction(id, action: action); guard generation == current, result.id.lowercased() == id.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }; job = result; message = "가져오기 상태를 확인했습니다." }
        catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 요청을 확인하지 못했습니다." }
    }
    func clearCompleted() async {
        guard !busy, var state, let save, let job, ["complete", "cancelled"].contains(job.state), !job.cleanupPending else { return }
        guard permitsLegacyJobMutation else {
            message = AuthoringFailure("OEX_IMPORT_PROFILE_UNAVAILABLE").errorDescription ?? "가져오기 기록을 보관하세요."
            return
        }
        busy = true; defer { busy = false }; let current = generation
        do {
            state.intent = nil; state.jobId = nil; state.fileName = nil
            let saved = try await save(state); guard generation == current else { return }
            self.state = saved; self.job = nil; file = nil; message = "다른 OEX 파일을 선택할 수 있습니다."
        } catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 기록을 확인하지 못했습니다." }
    }
    var hasFile: Bool { file != nil }
    var permitsLegacyJobMutation: Bool { state?.intent?.permitsLegacyJobMutation == true }
    var permitsLegacySelection: Bool { state.map { $0.intent == nil || $0.intent?.permitsLegacyJobMutation == true } ?? false }
    var hasPendingWork: Bool {
        state?.intent != nil
    }
}

struct OEXImportControls: View {
    @ObservedObject var model: OEXImportModel
    let ready: Bool
    let open: (String) -> Void
    private func status(_ value: String) -> String {
        switch value {
        case "uploading": "파일 전송 대기"
        case "queued": "검사 대기"
        case "processing": "전시 확인 중"
        case "complete": "가져오기 완료"
        case "failed": "다시 확인 필요"
        case "cancelled": "취소됨"
        default: "확인 필요"
        }
    }
    var body: some View {
        DisclosureGroup("OEX 가져오기") {
            Text("전시를 새 사본으로 가져옵니다. 현재 전시와 파일은 유지됩니다.").font(.caption).foregroundStyle(.secondary)
            if let name = model.state?.fileName { Text(name).font(.caption).lineLimit(2) }
            Button(model.state?.intent == nil ? "OEX 파일 선택…" : "같은 OEX 파일 선택…") { Task { await model.choose() } }.disabled(!ready || model.busy || !model.permitsSelection || model.job?.state == "complete")
            if model.state?.intent == nil || model.job?.state == "uploading" || (model.state?.intent != nil && model.job == nil) {
                Button("가져오기 시작·이어가기") { Task { await model.run() } }.disabled(!ready || model.busy || !model.permitsSelection || (model.state?.intent == nil && !model.hasFile))
            }
            if let job = model.job {
                Text(status(job.state)).font(.caption)
                Button("상태 다시 확인") { Task { await model.refresh() } }.disabled(!ready || model.busy)
                if job.state == "failed" {
                    if let code = job.errorCode { Text(AuthoringFailure(code).errorDescription ?? "파일을 확인하세요.").font(.caption) }
                    if job.attempts < 5 { Button("다시 시도") { Task { await model.action("retry") } }.disabled(!ready || model.busy || !model.permitsLegacyJobMutation) }
                }
                if ["uploading", "queued", "processing", "failed"].contains(job.state) { Button("가져오기 취소") { Task { await model.action("cancel") } }.disabled(!ready || model.busy || !model.permitsLegacyJobMutation) }
                if let id = job.result?.exhibitionId, job.state == "complete" { Button("가져온 전시 열기") { open(id) }.disabled(!ready || model.busy) }
                if ["complete", "cancelled"].contains(job.state) && !job.cleanupPending { Button("다른 OEX 가져오기") { Task { await model.clearCompleted() } }.disabled(!ready || model.busy || !model.permitsLegacyJobMutation) }
            }
            if !model.message.isEmpty { Text(model.message).font(.caption).textSelection(.enabled) }
        }
    }
}
