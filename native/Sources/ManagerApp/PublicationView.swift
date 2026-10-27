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
    private var api: AuthoringAPI?
    private var session: AuthoringSession?
    private var read: (() async throws -> NativePublicationState)?
    private var write: ((NativePublicationState) async throws -> NativePublicationState)?
    func connect(api: AuthoringAPI, session: AuthoringSession, read: @escaping () async throws -> NativePublicationState, write: @escaping (NativePublicationState) async throws -> NativePublicationState) async {
        guard !busy else { return }
        if self.session?.tenantId != session.tenantId || self.session?.userId != session.userId { state = nil; report = nil; checkedId = nil; reviewed = nil }
        self.api = api; self.session = session; self.read = read; self.write = write
        await work { state = try checked(await read()) }
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
            guard let api else { return }
            report = nil; reviewed = nil
            let value = try await api.publicationReady(exhibitionId: draft.id)
            guard value.etag == etag else { throw AuthoringFailure("REMOTE_CONFLICT", status: 412) }
            checkedId = draft.id; report = value
            notice = value.status == "READY" ? "현재 저장본의 공개 준비 항목을 확인했습니다." : "전시와 작품에서 확인할 항목이 있습니다."
        }
    }
    func isReviewed(_ draft: NativeExhibitionDraft, etag: String) -> Bool { reviewed == draft.id + etag }
    func review(_ accepted: Bool, draft: NativeExhibitionDraft, etag: String) {
        reviewed = accepted && checkedId == draft.id && report?.etag == etag && report?.status == "READY" ? draft.id + etag : nil
    }
    func prepare(_ draft: NativeExhibitionDraft, etag: String) async {
        await work {
            guard isReviewed(draft, etag: etag), checkedId == draft.id, report?.etag == etag, report?.status == "READY", let read else { throw AuthoringFailure("PUBLICATION_REVIEW_REQUIRED") }
            state = try checked(await read())
            guard state?.pending == nil, var value = state else { throw AuthoringFailure("PUBLICATION_PENDING") }
            if let completed = value.completed, completed.sourceExhibitionId == draft.id, completed.sourceEtag == etag, completed.publication.status == "published" {
                try await refreshCompleted(); return
            }
            value.pending = try NativePublicationPending(source: draft, etag: etag)
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
        if pending.snapshotEtag == nil {
            let saved = try await api.saveExhibition(pending.snapshot, requestId: pending.saveRequestId, etag: nil)
            pending.snapshotEtag = saved.etag; value.pending = pending; try await persist(value)
        }
        guard let snapshotEtag = pending.snapshotEtag else { throw AuthoringFailure("PUBLICATION_PENDING") }
        let publication = try await api.preparePublication(exhibitionId: pending.snapshot.id, etag: snapshotEtag, requestId: pending.publishRequestId)
        guard var accepted = state, accepted.pending?.publishRequestId == pending.publishRequestId else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        accepted.completed = NativePublicationCompleted(pending: pending, publication: publication); accepted.pending = nil
        try await persist(accepted)
        notice = "이 Mac에 공개용 저장본을 준비했습니다. 호스팅 배포는 별도로 진행합니다."
    }
    private func refreshCompleted() async throws {
        guard let api, var value = state, var completed = value.completed else { throw AuthoringFailure("PUBLICATION_PENDING") }
        let records = try await api.publications(exhibitionId: completed.snapshotExhibitionId)
        guard let current = records.first(where: { $0.publicationId == completed.publication.publicationId && $0.revisionSha256 == completed.publication.revisionSha256 }) else { throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID") }
        completed.publication = current; value.completed = completed; try await persist(value)
        notice = current.status == "published" ? "이 Mac의 준비 저장본을 확인했습니다." : "이 준비 저장본이 해제되었습니다. 다시 확인한 뒤 새로 준비하세요."
    }
    func refresh() async { await work { try await refreshCompleted() } }
}

struct NativePublicationView: View {
    @ObservedObject var model: PublicationModel
    @ObservedObject var source: ExhibitionModel
    @ObservedObject var staticExport: StaticExportModel
    @ObservedObject var hosting: HostingModel
    let ready: Bool
    let connect: () async -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("먼저 이 Mac에서 공개용 저장본을 준비합니다. 제작 중인 전시는 계속 편집할 수 있습니다.").foregroundStyle(.secondary)
            if !ready { Text("준비 화면에서 로컬 제작을 시작하세요.") }
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
                            .disabled(!ready || model.busy || model.state?.pending != nil || !model.isReviewed(draft, etag: etag))
                    }
                }
            } else { Text("전시 선택에서 전시를 열고 저장하세요.") }
            if let pending = model.state?.pending {
                Text("보관한 공개 준비 요청이 있습니다. 같은 요청으로 결과를 확인하세요.")
                Text(pending.snapshot.title).font(.headline)
                Button("보관한 준비 다시 확인") { Task { await model.retry() } }.disabled(!ready || model.busy)
            }
            if let completed = model.state?.completed {
                let matches = source.draft?.id == completed.sourceExhibitionId && source.state?.etag == completed.sourceEtag && !source.dirty
                Label(matches ? (completed.publication.status == "published" ? "이 Mac의 공개용 저장본 준비 완료" : "준비 해제됨") : "다른 저장본의 준비 기록", systemImage: "doc.badge.checkmark")
                if !matches { Text("현재 편집 내용은 위의 확인 버튼으로 다시 확인하세요.").foregroundStyle(.secondary) }
                Text("인터넷 공개 결과는 아래 온라인 공개에서 확인하세요.").foregroundStyle(.secondary)
                DisclosureGroup("저장본 식별 정보") { Text(completed.publication.publicationId).textSelection(.enabled); Text(completed.publication.revisionSha256).textSelection(.enabled) }
                Button("준비 상태 다시 확인") { Task { await model.refresh() } }.disabled(!ready || model.busy)
            }
            if model.busy { ProgressView("공개용 저장본을 확인하고 있습니다…") }
            if let failure = model.failure {
                Text(failure.localizedDescription)
                DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) }
                Button("작업 공간 다시 연결") { Task { await connect() } }.disabled(!ready || model.busy)
            }
            if !model.notice.isEmpty { Text(model.notice).foregroundStyle(.secondary) }
            NativeStaticExportView(model: staticExport, completed: model.state?.completed.flatMap { completed in
                source.draft?.id == completed.sourceExhibitionId && source.state?.etag == completed.sourceEtag && !source.dirty ? completed : nil
            }, ready: ready && !model.busy && !hosting.running && !hosting.busy)
            NativeHostingView(model: hosting, receipt: staticExport.status?.receipt.flatMap { receipt in
                model.state?.completed?.publication.publicationId == receipt.publicationId && source.draft?.id == model.state?.completed?.sourceExhibitionId && source.state?.etag == model.state?.completed?.sourceEtag && !source.dirty ? receipt : nil
            }, available: !model.busy && (ready || hosting.status?.work != nil))
        }.task(id: ready) { if ready && source.viewportCredentials == nil { await connect() } }
    }
}
