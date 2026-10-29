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

    func connect(api: AuthoringAPI, session: AuthoringSession,
                 read: @escaping () async throws -> NativeOEXImportState,
                 save: @escaping (NativeOEXImportState) async throws -> NativeOEXImportState) async {
        generation = UUID(); let current = generation
        // Invalidate the old scope immediately, but join the in-flight request
        // before binding the new one. Its durable intent remains recoverable.
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
            self.api = api; self.save = save; state = restored; file = nil; job = nil
            if let id = restored.jobId {
                let observed = try await api.oexImportJob(id)
                guard generation == current, observed.id.lowercased() == id.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                job = observed
            }
            if restored.intent != nil { message = "이 작업 공간에서 시작한 OEX 가져오기 상태를 유지했습니다." }
        } catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 상태를 확인하지 못했습니다." }
    }
    func invalidate() {
        generation = UUID(); api = nil; save = nil; file = nil; job = nil; state = nil
        message = "작업 공간이 중지됐습니다. 같은 공간에서 가져오기를 다시 확인하세요."
    }
    func choose() async {
        guard !busy, api != nil, state != nil else { return }
        let panel = NSOpenPanel(); panel.title = "OEX 가져오기"; panel.message = "전시를 새 사본으로 가져옵니다. 기존 전시는 변경하지 않습니다."
        panel.allowedContentTypes = [UTType(filenameExtension: "oex") ?? .data]; panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        busy = true; defer { busy = false }; let current = generation
        do {
            let selected = try await NativeOEXImportFile.load(url)
            guard generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            if let intent = state?.intent, !intent.matches(selected.archive) { throw AuthoringFailure("OEX_IMPORT_FILE_CHANGED") }
            file = selected; message = "파일을 확인했습니다. 가져오기를 시작하거나 이어가세요."
        } catch { message = (error as? AuthoringFailure)?.errorDescription ?? "OEX 파일을 읽지 못했습니다." }
    }
    func run() async {
        guard !busy, let initial = state, let api, let save else { return }
        busy = true; defer { busy = false }; let current = generation
        do {
            let result = try await NativeOEXImportTransfer.run(state: initial, file: file, transport: api, persist: { [weak self] value in
                let saved = try await save(value); guard let self, self.generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
                self.state = saved; return saved
            }, isCurrent: { [weak self] in self?.generation == current })
            guard generation == current else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            state = result.0; job = result.1; message = "가져오기 요청을 확인했습니다. 상태를 다시 확인하세요."
        } catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기를 완료하지 못했습니다. 같은 요청으로 다시 확인하세요." }
    }
    func refresh() async {
        guard !busy, let api, let id = state?.jobId else { return }
        busy = true; defer { busy = false }; let current = generation
        do { let result = try await api.oexImportJob(id); guard generation == current, result.id.lowercased() == id.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }; job = result; message = "가져오기 상태를 확인했습니다." }
        catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 상태를 확인하지 못했습니다." }
    }
    func action(_ action: String) async {
        guard !busy, let api, let id = state?.jobId else { return }
        busy = true; defer { busy = false }; let current = generation
        do { let result = try await api.oexImportAction(id, action: action); guard generation == current, result.id.lowercased() == id.lowercased() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }; job = result; message = "가져오기 상태를 확인했습니다." }
        catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 요청을 확인하지 못했습니다." }
    }
    func clearCompleted() async {
        guard !busy, var state, let save, let job, ["complete", "cancelled"].contains(job.state), !job.cleanupPending else { return }
        busy = true; defer { busy = false }; let current = generation
        do {
            state.intent = nil; state.jobId = nil; state.fileName = nil
            let saved = try await save(state); guard generation == current else { return }
            self.state = saved; self.job = nil; file = nil; message = "다른 OEX 파일을 선택할 수 있습니다."
        } catch { message = (error as? AuthoringFailure)?.errorDescription ?? "가져오기 기록을 확인하지 못했습니다." }
    }
    var hasFile: Bool { file != nil }
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
            Button(model.state?.intent == nil ? "OEX 파일 선택…" : "같은 OEX 파일 선택…") { Task { await model.choose() } }.disabled(!ready || model.busy || model.state == nil || model.job?.state == "complete")
            if model.state?.intent == nil || model.job?.state == "uploading" || (model.state?.intent != nil && model.job == nil) {
                Button("가져오기 시작·이어가기") { Task { await model.run() } }.disabled(!ready || model.busy || model.state == nil || (model.state?.intent == nil && !model.hasFile))
            }
            if let job = model.job {
                Text(status(job.state)).font(.caption)
                Button("상태 다시 확인") { Task { await model.refresh() } }.disabled(!ready || model.busy)
                if job.state == "failed" {
                    if let code = job.errorCode { Text(AuthoringFailure(code).errorDescription ?? "파일을 확인하세요.").font(.caption) }
                    if job.attempts < 5 { Button("다시 시도") { Task { await model.action("retry") } }.disabled(!ready || model.busy) }
                }
                if ["uploading", "queued", "processing", "failed"].contains(job.state) { Button("가져오기 취소") { Task { await model.action("cancel") } }.disabled(!ready || model.busy) }
                if let id = job.result?.exhibitionId, job.state == "complete" { Button("가져온 전시 열기") { open(id) }.disabled(!ready || model.busy) }
                if ["complete", "cancelled"].contains(job.state) && !job.cleanupPending { Button("다른 OEX 가져오기") { Task { await model.clearCompleted() } }.disabled(!ready || model.busy) }
            }
            if !model.message.isEmpty { Text(model.message).font(.caption) }
        }
    }
}
