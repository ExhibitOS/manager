// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import WebKit
import CryptoKit
import ManagerCore

@MainActor
final class NativeViewportController: ObservableObject {
    @Published var connected = false
    @Published fileprivate(set) var planSupported = false
    @Published var rendering = false
    @Published var loaded = 0
    @Published var failed = 0
    @Published var message = "3D 화면을 준비하고 있습니다."
    @Published var diagnostic = ""
    func diagnose(_ stage: String, _ error: Error) {
        let e = error as NSError
        let domain = e.domain.range(of: "^[A-Za-z0-9_.-]{1,80}$", options: .regularExpression) == nil ? "WebKit" : e.domain
        let detail = (e.userInfo["WKJavaScriptExceptionMessage"] as? String)?.replacingOccurrences(of: "Error: ", with: "") ?? ""
        let known = ["crypto.randomUUID", "structuredClone", "INVALID_GEOMETRY", "VIEWPORT_COMMAND_DENIED", "VIEWPORT_NOT_READY", "func2"].filter { detail.contains($0) }.joined(separator: ",")
        let code = detail.range(of: "^[A-Z0-9_]{1,180}$", options: .regularExpression) == nil ? "" : " · " + detail
        diagnostic = "\(stage) · \(domain) · \(e.code)" + code + (known.isEmpty ? "" : " · " + known)
    }
    fileprivate var owner: String?
    fileprivate var action: ((NativeGeometryAction) async throws -> (DocumentValue, NativeExhibitionDraft))?
    func perform(_ command: NativeGeometryAction, model: ExhibitionModel) async {
        guard let action, connected else { return }
        do {
            let (candidate, base) = try await action(command)
            if case .installation(let installation) = command { await model.acceptInstallationCandidate(candidate, base: base, command: installation) }
            else if case .architecture(let architecture) = command { await model.acceptArchitectureCandidate(candidate, base: base, command: architecture) }
            else if case .surfaceMaterial(let material) = command { await model.acceptSurfaceMaterialPreview(candidate, base: base, command: material) }
            else { await model.acceptViewportCandidate(candidate, base: base) }
        }
        catch {
            message = "이 조작을 적용하지 못했습니다. 현재 전시와 선택을 확인하세요."
            if case .architecture = command { message = "이 공간 편집을 적용하지 못했습니다. 입력은 유지되며 기존 전시는 바뀌지 않았습니다." }
            if case .surfaceMaterial = command { message = "재질을 미리 보지 못했습니다. 입력과 원래 재질은 유지됩니다." }
            if case .installation = command,
               let detail = (error as NSError).userInfo["WKJavaScriptExceptionMessage"] as? String,
               let reason = InstallationCommandReason.fromJavaScriptMessage(detail) { message = reason.explanation }
            diagnose("COMMAND", error)
        }
    }
}

