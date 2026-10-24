// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

@MainActor
final class HostingModel: ObservableObject {
    @Published private(set) var status: NativeHostingStatus?
    @Published private(set) var review: NativeHostingReview?
    @Published private(set) var busy = false
    @Published private(set) var credentialPresent: Bool?
    @Published private(set) var failure: BridgeFailure?
    @Published var accountId = ""
    @Published var rightsConfirmed = false
    @Published var costConfirmed = false
    @Published var tokenInput = ""
    @Published var replacingToken = false
    var running: Bool { status?.running == true }
    private let credentials = CloudflareCredentialStore()
    private var read: (() async throws -> NativeHostingStatus)?
    private var inspect: (() async throws -> NativeHostingReview)?
    private var accounts: ((String) async throws -> NativeHostingStatus)?
    private var deploy: ((String, String, NativeHostingReview, Bool, Bool) async throws -> NativeHostingStatus)?
    private var reconcile: ((String) async throws -> NativeHostingStatus)?
    private var cancel: (() async throws -> NativeHostingStatus)?
    private var polling: Task<Void, Never>?
    func connect(read: @escaping () async throws -> NativeHostingStatus, inspect: @escaping () async throws -> NativeHostingReview, accounts: @escaping (String) async throws -> NativeHostingStatus, deploy: @escaping (String, String, NativeHostingReview, Bool, Bool) async throws -> NativeHostingStatus, reconcile: @escaping (String) async throws -> NativeHostingStatus, cancel: @escaping () async throws -> NativeHostingStatus) async {
        self.read = read; self.inspect = inspect; self.accounts = accounts; self.deploy = deploy; self.reconcile = reconcile; self.cancel = cancel
        do { status = try await read(); if accountId.isEmpty { accountId = status?.work?.accountId ?? "" }; follow() } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_HOSTING_INVALID") }
        // Attribute-only, UI-fail query. No token read or OS prompt at startup.
        credentialPresent = try? await credentials.contains()
    }
    func resetReview() { rightsConfirmed = false; costConfirmed = false }
    func refreshReview(_ receipt: NativeStaticExportReceipt?) async {
        guard let receipt, let inspect else { review = nil; resetReview(); return }
        guard !busy, !running else { return }
        busy = true; defer { busy = false }
        do {
            let value = try await inspect()
            guard value.source.manifestSha256 == receipt.manifestSha256, value.source.publicationId == receipt.publicationId else { throw BridgeFailure("NATIVE_PUBLICATION_STATE_CHANGED") }
            if review?.source.manifestSha256 != value.source.manifestSha256 { resetReview() }
            review = value
        } catch { review = nil; resetReview(); failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_HOSTING_INVALID") }
    }
    func saveToken(_ token: String) async -> Bool {
        guard !busy, !running else { return false }
        busy = true; failure = nil; defer { busy = false }
        do { try await credentials.save(token); credentialPresent = true; return true }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("HOSTED_KEYCHAIN_STORE_FAILED"); return false }
    }
    private func perform(_ operation: (String) async throws -> NativeHostingStatus) async {
        guard !busy, !running else { return }
        busy = true; failure = nil; defer { busy = false }
        do { let token = try await credentials.load(); status = try await operation(token); follow() }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("HOSTED_OPERATION_UNCERTAIN") }
    }
    func loadAccounts() async { guard let accounts else { return }; await perform(accounts) }
    func publish() async {
        guard let deploy, let review, rightsConfirmed, costConfirmed, status?.accounts.contains(where: { $0.id == accountId }) == true else { failure = BridgeFailure("HOSTED_PERMISSION_REQUIRED"); return }
        await perform { token in try await deploy(token, accountId, review, rightsConfirmed, costConfirmed) }
    }
    func checkDeployment() async { guard let reconcile else { return }; await perform(reconcile) }
    private func follow() {
        polling?.cancel()
        guard running else { return }
        polling = Task { [weak self] in
            guard let self else { return }
            while !Task.isCancelled, running {
                do { try await Task.sleep(for: .milliseconds(250)); try Task.checkCancellation(); guard let read else { return }; status = try await read() }
                catch is CancellationError { return }
                catch { failure = error as? BridgeFailure ?? BridgeFailure("HOSTED_OPERATION_UNCERTAIN"); return }
            }
        }
    }
    func cancelAndWait() async -> Bool {
        guard running else { return true }
        guard let cancel, let read else { return false }
        do {
            status = try await cancel()
            let end = ContinuousClock.now.advanced(by: .seconds(15))
            while running, ContinuousClock.now < end { try await Task.sleep(for: .milliseconds(100)); status = try await read() }
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("HOSTED_OPERATION_UNCERTAIN"); return false }
        return !running
    }
    func cancelPublish() async { _ = await cancelAndWait() }
}

