// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

@MainActor
final class ManagerModel: ObservableObject {
    @Published private var selection = PresentationSelection()
    var presentation: Presentation? { selection.value }
    private var presentationSave: Task<Void, Never>?
    var presentationPending: Bool { selection.hasPendingSave }
    @Published private(set) var context: Context?
    @Published private(set) var native: NativeHostStatus?
    @Published var accountSubject = ""
    @Published var accountPassword = ""
    @Published private(set) var quitting = false
    @Published private(set) var jobs: [Job] = []
    @Published private(set) var busy = false
    @Published private(set) var failure: BridgeFailure?
    @Published private(set) var authoring = AuthoringModel()
    @Published private(set) var exhibitions = ExhibitionModel()
    @Published private(set) var publication = PublicationModel()
    @Published private(set) var staticExport = StaticExportModel()
    @Published private(set) var hosting = HostingModel()
    @Published private(set) var maintenanceNotice = ""
    @Published private(set) var backupLocation: URL?
    var pendingRestoration: Bool { native?.restoration.map { $0.state != "activated" } == true }
    var canMaintain: Bool { !busy && !runtimeBusy && !quitting && !hasUnsavedAuthoring && !authoring.busy && !exhibitions.busy && !publication.busy && !staticExport.running && !hosting.running && !hosting.busy && context != nil }
    var hasUnsavedAuthoring: Bool { authoring.dirty || exhibitions.dirty }
    private var bridge: NativeBridge?
    private var pollTask: Task<Void, Never>?
    private var authoringConnection: Task<Void, Never>?
    private var startAfterPrepare = false
    private var serviceRequested = false
    var mayHaveServices: Bool { serviceRequested || native?.needsShutdown == true || staticExport.running || hosting.running || hosting.busy }
    var runtimeBusy: Bool { native?.busy == true }
    var finalizingRestoration: Bool { runtimeBusy && native?.operation == "activating_restore" }
    var ready: Bool { native?.runtime?.readiness.ready == true }
    var canStart: Bool { !busy && !runtimeBusy && !quitting && !pendingRestoration && !ready && native?.runtime?.hasOwnedProcesses != true && native != nil && (native?.accountRequired != true || native?.configuration != nil || validInitialAccount) }
    var validInitialAccount: Bool {
        accountSubject.range(of: "^[A-Za-z0-9][A-Za-z0-9_.@-]{2,127}$", options: .regularExpression) != nil
            && accountPassword.count >= 12 && accountPassword.utf8.count <= 1024
            && accountPassword.unicodeScalars.allSatisfy { !CharacterSet.controlCharacters.contains($0) }
    }
    var canStop: Bool { !busy && !quitting && !finalizingRestoration && (runtimeBusy || native?.runtime?.hasOwnedProcesses == true) }

    init(preview: Presentation? = nil) { if let preview { selection.refresh(preview) } }

