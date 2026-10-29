// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

@MainActor
final class PublicationModel: ObservableObject {
    @Published private(set) var state: NativePublicationState?
    @Published private(set) var busy = false
    @Published private(set) var failure: AuthoringFailure?
    @Published private(set) var notice = ""
    @Published private(set) var report: NativePublicationReady?
    @Published private(set) var checkedId: String?
    @Published private(set) var reviewed: String?
    @Published private(set) var verifiedCompleted: NativePublicationCompleted?
    private var watermarkRuntimeAvailable = false
    private var runtimeManifestSha256: String?
    private var runtimeReady = false
    private var runtimeGeneration = UUID()
    private var checkedGeneration: UUID?
    func configureWatermarkRuntime(_ configuration: NativeConfiguration?, ready: Bool) {
        let available = PublicationWatermarkRuntime.supports(manifestSHA256: configuration?.manifestSha256, ready: ready)
        if runtimeManifestSha256 != configuration?.manifestSha256 || runtimeReady != ready || watermarkRuntimeAvailable != available {
            runtimeGeneration = UUID(); report = nil; checkedId = nil; reviewed = nil; checkedGeneration = nil
            verifiedCompleted = nil
        }
        runtimeManifestSha256 = configuration?.manifestSha256; runtimeReady = ready; watermarkRuntimeAvailable = available
    }
    private func requireWatermarkRuntime(_ draft: NativeExhibitionDraft) throws {
        if try draft.publicationWatermark() != nil, !watermarkRuntimeAvailable { throw AuthoringFailure("WATERMARK_RUNTIME_UNAVAILABLE") }
    }
    private func planBinding(_ draft: NativeExhibitionDraft, etag: String, sourceId: String? = nil) throws -> String? {
        try requireWatermarkRuntime(draft)
        guard runtimeReady else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
        // The legacy flow retains its old history. Only an adopted watermark
        // runtime emits the new optional journal field to its matching helper.
        guard watermarkRuntimeAvailable else { return nil }
        guard let runtimeManifestSha256 else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
        return try nativePublicationPlanBinding(sourceExhibitionId: sourceId ?? draft.id, etag: etag, runtimeManifestSha256: runtimeManifestSha256, watermark: draft.publicationWatermark())
    }
    func completedMatches(_ completed: NativePublicationCompleted, source: ExhibitionModel) -> Bool {
        guard statusVerified(completed), completed.publication.status == "published" else { return false }
        return completedRefersToSource(completed, source: source)
    }
    func statusVerified(_ completed: NativePublicationCompleted) -> Bool { verifiedCompleted == completed && state?.completed == completed }
    var preparationStatusKnown: Bool {
        guard let state else { return false }
        return state.completed.map(statusVerified) ?? true
    }
    func completedRefersToSource(_ completed: NativePublicationCompleted, source: ExhibitionModel) -> Bool {
        guard !source.dirty, let draft = source.draft, let etag = source.state?.etag else { return false }
        do { return completed.refersToSource(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: try planBinding(draft, etag: etag)) }
        catch { return false }
    }
    func canRelease(_ completed: NativePublicationCompleted, source: ExhibitionModel) -> Bool {
        !busy && !source.busy && !source.pending && state?.pending == nil && completedMatches(completed, source: source)
    }
    private var api: AuthoringAPI?
    private var session: AuthoringSession?
    private var read: (() async throws -> NativePublicationState)?
    private var write: ((NativePublicationState) async throws -> NativePublicationState)?
    func connect(api: AuthoringAPI, session: AuthoringSession, read: @escaping () async throws -> NativePublicationState, write: @escaping (NativePublicationState) async throws -> NativePublicationState) async {
        guard !busy else { return }
        if self.session?.tenantId != session.tenantId || self.session?.userId != session.userId { state = nil; report = nil; checkedId = nil; reviewed = nil; checkedGeneration = nil }
        self.api = api; self.session = session; self.read = read; self.write = write
        verifiedCompleted = nil
        await work {
            let generation = runtimeGeneration
            state = try checked(await read())
            guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
            if state?.completed != nil && state?.pending == nil { try await refreshCompleted() }
        }
    }
    private func checked(_ value: NativePublicationState) throws -> NativePublicationState {
        guard let session, value.tenantId.lowercased() == session.tenantId.lowercased(), value.principalId.lowercased() == session.userId.lowercased() else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        return value
    }
    private func persist(_ value: NativePublicationState) async throws {
        guard let write else { throw AuthoringFailure("PUBLICATION_STORE_REQUIRED") }
        state = try checked(await write(value))
    }
    private func work(_ body: () async throws -> Void) async {
        guard !busy, api != nil else { return }
        busy = true; failure = nil; notice = ""
        defer { busy = false }
        do { try await body() }
        catch {
            failure = error as? AuthoringFailure ?? AuthoringFailure((error as? BridgeFailure)?.code ?? "CONNECTION_FAILED")
            if (error as? BridgeFailure)?.code == "NATIVE_PUBLICATION_STATE_CHANGED", let read, let fresh = try? await read() { state = try? checked(fresh) }
        }
    }
    func check(_ draft: NativeExhibitionDraft, etag: String) async {
        await work {
            try requireWatermarkRuntime(draft)
            guard let api else { return }
            let generation = runtimeGeneration
            _ = try planBinding(draft, etag: etag)
            report = nil; reviewed = nil
            let value = try await api.publicationReady(exhibitionId: draft.id)
            guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
            guard value.etag == etag else { throw AuthoringFailure("REMOTE_CONFLICT", status: 412) }
            checkedId = draft.id; report = value; checkedGeneration = generation
            notice = value.status == "READY" ? "현재 저장본의 공개 준비 항목을 확인했습니다." : "전시와 작품에서 확인할 항목이 있습니다."
        }
    }
    func isReviewed(_ draft: NativeExhibitionDraft, etag: String) -> Bool { reviewed == draft.id + etag + runtimeGeneration.uuidString }
    func review(_ accepted: Bool, draft: NativeExhibitionDraft, etag: String) {
        reviewed = accepted && checkedGeneration == runtimeGeneration && checkedId == draft.id && report?.etag == etag && report?.status == "READY" ? draft.id + etag + runtimeGeneration.uuidString : nil
    }
    func prepare(_ draft: NativeExhibitionDraft, etag: String) async {
        await work {
            try requireWatermarkRuntime(draft)
            guard isReviewed(draft, etag: etag), checkedId == draft.id, report?.etag == etag, report?.status == "READY", let read else { throw AuthoringFailure("PUBLICATION_REVIEW_REQUIRED") }
            let generation = runtimeGeneration, binding = try planBinding(draft, etag: etag)
            state = try checked(await read())
            guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
            guard state?.pending == nil, var value = state else { throw AuthoringFailure("PUBLICATION_PENDING") }
            guard preparationStatusKnown else { throw AuthoringFailure("PUBLICATION_STATUS_UNCONFIRMED") }
            if let completed = value.completed, completed.isReusable(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: binding) {
                try await refreshCompleted(); return
            }
            value.pending = try NativePublicationPending(source: draft, etag: etag, planBindingSha256: binding)
            try await persist(value) // exact copy + BOTH request IDs BEFORE first POST
            try await finishPending()
        }
    }
    func retry() async {
        await work {
            guard let read else { throw AuthoringFailure("PUBLICATION_STORE_REQUIRED") }
            state = try checked(await read()); try await finishPending()
        }
    }
    private func finishPending() async throws {
        guard let api, var value = state, var pending = value.pending else { throw AuthoringFailure("PUBLICATION_PENDING") }
        try requireWatermarkRuntime(pending.snapshot)
        let generation = runtimeGeneration
        try pending.requirePlanBinding(planBinding(pending.snapshot, etag: pending.sourceEtag, sourceId: pending.sourceExhibitionId))
        if pending.snapshotEtag == nil {
            let saved = try await api.saveExhibition(pending.snapshot, requestId: pending.saveRequestId, etag: nil)
            guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
            pending.snapshotEtag = saved.etag; value.pending = pending; try await persist(value)
        }
        guard let snapshotEtag = pending.snapshotEtag else { throw AuthoringFailure("PUBLICATION_PENDING") }
        guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
        try requireWatermarkRuntime(pending.snapshot)
        let publication = try await api.preparePublication(exhibitionId: pending.snapshot.id, etag: snapshotEtag, requestId: pending.publishRequestId)
        guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
        guard var accepted = state, accepted.pending?.publishRequestId == pending.publishRequestId else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        accepted.completed = NativePublicationCompleted(pending: pending, publication: publication); accepted.pending = nil
        try await persist(accepted)
        guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        verifiedCompleted = state?.completed
        notice = "이 Mac에 공개용 저장본을 준비했습니다. 호스팅 배포는 별도로 진행합니다."
    }
    private func refreshCompleted() async throws {
        guard let api, let original = state else { throw AuthoringFailure("PUBLICATION_PENDING") }
        let binding = try NativePublicationStatusBinding(original), generation = runtimeGeneration
        verifiedCompleted = nil
        let records = try await api.publications(exhibitionId: binding.completed.snapshotExhibitionId)
        guard generation == runtimeGeneration, let state else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        guard let current = records.first(where: { $0.publicationId == binding.completed.publication.publicationId }) else { throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID") }
        let updated = try binding.replacingPublication(current, in: state)
        if updated != state { try await persist(updated) }
        guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        verifiedCompleted = self.state?.completed
        notice = current.status == "published" ? "이 Mac의 준비 저장본을 확인했습니다." : "이 준비 저장본이 해제되었습니다. 다시 확인한 뒤 새로 준비하세요."
    }
    func refresh() async { await work { try await refreshCompleted() } }
    func release(_ expected: NativePublicationCompleted, source: ExhibitionModel) async {
        guard canRelease(expected, source: source) else { return }
        await work {
            guard let api, let read else { throw AuthoringFailure("PUBLICATION_STORE_REQUIRED") }
            let generation = runtimeGeneration
            state = try checked(await read())
            guard generation == runtimeGeneration, let state, state.completed == expected, state.pending == nil,
                  completedRefersToSource(expected, source: source) else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
            let binding = try NativePublicationStatusBinding(state)
            verifiedCompleted = nil
            do {
                let response = try await api.unpublish(expected.publication)
                guard generation == runtimeGeneration, let current = self.state else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
                try await persist(binding.replacingPublication(response, in: current))
                guard generation == runtimeGeneration else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
                verifiedCompleted = self.state?.completed
                notice = "이 Mac의 공개용 저장본 준비를 해제했습니다. 제작 원본은 유지됩니다. 이미 내보낸 파일과 인터넷 배포는 별도로 중단하세요."
            } catch {
                notice = "준비 해제 결과를 확인하지 못했습니다. ‘준비 상태 다시 확인’으로 같은 저장본의 현재 상태를 확인하세요."
                throw error
            }
        }
    }
}

struct NativePublicationView: View {
    @ObservedObject var model: PublicationModel
    @ObservedObject var source: ExhibitionModel
    @ObservedObject var staticExport: StaticExportModel
    @ObservedObject var hosting: HostingModel
    let ready: Bool
    let mode: Mode
    let preview: () -> Void
    let connect: () async -> Void
    @State private var releaseCandidate: NativePublicationCompleted?
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("먼저 이 Mac에서 공개용 저장본을 준비합니다. 제작 중인 전시는 계속 편집할 수 있습니다.").foregroundStyle(.secondary)
            if !ready { Text("작업 환경에서 로컬 제작을 시작하세요.") }
            if source.draft != nil {
                NativePublicationWatermarkView(source: source, mode: mode, ready: ready, preview: preview)
            }
            if let draft = source.draft, let etag = source.state?.etag {
                Text(draft.title).font(.headline)
                if source.dirty { Text("현재 입력을 먼저 전시 저장본에 반영하세요.") }
                Button("공개 준비 항목 확인") { Task { await model.check(draft, etag: etag) } }.disabled(!ready || source.dirty || model.busy || model.state?.pending != nil)
                if model.checkedId == draft.id, let report = model.report, report.etag == etag, !source.dirty {
                    Label(report.status == "READY" ? "준비 항목 확인 완료" : "확인할 항목 \(report.issues.count)개", systemImage: report.status == "READY" ? "checkmark.circle" : "exclamationmark.circle")
                    ForEach(Array(report.issues.prefix(8).enumerated()), id: \.offset) { _, issue in
                        VStack(alignment: .leading) {
                            Text("작품의 승인·표시 권리와 전시 정보를 확인하세요.")
                            DisclosureGroup("항목 상세") { Text(issue.code); Text(issue.path).textSelection(.enabled); Text(issue.message); Text(issue.remediation) }
                        }
                    }
                    if report.status == "READY" {
                        Toggle("작품을 표시할 권리를 확인했습니다", isOn: Binding(get: { model.isReviewed(draft, etag: etag) }, set: { model.review($0, draft: draft, etag: etag) }))
                        Text("인터넷에 배포하면 브라우저로 전달된 작품 파일은 복사될 수 있습니다. 호스팅 대상·공개 범위·비용은 배포 전에 따로 확인합니다.").foregroundStyle(.secondary)
                        Button("공개용 저장본 준비") { Task { await model.prepare(draft, etag: etag) } }
                            .disabled(!ready || model.busy || model.state?.pending != nil || !model.preparationStatusKnown || !model.isReviewed(draft, etag: etag))
                    }
                }
            } else { Text("전시 편집에서 전시를 열고 저장하세요.") }
            if let pending = model.state?.pending {
                Text("보관한 공개 준비 요청이 있습니다. 같은 요청으로 결과를 확인하세요.")
                Text(pending.snapshot.title).font(.headline)
                Button("보관한 준비 다시 확인") { Task { await model.retry() } }.disabled(!ready || model.busy)
            }
            if let completed = model.state?.completed {
                let matches = model.completedRefersToSource(completed, source: source)
                Label(matches ? (model.statusVerified(completed) ? (completed.publication.status == "published" ? "이 Mac의 공개용 저장본 준비 완료" : "준비 해제됨") : "이 Mac의 준비 상태 확인 필요") : "이전 실행본·저장본의 준비 기록", systemImage: "doc.badge.checkmark")
                if !matches { Text("현재 편집 내용은 위의 확인 버튼으로 다시 확인하세요.").foregroundStyle(.secondary) }
                if !model.statusVerified(completed) { Text("새 공개용 저장본을 준비하기 전에 이 기록의 준비 상태를 다시 확인하세요. 확인 전에는 이 기록을 유지합니다.").foregroundStyle(.secondary) }
                Text("인터넷 공개 결과는 아래 온라인 공개에서 확인하세요.").foregroundStyle(.secondary)
                DisclosureGroup("저장본 식별 정보") { Text(completed.publication.publicationId).textSelection(.enabled); Text(completed.publication.revisionSha256).textSelection(.enabled) }
                Button("준비 상태 다시 확인") { Task { await model.refresh() } }.disabled(!ready || model.busy)
                Button("이 Mac의 준비 해제") { releaseCandidate = completed }
                    .disabled(!ready || !model.canRelease(completed, source: source))
                Text("준비 해제는 이 Mac의 저장본에만 적용됩니다. 이미 내보낸 파일과 인터넷 배포는 별도로 중단하세요.").font(.callout).foregroundStyle(.secondary)
            }
            if model.busy { ProgressView("공개용 저장본을 확인하고 있습니다…") }
            if let failure = model.failure {
                Text(failure.localizedDescription)
                DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) }
                Button("작업 공간 다시 연결") { Task { await connect() } }.disabled(!ready || model.busy)
            }
            if !model.notice.isEmpty { Text(model.notice).foregroundStyle(.secondary) }
            NativeStaticExportView(model: staticExport, completed: model.state?.completed.flatMap { completed in
                model.completedMatches(completed, source: source) ? completed : nil
            }, ready: ready && !model.busy && !hosting.running && !hosting.busy)
            NativeHostingView(model: hosting, receipt: staticExport.status?.receipt.flatMap { receipt in
                model.state?.completed.flatMap { completed in
                    completed.publication.publicationId == receipt.publicationId && model.completedMatches(completed, source: source) ? receipt : nil
                }
            }, available: !model.busy && (ready || hosting.status?.work != nil))
        }.task(id: ready) { if ready && source.viewportCredentials == nil { await connect() } }
        .confirmationDialog("이 Mac의 공개용 저장본 준비를 해제할까요?", isPresented: Binding(get: { releaseCandidate != nil }, set: { if !$0 { releaseCandidate = nil } }), titleVisibility: .visible, presenting: releaseCandidate) { expected in
            Button("이 Mac의 준비 해제", role: .destructive) { Task { await model.release(expected, source: source) } }
            Button("취소", role: .cancel) {}
        } message: { _ in
            Text("현재 전시의 이 Mac 공개용 저장본 제공을 중단합니다. 제작 원본과 파일은 삭제하지 않습니다. 이미 내보낸 파일·정적 사이트·인터넷 배포는 바뀌지 않습니다.")
        }
    }
}