struct NativeHostingView: View {
    @ObservedObject var model: HostingModel
    let receipt: NativeStaticExportReceipt?
    let available: Bool
    private var mayDeploy: Bool { model.status?.work.map { ["completed", "failed"].contains($0.state) } ?? true }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Divider()
            Text("온라인 공개").font(.headline)
            Text("Cloudflare Pages에 표시용 전시 파일을 올립니다. 이 Mac의 제작 데이터와 원본 파일은 보관됩니다.").foregroundStyle(.secondary)
            if model.credentialPresent != true || model.replacingToken {
                SecureField("Cloudflare API 토큰", text: $model.tokenInput)
                    .textContentType(.password).accessibilityHint("토큰은 이 Mac의 키체인에 저장됩니다.")
                HStack {
                    Button("토큰을 키체인에 저장") { Task { if await model.saveToken(model.tokenInput) { model.tokenInput = ""; model.replacingToken = false } } }
                        .disabled(model.tokenInput.isEmpty || model.busy || model.running || !available)
                    Link("Cloudflare에서 토큰 만들기", destination: URL(string: "https://dash.cloudflare.com/profile/api-tokens")!)
                }
                Text("Pages 배포에 필요한 권한만 허용하세요. macOS가 키체인 접근 확인을 요청할 수 있습니다.").foregroundStyle(.secondary)
            } else {
                Label("키체인에 토큰이 저장되어 있습니다", systemImage: "key")
                Button("다른 토큰 저장…") { model.replacingToken = true }.disabled(model.busy || model.running)
            }
            Button("키체인 토큰으로 호스팅 계정 확인") { Task { await model.loadAccounts() } }.disabled(!available || model.busy || model.running)
            if let accounts = model.status?.accounts, !accounts.isEmpty {
                Picker("공개할 계정", selection: $model.accountId) {
                    Text("계정을 선택하세요").tag("")
                    ForEach(accounts) { account in Text(account.name.isEmpty ? account.id : account.name).tag(account.id) }
                }.disabled(model.busy || model.running)
            }
            if let review = model.review, receipt?.manifestSha256 == review.source.manifestSha256 {
                Text(review.info.title).font(.headline)
                Text("공개 주소: \(review.expectedURL)").textSelection(.enabled)
                Text("공개 파일 \(review.source.files - 1)개").foregroundStyle(.secondary)
                Toggle("선택한 계정과 전시 파일의 인터넷 공개 권리를 확인했습니다", isOn: $model.rightsConfirmed)
                Toggle("무료 정적 배포만 사용하며 유료 기능을 추가하지 않는 것을 확인했습니다", isOn: $model.costConfirmed)
                Text("공개 파일은 관람자의 브라우저에 전달되며 복사될 수 있습니다. 이 작업은 Render 서버나 데이터베이스를 만들지 않습니다.").foregroundStyle(.secondary)
                Button("확인한 전시를 온라인 공개") { Task { await model.publish() } }
                    .disabled(!available || model.busy || model.running || !mayDeploy || model.status?.accounts.contains(where: { $0.id == model.accountId }) != true || !model.rightsConfirmed || !model.costConfirmed)
            } else { Text("먼저 위에서 현재 전시의 정적 파일을 저장하세요.").foregroundStyle(.secondary) }
            if model.running {
                ProgressView("배포와 공개 파일을 확인하고 있습니다…")
                Button("공개 작업 정지") { Task { await model.cancelPublish() } }
            }
            if let work = model.status?.work {
                if let url = work.verifiedURL {
                    Label("공개 주소의 전시 파일 확인 완료", systemImage: "checkmark.circle")
                    if receipt?.manifestSha256 != work.source.manifestSha256 { Text("이전에 저장한 전시의 공개 결과입니다.").foregroundStyle(.secondary) }
                    Link("공개 전시 열기", destination: url)
                    Text(url.absoluteString).textSelection(.enabled)
                    Button("공개 주소 복사") { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(url.absoluteString, forType: .string) }
                } else {
                    Text(work.state == "failed" ? "제공자가 배포 실패를 보고했습니다." : "이전 공개 요청의 실제 결과를 확인해야 합니다.")
                    Text("업로드를 반복하기 전에 같은 작업의 상태를 확인하세요.").foregroundStyle(.secondary)
                }
                Button("같은 작업의 배포 상태 다시 확인") { Task { await model.checkDeployment() } }.disabled(!available || model.busy || model.running)
                DisclosureGroup("배포 식별 정보") { Text(work.projectName).textSelection(.enabled); Text(work.id).textSelection(.enabled); Text(work.source.manifestSha256).textSelection(.enabled) }
            }
            if let failure = model.failure { Text(failure.localizedDescription); DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) } }
            if let code = model.status?.errorCode { Text(BridgeFailure(code).localizedDescription); DisclosureGroup("배포 오류 상세") { Text(code).textSelection(.enabled) } }
        }
        .task(id: receipt?.manifestSha256) { await model.refreshReview(receipt) }
        .onChange(of: model.accountId) { _, _ in model.resetReview() }
    }
}