struct NativeViewport: NSViewRepresentable {
    let credentials: LocalAuthoringSession
    let draft: NativeExhibitionDraft
    let selection: ViewportSelection?
    let presentation: ViewportPresentation
    let mode: String
    let view: String
    let cameraSequence: Int
    let transparent: Bool
    let disabled: Bool
    let interactionDisabled: Bool
    @ObservedObject var controller: NativeViewportController
    let onSelect: (ViewportSelection) -> Void
    let onCandidate: (DocumentValue, NativeExhibitionDraft) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: NSViewRepresentableContext<NativeViewport>) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.userContentController.add(context.coordinator, name: "exhibitosViewport")
        let web = WKWebView(frame: .zero, configuration: configuration)
        web.navigationDelegate = context.coordinator; web.uiDelegate = context.coordinator
        web.allowsBackForwardNavigationGestures = false; web.allowsLinkPreview = false
        web.setAccessibilityLabel("전시 편집 화면")
        context.coordinator.web = web
        context.coordinator.update(self)
        return web
    }
    func updateNSView(_ nsView: WKWebView, context: NSViewRepresentableContext<NativeViewport>) {
        nsView.setAccessibilityLabel(presentation == .plan && controller.planSupported ? "전시 2D 평면 편집 화면" : "전시 3D 편집 화면")
        context.coordinator.update(self)
    }
    static func dismantleNSView(_ web: WKWebView, coordinator: Coordinator) {
        coordinator.invalidate()
        web.stopLoading(); web.navigationDelegate = nil; web.uiDelegate = nil
        web.configuration.userContentController.removeScriptMessageHandler(forName: "exhibitosViewport")
    }
    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
        weak var web: WKWebView?
        var parent: NativeViewport
        var controller: NativeViewportController { parent.controller }
        var generation = UUID()
        private var active = true
        private var epoch = UUID().uuidString.lowercased()
        private var sequence = 0
        private var sent = 0
        private var fingerprint: Data?
        private var installedToken: String?
        private var prepared = false
        private var latest: ViewportFrame?
        var sendTask: Task<Void, Never>?
        init(_ parent: NativeViewport) { self.parent = parent; super.init(); parent.controller.owner = epoch }
        func invalidate() {
            active = false; generation = UUID(); sendTask?.cancel()
            if controller.owner == epoch { controller.action = nil; controller.connected = false; controller.planSupported = false }
        }
        private var page: URL? { URL(string: parent.credentials.origin + "/native-viewport.html") }
        private func isPage(_ url: URL?) -> Bool { url == page && url?.user == nil && url?.password == nil }
        func update(_ value: NativeViewport) {
            parent = value
            guard let page, page.scheme == "http", page.host == "127.0.0.1", let port = page.port, port >= 1024, UUID(uuidString: value.credentials.tenantId) != nil, UUID(uuidString: value.credentials.principalId) != nil else { return }
            if let installedToken, installedToken != value.credentials.token {
                prepared = false; controller.connected = false; controller.planSupported = false; web?.stopLoading(); return
            }
            if installedToken == nil {
                installedToken = value.credentials.token
                generation = UUID(); sequence = 0; sent = 0; fingerprint = nil; prepared = false
                sendTask?.cancel(); sendTask = nil
                controller.connected = false; controller.rendering = false; controller.planSupported = false
                let installGeneration = generation
                Task { await install(page: page, generation: installGeneration) }
            }
            let selection = value.selection.flatMap { $0.belongs(to: value.draft) ? $0 : nil }
            let probe = ViewportFrame(epoch: epoch, sequence: 1, tenantId: value.credentials.tenantId, principalId: value.credentials.principalId, draft: value.draft, selection: selection, mode: value.mode, view: value.view, disabled: value.disabled, cameraSequence: value.cameraSequence, transparent: value.transparent, presentation: controller.planSupported ? value.presentation : .threeD)
            let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
            guard let bytes = try? encoder.encode(probe) else { return }
            let hash = Data(SHA256.hash(data: bytes))
            guard hash != fingerprint else { return }
            fingerprint = hash; sequence += 1
            latest = ViewportFrame(epoch: epoch, sequence: sequence, tenantId: value.credentials.tenantId, principalId: value.credentials.principalId, draft: value.draft, selection: selection, mode: value.mode, view: value.view, disabled: value.disabled, cameraSequence: value.cameraSequence, transparent: value.transparent, presentation: controller.planSupported ? value.presentation : .threeD)
            schedule()
        }
        private func install(page: URL, generation: UUID) async {
            guard active, self.generation == generation, let web, parent.credentials.token.range(of: "^[A-Za-z0-9_-]{43}$", options: .regularExpression) != nil else { return }
            var stage = "COOKIE"
            do {
                // The token never enters JS, a URL, an argument, log or disk store.
                let headers = ["Set-Cookie":"exhibitos_local_session=\(parent.credentials.token); Path=/; HttpOnly; SameSite=Strict"]
                guard let cookie = HTTPCookie.cookies(withResponseHeaderFields: headers, for: page).first, cookie.isHTTPOnly else { throw BridgeFailure("VIEWPORT_COOKIE_INVALID") }
                let store = web.configuration.websiteDataStore.httpCookieStore
                for cookie in await store.allCookies() { await store.deleteCookie(cookie) }
                await store.setCookie(cookie)
                stage = "RESOURCE_POLICY"
                let encodedRules = try ViewportResourcePolicy.encodedRules(origin: parent.credentials.origin, tenantId: parent.credentials.tenantId)
                let identifier = "ExhibitOSViewport-" + UUID().uuidString
                let rule = try await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: encodedRules)
                try? await WKContentRuleListStore.default().removeContentRuleList(forIdentifier: identifier)
                guard active, self.generation == generation else { return }
                guard let rule else { throw BridgeFailure("VIEWPORT_POLICY_UNAVAILABLE") }
                web.configuration.userContentController.removeAllContentRuleLists(); web.configuration.userContentController.add(rule)
                web.load(URLRequest(url: page, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 15))
            } catch { if self.generation == generation { controller.message = "3D 화면 연결을 준비하지 못했습니다. 작업 공간을 다시 연결하세요."; controller.diagnose(stage, error) } }
        }
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            guard webView === web, isPage(webView.url) else { return }
            prepared = true; schedule()
        }
        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
            navigationAction.targetFrame?.isMainFrame == true && isPage(navigationAction.request.url) ? .allow : .cancel
        }
        func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse) async -> WKNavigationResponsePolicy {
            guard navigationResponse.isForMainFrame, isPage(navigationResponse.response.url), navigationResponse.response.mimeType == "text/html", (navigationResponse.response as? HTTPURLResponse)?.statusCode == 200 else { return .cancel }
            return .allow
        }
        func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? { nil }
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { prepared = false; controller.connected = false; controller.planSupported = false; controller.rendering = false; controller.message = "3D 화면이 닫혔습니다. 전시는 유지됩니다. 3D 다시 열기를 선택하세요." }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { controller.message = "3D 화면을 열지 못했습니다. 로컬 제작 상태를 확인하세요."; controller.diagnose("PAGE_LOAD", error) }
        private func schedule() {
            guard active, controller.owner == epoch, prepared, sendTask == nil, let web else { return }
            let generation = generation
            sendTask = Task { [weak self] in
                guard let self else { return }
                defer { self.sendTask = nil }
                while !Task.isCancelled, self.generation == generation, let frame = self.latest, frame.sequence > self.sent {
                    do {
                        let bytes = try JSONEncoder().encode(frame)
                        guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                        // Foundation can intern empty NSArray containers. Passing a JSON string
                        // gives every document collection its own JS array, as the wire format requires.
                        let value = String(decoding: bytes, as: UTF8.self)
                        let response = try await web.callAsyncJavaScript("const deadline = Date.now() + 10000; while (!window.exhibitosViewport && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 25)); if (!window.exhibitosViewport) throw new Error('VIEWPORT_ENTRY_UNAVAILABLE'); return await window.exhibitosViewport.accept(JSON.parse(frameJSON))", arguments: ["frameJSON":value], in: nil, contentWorld: .page)
                        guard self.generation == generation else { return }
                        guard let response, JSONSerialization.isValidJSONObject(response),
                              let acceptance = try? JSONDecoder().decode(ViewportAcceptance.self, from: JSONSerialization.data(withJSONObject: response)), acceptance.accepted else { throw BridgeFailure("VIEWPORT_ACCEPT_INVALID") }
                        self.controller.planSupported = acceptance.supportsPlan
                        self.sent = frame.sequence; self.controller.connected = self.sent == self.sequence; self.controller.diagnostic = ""
                    } catch { if self.generation == generation { self.controller.connected = false; self.controller.message = "전시를 3D에 표시하지 못했습니다. 저장한 전시와 연결을 확인하세요."; self.controller.diagnose("FRAME", error) }; break }
                }
            }
            controller.action = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                let generation = self.generation
                let value = String(decoding: try JSONEncoder().encode(command.encodedValue(in: frame.draft, selected: frame.selection)), as: UTF8.self)
                let candidate = try await web.callAsyncJavaScript("try { return window.exhibitosViewport.action(JSON.parse(actionJSON)); } catch (error) { if (error.code === 'INVALID_GEOMETRY' && Array.isArray(error.issues)) { const codes = error.issues.slice(0, 1).map(issue => issue.code + '_' + String(issue.path).replace(/[^a-zA-Z0-9_]/g, '_').toUpperCase() + '_' + (/^[a-zA-Z]{1,30}$/.test(issue.message) ? issue.message.toUpperCase() : '')).filter(code => /^[A-Z0-9_]{1,150}$/.test(code)); throw new Error('INVALID_GEOMETRY_' + codes.join('_')); } throw error; }", arguments: ["actionJSON":value], in: nil, contentWorld: .page)
                guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, let candidate, JSONSerialization.isValidJSONObject(candidate) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: candidate)
                guard bytes.count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return (try JSONDecoder().decode(DocumentValue.self, from: bytes), frame.draft)
            }
        }
        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
            guard active, controller.owner == epoch, message.webView === web, message.frameInfo.isMainFrame, isPage(web?.url), message.frameInfo.securityOrigin.protocol == "http", message.frameInfo.securityOrigin.host == "127.0.0.1", message.frameInfo.securityOrigin.port == page?.port,
                  let fields = message.body as? [String: Any], Set(fields.keys) == Set(["version", "epoch", "sequence", "type", "body"]),
                  JSONSerialization.isValidJSONObject(message.body), let bytes = try? JSONSerialization.data(withJSONObject: message.body), bytes.count <= 1_100_000,
                  let envelope = try? JSONDecoder().decode(Envelope.self, from: bytes), envelope.version == 1, envelope.epoch == epoch, envelope.sequence == sequence, let frame = latest else { return }
            switch envelope.type {
            case "selection":
                guard !frame.disabled, !parent.interactionDisabled, let selection = try? JSONDecoder().decode(ViewportSelection.self, from: JSONEncoder().encode(envelope.body)), selection.belongs(to: frame.draft) else { return }
                parent.onSelect(selection)
            case "candidate": guard !frame.disabled, !parent.interactionDisabled else { return }; parent.onCandidate(envelope.body, frame.draft)
            case "status":
                guard let report = try? JSONDecoder().decode(Report.self, from: JSONEncoder().encode(envelope.body)), report.loaded >= 0, report.failed >= 0, report.message.count <= 2048 else { return }
                controller.rendering = report.ready; controller.loaded = report.loaded; controller.failed = report.failed
                controller.message = !report.ready ? "편집 화면을 준비하고 있습니다." : report.loaded + report.failed < frame.draft.placements.count ? "전시 작품을 불러오고 있습니다." : report.failed > 0 ? "일부 작품을 불러오지 못했습니다. 작업 공간과 작품 사용 권한을 확인하세요." : "전시 편집 화면을 표시했습니다."
            case "error": controller.message = "3D 조작을 적용하지 못했습니다. 현재 전시와 선택은 유지됩니다."
            default: break
            }
        }
        private struct Envelope: Decodable { let version: Int; let epoch: String; let sequence: Int; let type: String; let body: DocumentValue }
        private struct Report: Decodable { let ready: Bool; let message: String; let loaded: Int; let failed: Int }
    }
}