    func refresh() async {
        guard !busy else { return }
        busy = true; failure = nil
        defer { busy = false }
        do {
            if bridge == nil {
                let helper = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/exhibitos-native")
                let profile: URL
                #if DEBUG
                let validationProfile = Bundle.main.object(forInfoDictionaryKey: "ExhibitOSValidationProfile") as? String
                #else
                let validationProfile: String? = nil
                #endif
                if let path = validationProfile ?? ProcessInfo.processInfo.environment["EXHIBITOS_MANAGER_PROFILE"] {
                    guard path.hasPrefix("/") else { throw BridgeFailure("NATIVE_PROFILE_REQUIRED") }
                    profile = URL(fileURLWithPath: path, isDirectory: true)
                } else {
                    profile = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
                        .appendingPathComponent("org.exhibitos.manager", isDirectory: true)
                }
                bridge = try NativeBridge(helper: helper, profile: profile)
            }
            guard let bridge else { throw BridgeFailure("NATIVE_HELPER_MISSING") }
            selection.refresh(try await bridge.presentation())
            savePresentation()
            let selected = try await bridge.context()
            context = selected
            // Load durable jobs before runtime probe: stopped/unavailable legacy
            // engines must not hide the existing operation history.
            jobs = try await bridge.jobs(selected)
            native = try await bridge.nativeStatus(selected)
            await bindStaticExport()
            schedulePoll()
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_CONNECTION_FAILED") }
    }

    func connectPersonalAuthoring() async {
        if let authoringConnection { await authoringConnection.value; return }
        guard !busy, !quitting, ready else { return }
        // This shared workspace connection belongs to the model, not whichever
        // screen's .task first asked for it. New screens join the same operation.
        let connection = Task<Void, Never> { [weak self] in
            guard let self else { return }
            await self.connectPersonalAuthoringOwned()
        }
        authoringConnection = connection
        await connection.value
        authoringConnection = nil
    }
    private func connectPersonalAuthoringOwned() async {
        guard !busy, !quitting, ready, let bridge, let context else { return }
        busy = true
        defer { busy = false }
        do {
            await bindStaticExport()
            authoring.configure(native?.configuration, ready: ready)
            authoring.bindImportStore(read: { try await bridge.uploadState(context) }, save: { state in try await bridge.saveUploadState(context, state: state) })
            let credentials = try await bridge.authoringSession(context)
            await authoring.connect(credentials)
            if let api = authoring.exhibitionAPI, let session = authoring.session {
                await exhibitions.connect(api: api, session: session, credentials: credentials, read: { try await bridge.documentState(context) }, write: { state in try await bridge.saveDocumentState(context, state: state) })
                await publication.connect(api: api, session: session, read: { try await bridge.publicationState(context) }, write: { state in try await bridge.savePublicationState(context, state: state) })
            }
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_CONNECTION_FAILED") }
    }
    private func bindStaticExport() async {
        guard native?.configuration?.kind == "personal", let bridge, let context else { return }
        await staticExport.connect(read: { try await bridge.staticExportStatus(context) }, start: { id,output in try await bridge.startStaticExport(context, publicationId: id, output: output) }, cancel: { try await bridge.cancelStaticExport(context) })
        await hosting.connect(read: { try await bridge.hostingStatus(context) }, inspect: { try await bridge.hostingReview(context) }, accounts: { token in try await bridge.hostingAccounts(context, token: token) }, deploy: { token, account, review, rights, cost in try await bridge.deployStatic(context, token: token, accountId: account, review: review, rightsConfirmed: rights, costConfirmed: cost) }, reconcile: { token in try await bridge.reconcileHosting(context, token: token) }, cancel: { try await bridge.cancelHosting(context) })
    }
    func prepareAndStart() async {
        guard canStart, let bridge, let context else { return }
        busy = true; failure = nil
        defer { busy = false }
        do {
            if native?.configuration == nil {
                let needsAccount = native?.accountRequired == true
                native = try await bridge.prepare(context, subject: needsAccount ? accountSubject : nil, password: needsAccount ? accountPassword : nil)
                accountPassword = ""
                startAfterPrepare = true
            } else {
                serviceRequested = true
                native = try await bridge.start(context)
            }
            schedulePoll()
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_CONNECTION_FAILED") }
    }

    func stop() async {
        guard !busy, let bridge, let context else { return }
        busy = true; failure = nil; startAfterPrepare = false
        defer { busy = false }
        do { native = try await bridge.stop(context); schedulePoll() }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_CONNECTION_FAILED") }
    }

    private func waitForMaintenanceIdle(_ bridge: NativeBridge, _ context: Context) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(120))
        while ContinuousClock.now < deadline {
            native = try await bridge.nativeStatus(context)
            if native?.busy == false {
                if let code = native?.errorCode { throw BridgeFailure(code) }
                return
            }
            try await Task.sleep(for: .milliseconds(200))
        }
        throw BridgeFailure("NATIVE_TIMEOUT")
    }
    private func stopForMaintenance(_ bridge: NativeBridge, _ context: Context) async throws {
        startAfterPrepare = false
        native = try await bridge.stop(context)
        try await waitForMaintenanceIdle(bridge, context)
        guard native?.runtime?.hasOwnedProcesses != true else { throw BridgeFailure("NATIVE_STOP_PENDING") }
        serviceRequested = false
    }
    private func replaceWorkspaceModels() {
        authoring = AuthoringModel(); exhibitions = ExhibitionModel()
        publication = PublicationModel(); staticExport = StaticExportModel()
        hosting = HostingModel()
    }
    func makeBackup(key: URL, destination: URL) async {
        guard canMaintain, !pendingRestoration, let bridge, let context else { return }
        busy = true; failure = nil; maintenanceNotice = ""; backupLocation = nil
        defer { busy = false }
        do {
            try await stopForMaintenance(bridge, context)
            try await bridge.createBackupKey(context, at: key)
            native = try await bridge.backup(context, key: key, destination: destination)
            backupLocation = destination
            maintenanceNotice = "백업과 키 파일을 서로 다른 장소에 보관하세요. 완료 후 로컬 제작을 다시 시작할 수 있습니다."
            schedulePoll()
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_BACKUP_FAILED") }
    }
    func restoreBackup(source: URL, key: URL, createWorkspace: Bool) async {
        guard canMaintain, let bridge, var selected = context else { return }
        busy = true; failure = nil; maintenanceNotice = ""
        defer { busy = false }
        do {
            try await stopForMaintenance(bridge, selected)
            if createWorkspace {
                selected = try await bridge.createRecoveryWorkspace(selected)
                context = selected
                replaceWorkspaceModels()
                native = try await bridge.nativeStatus(selected)
            }
            if native?.configuration == nil && !pendingRestoration {
                native = try await bridge.prepare(selected)
                try await waitForMaintenanceIdle(bridge, selected)
            }
            native = try await bridge.restore(selected, key: key, source: source)
            maintenanceNotice = "백업을 새 작업 공간에서 확인하고 있습니다. 원래 작업 공간은 보관됩니다."
            schedulePoll()
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_RESTORE_INVALID") }
    }
    func activateRestoration() async {
        guard canMaintain, let bridge, let context, let restoration = native?.restoration, restoration.state == "staged" else { return }
        busy = true; failure = nil; maintenanceNotice = ""
        defer { busy = false }
        do {
            native = try await bridge.activateRestoration(context, id: restoration.id)
            replaceWorkspaceModels()
            maintenanceNotice = "복원한 파일과 작업 공간을 다시 확인하고 있습니다."
            schedulePoll()
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_RESTORE_INVALID") }
    }
    func selectWorkspace(_ id: String) async {
        guard canMaintain, let bridge, let context, id != context.activeId else { return }
        busy = true; failure = nil; maintenanceNotice = ""
        defer { busy = false }
        do {
            try await stopForMaintenance(bridge, context)
            let selected = try await bridge.selectWorkspace(context, id: id)
            self.context = selected
            replaceWorkspaceModels()
            jobs = try await bridge.jobs(selected)
            native = try await bridge.nativeStatus(selected)
            maintenanceNotice = "선택한 작업 공간을 열었습니다. 로컬 제작을 시작할 수 있습니다."
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_CONNECTION_FAILED") }
    }

    private func schedulePoll() {
        guard pollTask == nil, native?.needsShutdown == true || startAfterPrepare else { return }
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(1)) } catch { break }
                guard let self else { break }
                if self.busy { continue }
                await self.pollRuntime()
                if self.native?.needsShutdown != true && !self.startAfterPrepare { break }
            }
            self?.pollTask = nil
        }
    }

    private func pollRuntime() async {
        guard let bridge, let context, !busy else { return }
        // Background host work does not disable mode or navigation. Only this
        // short pipe exchange is serialized with other presentation writes.
        busy = true
        defer { busy = false }
        do {
            native = try await bridge.nativeStatus(context)
            if native?.busy == false {
                if let code = native?.errorCode { failure = BridgeFailure(code); startAfterPrepare = false }
                if startAfterPrepare && native?.configuration != nil && !quitting {
                    startAfterPrepare = false; serviceRequested = true
                    native = try await bridge.start(context)
                }
                if native?.needsShutdown == false { serviceRequested = false }
            }
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_CONNECTION_FAILED") }
    }

    func stopForQuit() async -> Bool {
        quitting = true; startAfterPrepare = false
        defer { quitting = false }
        while busy { try? await Task.sleep(for: .milliseconds(100)) }
        await presentationSave?.value
        guard !presentationPending else { failure = BridgeFailure("NATIVE_SAVE_FAILED"); return false }
        guard let bridge, let context else { return !serviceRequested }
        while hosting.busy { try? await Task.sleep(for: .milliseconds(100)) }
        guard await hosting.cancelAndWait() else { failure = BridgeFailure("HOSTED_OPERATION_UNCERTAIN"); return false }
        guard await staticExport.cancelAndWait() else { failure = BridgeFailure("NATIVE_STATIC_STOP_PENDING"); return false }
        do {
            native = try await bridge.stop(context)
            let deadline = ContinuousClock.now.advanced(by: .seconds(90))
            while ContinuousClock.now < deadline || finalizingRestoration {
                native = try await bridge.nativeStatus(context)
                if native?.needsShutdown == false { serviceRequested = false; return true }
                try await Task.sleep(for: .milliseconds(250))
            }
            failure = BridgeFailure("NATIVE_STOP_PENDING")
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_CONNECTION_FAILED") }
        return false
    }

    func change(mode: Mode? = nil, step: Step? = nil, scale: Int? = nil) {
        guard !quitting, bridge != nil else { return }
        // SwiftUI's selection binding and detail view change in the same turn.
        // Runtime work and disk persistence must not disable or revert navigation.
        if selection.select(mode: mode, step: step, scale: scale) { savePresentation() }
    }
    private func savePresentation() {
        guard presentationSave == nil, let bridge else { return }
        presentationSave = Task { [weak self] in
            guard let self else { return }
            defer { self.presentationSave = nil }
            while let request = self.selection.nextSave() {
                do { try self.selection.acknowledge(request, stored: await bridge.save(request.value)) }
                catch {
                    self.selection.failed(request)
                    self.failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_SAVE_FAILED")
                    return
                }
            }
        }
    }

}

extension Step {
    var title: String {
        switch self { case .prepare: "준비"; case .create: "Studio 제작"; case .select: "전시 선택"; case .preview: "미리보기"; case .publish: "공개" }
    }
    var symbol: String {
        switch self { case .prepare: "desktopcomputer"; case .create: "paintbrush"; case .select: "square.grid.2x2"; case .preview: "eye"; case .publish: "globe" }
    }
}

@MainActor
final class ManagerDelegate: NSObject, NSApplicationDelegate {
    weak var model: ManagerModel?
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let model, model.mayHaveServices || model.hasUnsavedAuthoring || model.presentationPending else { return .terminateNow }
        if model.quitting { return .terminateCancel }
        if model.mayHaveServices || model.hasUnsavedAuthoring {
        let alert = NSAlert()
        alert.messageText = "로컬 작업을 정지하고 종료할까요?"
        alert.informativeText = model.hasUnsavedAuthoring ? "저장본에 반영하지 않은 입력이 있습니다. 이 Mac에 보관한 전시는 다시 열 수 있지만, 아직 보관하지 않은 입력은 사라집니다. 계속 편집하려면 취소하세요." : "이 Mac의 제작 서비스가 정지됩니다. 저장된 작품과 인터넷에 공개한 전시는 유지됩니다."
        alert.addButton(withTitle: "정지하고 종료")
        alert.addButton(withTitle: "취소")
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        }
        Task {
            let stopped = await model.stopForQuit()
            sender.reply(toApplicationShouldTerminate: stopped)
        }
        return .terminateLater
    }
}

@main
struct ManagerApp: App {
    @NSApplicationDelegateAdaptor(ManagerDelegate.self) private var delegate
    @StateObject private var model = ManagerModel()
    var body: some Scene {
        Window("ExhibitOS Manager", id: "manager") {
            ManagerView(model: model).task { delegate.model = model; await model.refresh() }
        }
        .defaultSize(width: 960, height: 680)
        .commands {
            CommandMenu("전시") {
                Button("상태 다시 확인") { Task { await model.refresh() } }
                    .keyboardShortcut("r").disabled(model.busy)
                Button("로컬 제작 시작") { Task { await model.prepareAndStart() } }.disabled(!model.canStart)
                Button("로컬 제작 정지") { Task { await model.stop() } }.disabled(!model.canStop)
                Divider()
                ForEach(Step.allCases, id: \.self) { step in
                    Button(step.title) { model.change(step: step) }
                        .disabled(model.quitting || model.presentation == nil)
                }
            }
            CommandMenu("보기") {
                Button("초보자 모드") { model.change(mode: .beginner) }
                Button("전문가 모드") { model.change(mode: .expert) }
            }
        }
        Settings {
            Form {
                Picker("글자 크기", selection: Binding(get: { model.presentation?.textScale ?? 100 }, set: { value in model.change(scale: value) })) {
                    ForEach([100, 125, 150, 175, 200], id: \.self) { Text("\($0)%").tag($0) }
                }.disabled(model.quitting || model.presentation == nil)
                    Text("창을 닫아도 로컬 제작은 계속됩니다. 앱을 종료하면 로컬 제작을 정지합니다.").foregroundStyle(.secondary)
            }.padding().frame(width: 420)
        }
    }
}

struct ManagerView: View {
    @ObservedObject var model: ManagerModel
    @FocusState private var accountField: AccountField?
    private enum AccountField { case subject, password }
    @ScaledMetric(relativeTo: .body) private var bodySize = 13.0
    @ScaledMetric(relativeTo: .title2) private var headingSize = 24.0
    private var scale: CGFloat { CGFloat(model.presentation?.textScale ?? 100) / 100 }
    var body: some View {
        NavigationSplitView {
            List(selection: Binding<Step?>(get: { model.presentation?.step }, set: { step in
                if let step { model.change(step: step) }
            })) {
                ForEach(Step.allCases, id: \.self) { step in Label(step.title, systemImage: step.symbol).tag(step) }
            }.disabled(model.quitting || model.presentation == nil)
                .navigationSplitViewColumnWidth(min: 180, ideal: 210, max: 320)
        } detail: {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    Text(model.presentation?.step.title ?? "전시 준비").font(.system(size: headingSize * scale, weight: .semibold)).accessibilityAddTraits(.isHeader)
                    if model.quitting { ProgressView(model.finalizingRestoration ? "복원 최종 확인을 마친 뒤 종료합니다…" : "로컬 작업을 정지한 뒤 종료합니다…") }
                    else if model.runtimeBusy { ProgressView(operationTitle) }
                    else if model.busy { ProgressView("확인 중…").accessibilityLabel("상태 확인 중") }
                    if let failure = model.failure {
                        VStack(alignment: .leading, spacing: 8) {
                            Label(failure.localizedDescription, systemImage: "exclamationmark.triangle")
                            Button("다시 확인") { Task { await model.refresh() } }.disabled(model.busy)
                            DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) }
                        }
                    }
                    if model.presentation != nil { content }
                    if model.presentation?.mode == .expert { details }
                    Spacer(minLength: 0)
                }.font(.system(size: bodySize * scale)).padding(28).frame(maxWidth: 800, alignment: .leading)
            }
        }
        .frame(minWidth: 680, minHeight: 480)
        .toolbar {
            ToolbarItem {
                Picker("사용 모드", selection: Binding(get: { model.presentation?.mode ?? .beginner }, set: { mode in model.change(mode: mode) })) {
                    Text("초보자").tag(Mode.beginner)
                    Text("전문가").tag(Mode.expert)
                }.disabled(model.quitting || model.presentation == nil).help("같은 전시와 작업을 두 모드에서 사용합니다.")
            }
            ToolbarItem { Button("다시 확인", systemImage: "arrow.clockwise") { Task { await model.refresh() } }.disabled(model.busy) }
        }
    }
    @ViewBuilder private var content: some View {
        switch model.presentation?.step ?? .prepare {
        case .prepare:
            Label(model.ready ? "이 Mac에서 제작할 준비가 되었습니다." : "작품과 전시를 만들 준비를 하세요.", systemImage: model.ready ? "checkmark.circle" : "desktopcomputer")
            Text("작품은 이 Mac에 저장됩니다. 인터넷 공개는 나중에 선택합니다.").foregroundStyle(.secondary)
            if model.native?.accountRequired == true && model.native?.configuration == nil {
                Form {
                    TextField("처음 사용할 계정", text: $model.accountSubject)
                        .focused($accountField, equals: .subject)
                        .textContentType(.username)
                        .onSubmit { accountField = .password }
                    SecureField("비밀번호", text: $model.accountPassword)
                        .focused($accountField, equals: .password)
                        .textContentType(.newPassword)
                    Text("계정은 영문·숫자 등 3–128자, 비밀번호는 12자 이상을 사용하세요.").foregroundStyle(.secondary)
                }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true)
                    .disabled(model.runtimeBusy || model.quitting)
            }
            HStack {
                if !model.ready {
                    Button(model.native?.configuration == nil ? "이 Mac에서 시작하기" : "로컬 제작 시작") { Task { await model.prepareAndStart() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(!model.canStart)
                } else {
                    Button("작품 만들기") { model.change(step: .create) }.buttonStyle(.borderedProminent)
                }
                if model.canStop {
                    Button(model.runtimeBusy ? "작업 정지 요청" : "로컬 제작 정지") { Task { await model.stop() } }
                }
            }
            if let runtime = model.native?.runtime, runtime.state == "stopping" {
                Text("로컬 작업이 정지되기를 기다리고 있습니다. 데이터는 유지됩니다.").foregroundStyle(.secondary)
            }
            if let code = model.native?.runtime?.errorCode, model.failure == nil {
                Text(BridgeFailure(code).localizedDescription).foregroundStyle(.secondary)
            }
            NativeMaintenanceView(model: model)
        case .create:
            NativeAuthoringView(model: model.authoring, configuration: model.native?.configuration, ready: model.ready, connect: { await model.connectPersonalAuthoring() })
        case .select:
            NativeExhibitionView(model: model.exhibitions, ready: model.ready, connect: { await model.connectPersonalAuthoring() })
        case .preview:
            Text("공개 전에 현재 전시와 작품을 살펴보세요.").foregroundStyle(.secondary)
            NativeViewportArea(model: model.exhibitions, ready: model.ready, editable: false, controller: model.exhibitions.viewportController)
            NativeOEXExportControls(model: model.exhibitions, ready: model.ready, showFeedback: true)
                .task(id: model.ready) { if model.ready { await model.connectPersonalAuthoring() } }
        case .publish:
            NativePublicationView(model: model.publication, source: model.exhibitions, staticExport: model.staticExport, hosting: model.hosting, ready: model.ready, connect: { await model.connectPersonalAuthoring() })
        }
        if !model.jobs.isEmpty {
            GroupBox("작업 기록") {
                ForEach(model.jobs) { job in
                    VStack(alignment: .leading) {
                        Text("\(job.action) · \(job.state)")
                        if let guidance = job.guidance { Text(guidance).foregroundStyle(.secondary) }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }
    private var operationTitle: String {
        switch model.native?.operation {
        case "preparing": "제작 환경을 준비하고 있습니다…"
        case "starting": "로컬 제작을 시작하고 있습니다…"
        case "stopping": "로컬 작업을 정지하고 있습니다…"
        case "backing_up": "백업을 저장하고 무결성을 확인하고 있습니다…"
        case "restoring": "새 작업 공간에 백업을 복원하고 있습니다…"
        case "activating_restore": "복원한 작업 공간을 다시 확인하고 있습니다…"
        default: "상태를 확인하고 있습니다…"
        }
    }
    private var details: some View {
        GroupBox("상세 상태") {
            VStack(alignment: .leading, spacing: 8) {
                Text("실행 상태: \(model.native?.runtime?.state ?? (model.runtimeBusy ? operationTitle : "정지됨"))")
                if let context = model.context,
                   let selected = context.installations.first(where: { $0.id == context.activeId }) {
                    Text("저장 위치: \(selected.path)").textSelection(.enabled)
                }
                if let runtime = model.native?.runtime {
                    Text("작업: \(runtime.jobId ?? "없음")").textSelection(.enabled)
                    Text("로컬 포트: \(runtime.ports.web) · API \(runtime.ports.api) · DB \(runtime.ports.database)")
                }
                Text("창 닫기는 제작을 유지합니다. 앱 종료는 로컬 제작을 정지하며 인터넷에 공개한 전시는 유지합니다.")
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

#Preview("초보자 · Light") {
    ManagerView(model: ManagerModel(preview: Presentation())).preferredColorScheme(.light)
}
#Preview("전문가 · Dark") {
    ManagerView(model: ManagerModel(preview: Presentation(mode: .expert))).preferredColorScheme(.dark)
}
#Preview("200% · 좁은 창") {
    ManagerView(model: ManagerModel(preview: Presentation(textScale: 200)))
        .frame(width: 680, height: 480)
}