struct NativeViewportArea: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    var editable = true
    @ObservedObject var controller: NativeViewportController
    @State private var identity = UUID()
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let credentials = model.viewportCredentials, let draft = model.viewportDraft, ready {
                Picker("편집 화면", selection: Binding(get: { controller.planSupported ? model.viewportPresentation : .threeD }, set: { value in
                    if value == .threeD || controller.planSupported { model.viewportPresentation = value }
                })) {
                    Text("3D 공간").tag(ViewportPresentation.threeD)
                    Text("2D 평면").tag(ViewportPresentation.plan).disabled(!controller.planSupported)
                }.pickerStyle(.segmented)
                    .disabled(!controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying)
                    .accessibilityHint("같은 전시와 선택을 유지하며 편집 화면을 전환합니다.")
                if !controller.planSupported { Text("이 제작 서비스는 3D 화면을 제공합니다. 2D 평면 지원 연결을 확인하세요.").foregroundStyle(.secondary) }
                HStack {
                    Button("전체 공간") { model.requestViewportView("isometric") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    Button("위에서 보기") { model.requestViewportView("top") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    Button("정면 보기") { model.requestViewportView("front") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    if editable {
                        Picker("편집 조작", selection: $model.viewportMode) { Text("이동").tag("translate"); Text("회전").tag("rotate"); Text("크기").tag("dimensions") }
                    }
                    Button("편집 화면 다시 열기") { identity = UUID() }
                }
                Toggle("공간을 투명하게 보기", isOn: $model.viewportTransparent)
                    .help("벽·바닥 너머 작품을 확인합니다. 저장한 재질은 바뀌지 않습니다.")
                NativeViewport(credentials: credentials, draft: draft, selection: model.viewportSelection, presentation: model.viewportPresentation, mode: model.viewportMode, view: model.viewportView, cameraSequence: model.viewportCameraSequence, transparent: model.viewportTransparent, disabled: !editable || model.busy || model.pending || model.formDirty || model.positionDirty || model.surfaceMaterialPreviewing, interactionDisabled: !editable || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.surfaceMaterialPreviewing, controller: controller, onSelect: { model.selectViewport($0) }, onCandidate: { candidate, base in Task { await model.acceptViewportCandidate(candidate, base: base) } })
                    .id(identity.uuidString + model.viewportAccessID.uuidString).frame(minHeight: 360, idealHeight: 440).clipShape(RoundedRectangle(cornerRadius: 8))
                    .allowsHitTesting(!model.architectureDirty && !model.architectureApplying)
                    .disabled(model.architectureDirty || model.architectureApplying)
                Text("불러온 작품 \(controller.loaded)개 · 불러오지 못한 작품 \(controller.failed)개").foregroundStyle(.secondary)
                Text(controller.message).foregroundStyle(.secondary)
                if !controller.diagnostic.isEmpty { DisclosureGroup("3D 연결 상세") { Text(controller.diagnostic).textSelection(.enabled) } }
                if editable {
                    HStack {
                        Button("벽과 바닥 만들기") { Task { await controller.perform(.whiteCube(roomId: model.roomId), model: model) } }
                        Button("공간 추가") { Task { await controller.perform(.addRoom(name: "새 공간"), model: model) } }
                        Button("조명 추가") { Task { await controller.perform(.addLight(roomId: model.roomId), model: model) } }
                    }.disabled(!controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying)
                    Text("숫자 입력은 이 Mac에 보관한 뒤 3D에 반영됩니다. 3D 변경도 이 Mac에 보관하며, 전시 저장과 공개는 별도로 진행합니다.").foregroundStyle(.secondary)
                }
            } else { Text("전시를 열고 로컬 제작을 시작하면 3D를 볼 수 있습니다.").foregroundStyle(.secondary) }
        }
    }
}
