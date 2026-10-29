// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import WebKit
import CryptoKit
import ManagerCore

struct NativeEntrancePreviewInput {
    let action: EntrancePreviewAction
    let device: EntrancePreviewDevice
    let scenario: EntrancePreviewScenario
    let reducedMotion: Bool
    let theme: CoverEntryTheme?
    let locale: String?
    let draft: NativeExhibitionDraft
}
struct NativeEntrancePreviewOutput {
    let request: ViewportEntrancePreviewRequest
    let png: Data?
}

struct NativeDisplayReadoutInput {
    let draft: NativeExhibitionDraft
    let selection: ViewportSelection?
    let view: String
    let cameraSequence: Int
    let transparent: Bool
    let presentation: ViewportPresentation
}

@MainActor
final class NativeViewportController: ObservableObject {
    // Synchronously updated by native navigation, before a delayed WK frame or
    // queued candidate callback can act on the former editing purpose.
    private(set) var editingContextGeneration = UUID()
    @Published var editingContextActive = true {
        didSet { if oldValue != editingContextActive { editingContextGeneration = UUID(); displayReadout = "" } }
    }
    @Published var connected = false
    @Published fileprivate(set) var planSupported = false
    @Published fileprivate(set) var thumbnailSupported = false
    @Published fileprivate(set) var findSelectionSupported = false
    @Published fileprivate(set) var placementSupported = false
    @Published fileprivate(set) var furnishingVisualsSupported = false
    @Published fileprivate(set) var spaceSettingsSupported = false
    @Published fileprivate(set) var lightInspectorSupported = false
    @Published fileprivate(set) var lightFixtureSupported = false
    @Published fileprivate(set) var benchInspectorSupported = false
    @Published fileprivate(set) var plaqueInspectorSupported = false
    @Published fileprivate(set) var surfaceMaterialTexturesSupported = false
    @Published fileprivate(set) var entrancePreviewSupported = false
    fileprivate var acknowledgedFrame: ViewportFrame?
    @Published fileprivate(set) var displayReadout = ""
    @Published private(set) var readingDisplay = false
    fileprivate var displayReadoutAction: ((NativeDisplayReadoutInput) async throws -> String)?
    func readDisplayStatus(_ input: NativeDisplayReadoutInput) async {
        guard connected, editingContextActive, !readingDisplay, let displayReadoutAction else { return }
        let context = editingContextGeneration, epoch = owner, accepted = acceptedSequence
        readingDisplay = true; displayReadout = ""
        defer { readingDisplay = false }
        do {
            let value = try await displayReadoutAction(input)
            guard connected, editingContextActive, editingContextGeneration == context, owner == epoch,
                  acceptedSequence == accepted, acknowledgedFrame?.draft == input.draft,
                  acknowledgedFrame?.selection == input.selection else { return }
            displayReadout = value
        } catch {
            guard editingContextActive, editingContextGeneration == context, owner == epoch else { return }
            diagnose("DISPLAY_READ", error)
        }
    }
    var hostEpoch: String? { owner }
    func confirmsFloorPreview(_ draft: NativeExhibitionDraft, tenantId: String, principalId: String) -> Bool {
        connected && acknowledgedFrame?.confirmsFloorPreview(ownerEpoch: owner, acceptedSequence: acceptedSequence,
            expectedDraft: draft, tenantId: tenantId, principalId: principalId) == true
    }
    func ordinaryRecoveryReady(replacing previousEpoch: String, draft: NativeExhibitionDraft, tenantId: String, principalId: String) -> Bool {
        guard connected, let acknowledgedFrame else { return false }
        return acknowledgedFrame.confirmsHostRecovery(replacing: previousEpoch, ownerEpoch: owner, acceptedSequence: acceptedSequence,
                                                     expectedDraft: draft, tenantId: tenantId, principalId: principalId)
    }
    func waitForOrdinaryRecovery(replacing previousEpoch: String, draft: NativeExhibitionDraft, tenantId: String, principalId: String) async throws {
        for _ in 0..<100 {
            try Task.checkCancellation()
            if ordinaryRecoveryReady(replacing: previousEpoch, draft: draft, tenantId: tenantId, principalId: principalId) { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw AuthoringFailure("VIEWPORT_FRAME_STALE")
    }
    fileprivate var entrancePreviewAction: ((NativeEntrancePreviewInput) async throws -> NativeEntrancePreviewOutput)?
    func entrancePreview(_ input: NativeEntrancePreviewInput) async throws -> NativeEntrancePreviewOutput {
        // The explicitly staged display draft must pass the ordinary frame
        // handshake before any preview request can name that frame.
        for _ in 0..<100 {
            try Task.checkCancellation()
            if connected, acknowledgedFrame?.disabled == true, acknowledgedFrame?.draft == input.draft {
                guard entrancePreviewSupported, let entrancePreviewAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                return try await entrancePreviewAction(input)
            }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw AuthoringFailure("VIEWPORT_FRAME_STALE")
    }
    @Published fileprivate(set) var acceptedSequence = 0
    fileprivate var thumbnailAction: ((String, Int) async throws -> Data)?
    fileprivate var findSelectionAction: (() async throws -> Void)?
    fileprivate var placementAction: ((ArtworkPlacementIntent, ViewportPlacementPhase, ViewportPlacementTarget?) async throws -> DocumentValue?)?
    fileprivate var furnishingVisualsAction: ((FurnishingCommand) async throws -> FurnishingPreview)?
    fileprivate var furnishingThumbnailAction: (() async throws -> Data)?
    fileprivate var spaceSettingsAction: ((SpaceLightingCommand) async throws -> SpaceLightingPreview)?
    fileprivate var lightInspectorAction: ((LightCommand, String) async throws -> LightPreview)?
    fileprivate var benchInspectorAction: ((BenchCommand, String) async throws -> BenchPreview)?
    fileprivate var plaqueInspectorAction: ((PlaqueCommand, String) async throws -> PlaquePreview)?
    func plaqueInspector(_ command: PlaqueCommand, selectedPlaqueId: String) async throws -> PlaquePreview {
        guard connected, plaqueInspectorSupported, let plaqueInspectorAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await plaqueInspectorAction(command, selectedPlaqueId)
    }
    func benchInspector(_ command: BenchCommand, selectedBenchId: String) async throws -> BenchPreview {
        guard connected, benchInspectorSupported, let benchInspectorAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await benchInspectorAction(command, selectedBenchId)
    }
    func furnishingVisuals(_ command: FurnishingCommand) async throws -> FurnishingPreview {
        guard connected, furnishingVisualsSupported, let furnishingVisualsAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await furnishingVisualsAction(command)
    }
    func furnishingThumbnail() async throws -> Data {
        guard connected, furnishingVisualsSupported, let furnishingThumbnailAction else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        return try await furnishingThumbnailAction()
    }
    func spaceSettings(_ command: SpaceLightingCommand) async throws -> SpaceLightingPreview {
        guard connected, spaceSettingsSupported, let spaceSettingsAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await spaceSettingsAction(command)
    }
    func lightInspector(_ command: LightCommand, selectedLightId: String) async throws -> LightPreview {
        guard connected, lightInspectorSupported, command.fixture == nil || lightFixtureSupported,
              let lightInspectorAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await lightInspectorAction(command, selectedLightId)
    }
    func placement(_ intent: ArtworkPlacementIntent, phase: ViewportPlacementPhase, target: ViewportPlacementTarget? = nil) async throws -> DocumentValue? {
        guard connected, placementSupported, let placementAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await placementAction(intent, phase, target)
    }
    func thumbnail(artworkId: String, revision: Int) async throws -> Data {
        guard connected, thumbnailSupported, let thumbnailAction else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        return try await thumbnailAction(artworkId, revision)
    }
    func findSelection() async {
        guard connected, findSelectionSupported, let findSelectionAction else { return }
        do { try await findSelectionAction(); message = "선택한 배치가 보이도록 시점을 맞췄습니다." }
        catch { message = "선택한 배치를 찾지 못했습니다. 선택과 기존 시점은 유지됩니다."; diagnose("FIND", error) }
    }
    @Published var rendering = false
    @Published var loaded = 0
    @Published var failed = 0
    @Published var message = "3D 화면을 준비하고 있습니다."
    @Published var diagnostic = ""
    func diagnose(_ stage: String, _ error: Error) {
        let e = error as NSError
        let domain = e.domain.range(of: "^[A-Za-z0-9_.-]{1,80}$", options: .regularExpression) == nil ? "WebKit" : e.domain
        let detail = (error as? AuthoringFailure)?.code ?? (e.userInfo["WKJavaScriptExceptionMessage"] as? String)?.replacingOccurrences(of: "Error: ", with: "") ?? ""
        let known = ["crypto.randomUUID", "structuredClone", "INVALID_GEOMETRY", "VIEWPORT_COMMAND_DENIED", "VIEWPORT_NOT_READY", "func2"].filter { detail.contains($0) }.joined(separator: ",")
        let code = detail.range(of: "^[A-Z0-9_]{1,180}$", options: .regularExpression) == nil ? "" : " · " + detail
        diagnostic = "\(stage) · \(domain) · \(e.code)" + code + (known.isEmpty ? "" : " · " + known)
    }
    fileprivate var owner: String?
    fileprivate var action: ((NativeGeometryAction) async throws -> (DocumentValue, NativeExhibitionDraft))?
    func perform(_ command: NativeGeometryAction, model: ExhibitionModel) async {
        guard editingContextActive, !model.placingArtwork, let action, connected else { return }
        let contextGeneration = editingContextGeneration
        do {
            let (candidate, base) = try await action(command)
            guard editingContextActive, editingContextGeneration == contextGeneration else { return }
            if case .installation(let installation) = command { await model.acceptInstallationCandidate(candidate, base: base, command: installation) }
            else if case .architecture(let architecture) = command { await model.acceptArchitectureCandidate(candidate, base: base, command: architecture) }
            else if case .surfaceMaterial(let material) = command { await model.acceptSurfaceMaterialPreview(candidate, base: base, command: material) }
            else if case .surfaceMaterialTexture(let material) = command { await model.acceptSurfaceMaterialTexturePreview(candidate, base: base, command: material) }
            else { await model.acceptViewportCandidate(candidate, base: base) }
        }
        catch {
            message = "이 조작을 적용하지 못했습니다. 현재 전시와 선택을 확인하세요."
            if case .architecture = command { message = "이 공간 편집을 적용하지 못했습니다. 입력은 유지되며 기존 전시는 바뀌지 않았습니다." }
            if case .surfaceMaterial = command { message = "재질을 미리 보지 못했습니다. 입력과 원래 재질은 유지됩니다." }
            if case .surfaceMaterialTexture = command { message = "재질을 미리 보지 못했습니다. 입력과 원래 재질은 유지됩니다." }
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
    let onPlacementTarget: (DocumentValue) -> Void
    let onPlacementCancel: (String) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: NSViewRepresentableContext<NativeViewport>) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.userContentController.add(context.coordinator, name: "exhibitosViewport")
        let web = WKWebView(frame: .zero, configuration: configuration)
        web.pageZoom = 1 // Snapshot CSS pixels map one-to-one to native points.
        web.navigationDelegate = context.coordinator; web.uiDelegate = context.coordinator
        web.allowsBackForwardNavigationGestures = false; web.allowsLinkPreview = false
        web.setAccessibilityLabel("전시 편집 화면")
        context.coordinator.attach(web)
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
        private var editingContextGeneration: UUID?
        private var installedToken: String?
        private var prepared = false
        private var latest: ViewportFrame?
        private var placementIntent: ArtworkPlacementIntent?
        private var inspectorPending = false
        private var entranceOperation: UUID?
        var sendTask: Task<Void, Never>?
        init(_ parent: NativeViewport) { self.parent = parent; super.init() }
        func attach(_ web: WKWebView) {
            // A coordinator owns the controller only once its actual view exists.
            self.web = web
            controller.owner = epoch
        }
        func invalidate() {
            guard active else { return }
            endPlacement()
            entranceOperation = nil
            if controller.owner == epoch { controller.entrancePreviewAction = nil; controller.entrancePreviewSupported = false; controller.acknowledgedFrame = nil; controller.displayReadoutAction = nil; controller.displayReadout = "" }
            active = false; generation = UUID(); sendTask?.cancel()
            if controller.owner == epoch { controller.furnishingVisualsAction = nil; controller.furnishingThumbnailAction = nil; controller.spaceSettingsAction = nil; controller.lightInspectorAction = nil; controller.furnishingVisualsSupported = false; controller.spaceSettingsSupported = false; controller.lightInspectorSupported = false; controller.lightFixtureSupported = false; controller.benchInspectorAction = nil; controller.benchInspectorSupported = false; controller.plaqueInspectorAction = nil; controller.plaqueInspectorSupported = false; controller.surfaceMaterialTexturesSupported = false }
            if controller.owner == epoch { controller.action = nil; controller.thumbnailAction = nil; controller.findSelectionAction = nil; controller.placementAction = nil; controller.placementSupported = false; controller.connected = false; controller.planSupported = false; controller.thumbnailSupported = false; controller.findSelectionSupported = false; controller.acceptedSequence = 0 }
        }
        private func endPlacement() {
            guard let intent = placementIntent else { return }
            placementIntent = nil
            let notify = parent.onPlacementCancel
            Task { @MainActor in notify(intent.requestId) }
        }
        private var page: URL? { URL(string: parent.credentials.origin + "/native-viewport.html") }
        private func isPage(_ url: URL?) -> Bool { url == page && url?.user == nil && url?.password == nil }
        func update(_ value: NativeViewport) {
            guard active else { return }
            parent = value
            guard let page, page.scheme == "http", page.host == "127.0.0.1", let port = page.port, port >= 1024, UUID(uuidString: value.credentials.tenantId) != nil, UUID(uuidString: value.credentials.principalId) != nil else { return }
            if let installedToken, installedToken != value.credentials.token {
                // A renewed session requires the existing new-coordinator identity.
                // Revoke this page once; repeated published resets during a SwiftUI
                // update would prevent that replacement from being mounted.
                invalidate()
                web?.stopLoading()
                return
            }
            if installedToken == nil {
                entranceOperation = nil; controller.entrancePreviewSupported = false; controller.acknowledgedFrame = nil; controller.displayReadoutAction = nil; controller.displayReadout = ""
                controller.furnishingVisualsSupported = false; controller.spaceSettingsSupported = false; controller.lightInspectorSupported = false; controller.lightFixtureSupported = false; controller.benchInspectorSupported = false; controller.plaqueInspectorSupported = false; controller.surfaceMaterialTexturesSupported = false
                installedToken = value.credentials.token
                generation = UUID(); sequence = 0; sent = 0; fingerprint = nil; prepared = false
                sendTask?.cancel(); sendTask = nil
                controller.connected = false; controller.rendering = false; controller.planSupported = false; controller.thumbnailSupported = false; controller.findSelectionSupported = false; controller.acceptedSequence = 0
                let installGeneration = generation
                Task { await install(page: page, generation: installGeneration) }
            }
            let selection = value.selection.flatMap { $0.belongs(to: value.draft) ? $0 : nil }
            let probe = ViewportFrame(epoch: epoch, sequence: 1, tenantId: value.credentials.tenantId, principalId: value.credentials.principalId, draft: value.draft, selection: selection, mode: value.mode, view: value.view, disabled: value.disabled, cameraSequence: value.cameraSequence, transparent: value.transparent, presentation: controller.planSupported ? value.presentation : .threeD)
            let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
            guard let bytes = try? encoder.encode(probe) else { return }
            let hash = Data(SHA256.hash(data: bytes))
            let contextChanged = editingContextGeneration != controller.editingContextGeneration
            editingContextGeneration = controller.editingContextGeneration
            guard hash != fingerprint || contextChanged else { return }
            // Status-only rerenders dedupe above. A genuinely different frame
            // invalidates the transient operation before any late callback.
            endPlacement()
            entranceOperation = nil
            fingerprint = hash; sequence += 1
            if !controller.displayReadout.isEmpty { controller.displayReadout = "" }
            controller.connected = false
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
            guard active, controller.owner == epoch, webView === web, isPage(webView.url) else { return }
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
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { guard active, controller.owner == epoch, webView === web else { return }; entranceOperation = nil; if controller.owner == epoch { controller.entrancePreviewSupported = false; controller.acknowledgedFrame = nil; controller.displayReadoutAction = nil; controller.displayReadout = "" }; endPlacement(); prepared = false; controller.connected = false; controller.planSupported = false; controller.thumbnailSupported = false; controller.findSelectionSupported = false; controller.placementSupported = false; controller.furnishingVisualsSupported = false; controller.spaceSettingsSupported = false; controller.lightInspectorSupported = false; controller.lightFixtureSupported = false; controller.acceptedSequence = 0; controller.rendering = false; controller.message = "3D 화면이 닫혔습니다. 입력은 유지됩니다. 편집 화면 다시 열기를 선택하세요." }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            guard active, controller.owner == epoch, webView === web else { return }
            controller.message = "3D 화면을 열지 못했습니다. 로컬 제작 상태를 확인하세요."
            controller.diagnose("PAGE_LOAD", error)
        }
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
                        guard self.active, !Task.isCancelled, self.controller.owner == self.epoch, self.generation == generation else { return }
                        guard let response, JSONSerialization.isValidJSONObject(response),
                              let acceptance = try? JSONDecoder().decode(ViewportAcceptance.self, from: JSONSerialization.data(withJSONObject: response)), acceptance.accepted else { throw BridgeFailure("VIEWPORT_ACCEPT_INVALID") }
                        self.controller.planSupported = acceptance.supportsPlan
                        self.controller.thumbnailSupported = acceptance.supportsArtworkThumbnail
                        self.controller.findSelectionSupported = acceptance.supportsFindSelection
                        self.controller.placementSupported = acceptance.supportsArtworkPlacement
                        self.controller.furnishingVisualsSupported = acceptance.supportsFurnishingVisuals
                        self.controller.spaceSettingsSupported = acceptance.supportsSpaceSettings
                        self.controller.lightInspectorSupported = acceptance.supportsLightInspector
                        self.controller.lightFixtureSupported = acceptance.supportsLightFixture
                        self.controller.benchInspectorSupported = acceptance.supportsBenchInspector
                        self.controller.plaqueInspectorSupported = acceptance.supportsPlaqueInspector
                        self.controller.surfaceMaterialTexturesSupported = acceptance.supportsSurfaceMaterialTextures
                        self.controller.entrancePreviewSupported = acceptance.supportsEntrancePreview
                        self.controller.acknowledgedFrame = frame
                        self.controller.displayReadout = ""
                        self.controller.acceptedSequence = frame.sequence
                        self.sent = frame.sequence; self.controller.connected = self.sent == self.sequence; self.controller.diagnostic = ""
                    } catch { if self.generation == generation { self.controller.connected = false; self.controller.message = "전시를 3D에 표시하지 못했습니다. 저장한 전시와 연결을 확인하세요."; self.controller.diagnose("FRAME", error) }; break }
                }
            }
            controller.displayReadoutAction = { [weak self] input in
                guard let self, self.active, self.prepared, let web = self.web, let frame = self.latest,
                      frame.draft == input.draft, frame.selection == input.selection,
                      frame.view == input.view, frame.cameraSequence == input.cameraSequence,
                      frame.transparent == input.transparent, frame.presentation == input.presentation,
                      frame.presentation == .threeD else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let generation = self.generation, context = self.controller.editingContextGeneration
                func current() throws {
                    guard self.active, self.prepared, self.isPage(web.url), self.generation == generation,
                          self.controller.owner == self.epoch, frame.epoch == self.epoch,
                          self.controller.editingContextActive, self.controller.editingContextGeneration == context,
                          self.controller.connected, self.sent == frame.sequence,
                          self.controller.acceptedSequence == frame.sequence,
                          self.controller.acknowledgedFrame?.epoch == frame.epoch,
                          self.controller.acknowledgedFrame?.selection == input.selection,
                          self.controller.acknowledgedFrame?.draft == input.draft,
                          self.controller.acknowledgedFrame?.view == input.view,
                          self.controller.acknowledgedFrame?.cameraSequence == input.cameraSequence,
                          self.controller.acknowledgedFrame?.transparent == input.transparent,
                          self.controller.acknowledgedFrame?.presentation == input.presentation,
                          self.latest?.sequence == frame.sequence,
                          self.parent.draft == input.draft, self.parent.selection == input.selection,
                          self.parent.view == input.view, self.parent.cameraSequence == input.cameraSequence,
                          (self.controller.planSupported ? self.parent.presentation : .threeD) == input.presentation,
                          self.parent.credentials.tenantId == frame.tenantId, self.parent.credentials.principalId == frame.principalId,
                          self.installedToken == self.parent.credentials.token, self.parent.disabled == frame.disabled,
                          self.parent.transparent == input.transparent, self.entranceOperation == nil,
                          self.placementIntent == nil, !self.inspectorPending else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                try current()
                // One synchronous, read-only sample. These existing datasets
                // report a last request/map admission, never live pose or appearance.
                let script = """
                const canvases = [...document.querySelectorAll('canvas[aria-label="전시 공간 3D 미리보기"]')].filter(c => {
                  const r = c.getBoundingClientRect(); return c.isConnected && r.width > 0 && r.height > 0;
                });
                if (canvases.length !== 1) throw new Error('VIEWPORT_DISPLAY_CANVAS_UNAVAILABLE');
                const c = canvases[0], result = {};
                for (const key of ['floorView', 'cameraProjection', 'floorTextures']) {
                  const value = c.dataset[key] ?? null;
                  if (value !== null && value.length > 4096) throw new Error('VIEWPORT_DISPLAY_LIMIT');
                  result[key] = value;
                }
                return result;
                """
                let response = try await web.callAsyncJavaScript(script, arguments: [:], in: nil, contentWorld: .page)
                try Task.checkCancellation(); try current()
                guard let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 16_384, case .object(let fields) = try JSONDecoder().decode(DocumentValue.self, from: bytes),
                      Set(fields.keys) == Set(["floorView", "cameraProjection", "floorTextures"]) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                var datasets: [String: DocumentValue] = [:]
                for (key, value) in fields {
                    if value == .null { datasets[key] = .null; continue }
                    guard let string = value.string, string.utf8.count <= 4096 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    let parsed = try JSONDecoder().decode(DocumentValue.self, from: Data(string.utf8))
                    guard case .object = parsed else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    datasets[key] = parsed
                }
                let selectedFloor = input.selection.flatMap { selection in
                    selection.kind == "surface" && input.draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == selection.id && $0["type"].string == "floor" }) == true ? selection.id : nil
                }
                let lastFloor = datasets["floorView"]?["surfaceId"].string
                let match = selectedFloor != nil && lastFloor?.lowercased() == selectedFloor?.lowercased()
                let selection: DocumentValue = frame.selection.map { .object(["kind": .string($0.kind), "id": .string($0.id)]) } ?? .null
                let ack: DocumentValue = .object(["epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                    "selection": selection, "view": .string(frame.view), "cameraSequence": .integer(Int64(frame.cameraSequence)),
                    "transparent": .bool(frame.transparent), "presentation": .string(frame.presentation.rawValue)])
                let value: DocumentValue = .object(["ack": ack, "canvas": .object(datasets),
                    "lastFloorRequestMatchesSelection": .bool(match)])
                let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
                return String(decoding: try encoder.encode(value), as: UTF8.self)
            }
            controller.entrancePreviewAction = { [weak self] input in
                guard let self, self.active, self.prepared, self.controller.owner == self.epoch,
                      self.controller.entrancePreviewSupported, self.placementIntent == nil, !self.inspectorPending,
                      self.entranceOperation == nil || input.action == .close,
                      let web = self.web, let frame = self.latest, self.sent == frame.sequence,
                      frame.draft == input.draft, frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let operation = UUID(), generation = self.generation
                self.entranceOperation = operation
                defer { if self.entranceOperation == operation { self.entranceOperation = nil } }
                let request = try ViewportEntrancePreviewRequest(frame: frame, action: input.action, device: input.device, scenario: input.scenario,
                                                               reducedMotion: input.reducedMotion, theme: input.theme, locale: input.locale)
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                var acceptedViewport: CGSize?
                var acceptedBackingScale: CGFloat?
                @MainActor func current() throws {
                    try Task.checkCancellation()
                    guard self.active, self.prepared, self.controller.connected, self.controller.owner == self.epoch,
                          self.controller.entrancePreviewSupported, self.generation == generation,
                          self.entranceOperation == operation, self.latest?.sequence == frame.sequence,
                          self.latest?.draft == input.draft, web.pageZoom == 1 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    if let acceptedViewport, web.bounds.size != acceptedViewport { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    if let acceptedBackingScale, web.window?.backingScaleFactor != acceptedBackingScale { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                func decode(_ value: Any?) throws -> DocumentValue {
                    guard let value, JSONSerialization.isValidJSONObject(value) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    let data = try JSONSerialization.data(withJSONObject: value)
                    guard data.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                    return try JSONDecoder().decode(DocumentValue.self, from: data)
                }
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.entrancePreview !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.entrancePreview(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try current()
                let reply = try request.checkedReply(decode(response))
                guard let width = reply.value["viewport"]["width"].double, let height = reply.value["viewport"]["height"].double,
                      abs(width - Double(web.bounds.width)) <= 0.5, abs(height - Double(web.bounds.height)) <= 0.5 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                acceptedViewport = web.bounds.size
                guard input.action == .thumbnail else { return NativeEntrancePreviewOutput(request: request, png: nil) }
                // A fallback layout is not proof of an actual space image.
                if input.theme == .threshold, reply.value["imageStatus"].string != "ready" { return NativeEntrancePreviewOutput(request: request, png: nil) }
                guard reply.snapshotRect.height / reply.snapshotRect.width <= 4 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                let scale = web.window?.backingScaleFactor ?? 1
                guard scale.isFinite, (1...4).contains(scale) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                acceptedBackingScale = web.window?.backingScaleFactor
                let config = WKSnapshotConfiguration(); config.rect = reply.snapshotRect
                config.snapshotWidth = NSNumber(value: 256 / Double(scale)); config.afterScreenUpdates = true
                let snapshot: NSImage? = try await web.takeSnapshot(configuration: config)
                try current()
                let state = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.entrancePreviewState !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.entrancePreviewState(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try current(); try reply.recheck(decode(state), request: request)
                guard let image = snapshot, let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff), bitmap.pixelsWide <= 258,
                      bitmap.pixelsHigh <= 1032, let png = bitmap.representation(using: .png, properties: [:]), png.count <= 1_048_576 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                return NativeEntrancePreviewOutput(request: request, png: png)
            }
            controller.furnishingVisualsAction = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.furnishingVisualsSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try FurnishingViewportRequest(frame: frame, command: command)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.furnishingVisuals !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.furnishingVisuals(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.furnishingVisualsSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.spaceSettingsAction = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.spaceSettingsSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try SpaceLightingViewportRequest(frame: frame, command: command)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.spaceSettings !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.spaceSettings(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.spaceSettingsSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.lightInspectorAction = { [weak self] command, selectedLightId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.lightInspectorSupported,
                      command.fixture == nil || self.controller.lightFixtureSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try LightViewportRequest(frame: frame, command: command, selectedLightId: selectedLightId)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let script = command.fixture == nil
                    ? "if (typeof window.exhibitosViewport?.lightInspector !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.lightInspector(JSON.parse(requestJSON));"
                    : "if (typeof window.exhibitosViewport?.lightDetails !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.lightDetails(JSON.parse(requestJSON));"
                let response = try await web.callAsyncJavaScript(script, arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.lightInspectorSupported, self.controller.connected,
                      command.fixture == nil || self.controller.lightFixtureSupported,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes), selectedLightId: selectedLightId)
            }
            controller.benchInspectorAction = { [weak self] command, selectedBenchId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.benchInspectorSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try BenchViewportRequest(frame: frame, command: command, selectedBenchId: selectedBenchId)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.benchInspector !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.benchInspector(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.benchInspectorSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes), selectedBenchId: selectedBenchId)
            }
            controller.plaqueInspectorAction = { [weak self] command, selectedPlaqueId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.plaqueInspectorSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try PlaqueViewportRequest(frame: frame, command: command, selectedPlaqueId: selectedPlaqueId)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.plaqueInspector !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.plaqueInspector(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.plaqueInspectorSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes), selectedPlaqueId: selectedPlaqueId)
            }
            controller.placementAction = { [weak self] intent, phase, target in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.placementSupported, !self.inspectorPending,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, frame.draft == intent.base else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try ViewportPlacementRequest(frame: frame, intent: intent, phase: phase, target: target)
                if phase == .begin {
                    guard self.placementIntent == nil else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                    self.placementIntent = intent
                } else {
                    guard self.placementIntent?.requestId == intent.requestId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                let generation = self.generation
                if phase == .cancel { self.placementIntent = nil }
                do {
                    let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                    let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.placement !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.placement(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                    try Task.checkCancellation()
                    guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence,
                          phase == .cancel || self.placementIntent?.requestId == intent.requestId,
                          let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    let bytes = try JSONSerialization.data(withJSONObject: response)
                    guard bytes.count <= 16384 else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
                    let placement = try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
                    if phase == .commit { self.placementIntent = nil }
                    return placement
                } catch {
                    if phase != .cancel, self.placementIntent?.requestId == intent.requestId {
                        // An invalid/lost acknowledgement must not leave a ghost
                        // after native refuses the operation. A changed frame
                        // already invalidates its own web operation.
                        if self.generation == generation, self.latest?.sequence == frame.sequence,
                           let cancel = try? ViewportPlacementRequest(frame: frame, intent: intent, phase: .cancel),
                           let bytes = try? JSONEncoder().encode(cancel.value) {
                            _ = try? await web.callAsyncJavaScript("return await window.exhibitosViewport?.placement(JSON.parse(requestJSON));", arguments: ["requestJSON": String(decoding: bytes, as: UTF8.self)], in: nil, contentWorld: .page)
                        }
                        self.endPlacement()
                    }
                    throw error
                }
            }
            controller.furnishingThumbnailAction = { [weak self] in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.furnishingVisualsSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, !self.parent.interactionDisabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let request = try FurnishingThumbnailRequest(frame: frame)
                let value = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.furnishingThumbnail !== 'function') throw new Error('CATALOG_PREVIEW_UNAVAILABLE'); return await window.exhibitosViewport.furnishingThumbnail(JSON.parse(requestJSON));", arguments: ["requestJSON": value], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.connected, self.controller.furnishingVisualsSupported, self.placementIntent == nil, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, !self.parent.interactionDisabled,
                      let fields = response as? [String: Any], let png = fields["pngBase64"] as? String, png.utf8.count <= 699_052,
                      JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: fields)
                guard bytes.count <= 710_000 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                return try request.pngData(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.thumbnailAction = { [weak self] artworkId, revision in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.thumbnailSupported,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, !self.parent.interactionDisabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                let generation = self.generation
                let request = try ViewportThumbnailRequest(frame: frame, artworkId: artworkId, expectedRevision: revision)
                let value = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.thumbnail !== 'function') throw new Error('CATALOG_PREVIEW_UNAVAILABLE'); return await window.exhibitosViewport.thumbnail(JSON.parse(requestJSON));", arguments: ["requestJSON": value], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, !self.parent.interactionDisabled,
                      let fields = response as? [String: Any], let png = fields["pngBase64"] as? String, png.utf8.count <= 699_052,
                      JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: fields)
                guard bytes.count <= 710_000 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                return try JSONDecoder().decode(ViewportThumbnailReply.self, from: bytes).pngData(matching: request)
            }
            controller.findSelectionAction = { [weak self] in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.findSelectionSupported,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, !self.parent.interactionDisabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                let generation = self.generation, request = try ViewportFindSelectionRequest(frame: frame)
                let value = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.findSelection !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.findSelection(JSON.parse(requestJSON));", arguments: ["requestJSON": value], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, !self.parent.interactionDisabled,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 2048, try JSONDecoder().decode(ViewportFindSelectionReply.self, from: bytes).matches(request) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            }
            controller.action = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, !self.inspectorPending, self.placementIntent == nil, let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                if case .surfaceMaterialTexture = command {
                    guard self.controller.surfaceMaterialTexturesSupported else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                }
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
            case "placement-target":
                guard controller.editingContextActive, editingContextGeneration == controller.editingContextGeneration, !frame.disabled, let intent = placementIntent,
                      (try? ViewportPlacementTarget(value: envelope.body, intent: intent)) != nil else { return }
                parent.onPlacementTarget(envelope.body)
            case "placement-cancel":
                guard let intent = placementIntent, case .object(let body) = envelope.body,
                      Set(body.keys) == Set(["placementRequestId", "reason"]), envelope.body["placementRequestId"].string == intent.requestId,
                      let reason = envelope.body["reason"].string, reason.utf8.count <= 160 else { return }
                endPlacement()
            case "selection":
                guard controller.editingContextActive, editingContextGeneration == controller.editingContextGeneration, placementIntent == nil, !frame.disabled, !parent.interactionDisabled, let selection = try? JSONDecoder().decode(ViewportSelection.self, from: JSONEncoder().encode(envelope.body)), selection.belongs(to: frame.draft) else { return }
                parent.onSelect(selection)
            case "candidate": guard controller.editingContextActive, editingContextGeneration == controller.editingContextGeneration, placementIntent == nil, !frame.disabled, !parent.interactionDisabled else { return }; parent.onCandidate(envelope.body, frame.draft)
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
    var workbench = false
    var mode: Mode = .general
    @State private var identity = UUID()
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let credentials = model.viewportCredentials, let draft = model.viewportDraft, ready {
                if workbench { workbenchViewTools } else {
                Picker("편집 화면", selection: Binding(get: { controller.planSupported ? model.viewportPresentation : .threeD }, set: { value in
                    if value == .threeD || controller.planSupported { model.viewportPresentation = value }
                })) {
                    Text("3D 공간").tag(ViewportPresentation.threeD)
                    Text("2D 평면").tag(ViewportPresentation.plan).disabled(!controller.planSupported)
                }.pickerStyle(.segmented)
                    .disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
                    .accessibilityHint("같은 전시와 선택을 유지하며 편집 화면을 전환합니다.")
                if !controller.planSupported { Text("이 제작 서비스는 3D 화면을 제공합니다. 2D 평면 지원 연결을 확인하세요.").foregroundStyle(.secondary) }
                HStack {
                    Button("전체 공간") { model.requestViewportView("isometric") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    Button("위에서 보기") { model.requestViewportView("top") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    Button("정면 보기") { model.requestViewportView("front") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    if editable {
                        Picker("편집 조작", selection: $model.viewportMode) { Text("이동").tag("translate"); Text("회전").tag("rotate"); Text("크기").tag("dimensions") }
                    }
                }.disabled(model.placingArtwork || model.entrancePreviewActive)
                Button("편집 화면 다시 열기") {
                    if model.entrancePreviewActive { model.recoverEntrancePreviewHost { identity = UUID() } }
                    else { identity = UUID() }
                }.disabled(model.entrancePreviewActive ? !model.canRecoverEntranceHost : (model.placingArtwork || model.busy || model.pending || model.architectureApplying))
                    .help("이 편집 화면만 다시 만듭니다. 입력·선택·이력과 마지막으로 선택한 보기를 유지합니다.")
                Toggle("공간을 투명하게 보기", isOn: $model.viewportTransparent)
                    .disabled(model.placingArtwork || model.entrancePreviewActive)
                    .help("벽·바닥 너머 작품을 확인합니다. 저장한 재질은 바뀌지 않습니다.")
                }
                viewportSurface(credentials: credentials, draft: draft)
                    .frame(height: workbench ? nil : (model.entrancePreviewActive ? 740 : 440))
                    .frame(minHeight: workbench ? 120 : nil, maxHeight: workbench ? .infinity : (model.entrancePreviewActive ? 740 : 440))
                if !workbench { Text("불러온 작품 \(controller.loaded)개 · 불러오지 못한 작품 \(controller.failed)개").foregroundStyle(.secondary) }
                Text(workbench ? "작품 \(controller.loaded)개 · 불러오기 실패 \(controller.failed)개 · " + controller.message : controller.message).foregroundStyle(.secondary)
                    .font(workbench ? .caption : .body).lineLimit(workbench ? 2 : nil).padding(.horizontal, workbench ? 12 : 0)
                    .onChange(of: controller.acceptedSequence) { _, _ in model.confirmEntranceHostRecovery() }
                    .onChange(of: controller.connected) { _, _ in model.confirmEntranceHostRecovery() }
                if mode == .expert || !controller.diagnostic.isEmpty {
                    DisclosureGroup("3D 연결 상세") {
                        if !controller.diagnostic.isEmpty { Text(controller.diagnostic).textSelection(.enabled) }
                        if mode == .expert {
                            Button(controller.readingDisplay ? "표시 상태 확인 중…" : "표시 상태 확인") {
                                let input = NativeDisplayReadoutInput(draft: draft, selection: model.viewportSelection,
                                    view: model.viewportView, cameraSequence: model.viewportCameraSequence,
                                    transparent: model.viewportTransparent,
                                    presentation: controller.planSupported ? model.viewportPresentation : .threeD)
                                Task { await controller.readDisplayStatus(input) }
                            }.disabled(!controller.connected || !controller.editingContextActive || controller.readingDisplay || model.entrancePreviewActive)
                            Text("요청·맵 상태 정보입니다. 현재 카메라 좌표나 외관 수락을 뜻하지 않습니다.").font(.caption).foregroundStyle(.secondary)
                            ScrollView {
                                Text(controller.displayReadout.isEmpty ? "현재 표시 상태를 확인하세요." : controller.displayReadout)
                                    .font(.caption.monospaced()).textSelection(.enabled)
                            }.frame(height: 120)
                        }
                    }
                }
                if !workbench {
                if editable {
                    HStack {
                        Button("벽과 바닥 만들기") { Task { await controller.perform(.whiteCube(roomId: model.roomId), model: model) } }
                        Button("공간 추가") { Task { await controller.perform(.addRoom(name: "새 공간"), model: model) } }
                        Button("조명 추가") { Task { await controller.perform(.addLight(roomId: model.roomId), model: model) } }
                    }.disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
                    Text("숫자 입력은 이 Mac에 보관한 뒤 3D에 반영됩니다. 3D 변경도 이 Mac에 보관하며, 전시 저장과 공개는 별도로 진행합니다.").foregroundStyle(.secondary)
                }
                NativeLightPicker(model: model, ready: ready).disabled(model.entrancePreviewActive)
                NativeBenchPicker(model: model, ready: ready).disabled(model.entrancePreviewActive)
                NativePlaquePicker(model: model, ready: ready).disabled(model.entrancePreviewActive)
                }
            } else { Text("전시를 열고 로컬 제작을 시작하면 3D를 볼 수 있습니다.").foregroundStyle(.secondary) }
        }
    }
    private func viewportSurface(credentials: LocalAuthoringSession, draft: NativeExhibitionDraft) -> some View {
        GeometryReader { geometry in
            let width = model.entrancePreviewActive ? (model.entrancePreviewDevice == .mobile ? 360.0 : 1024.0) : max(1, geometry.size.width)
            let height = model.entrancePreviewActive ? 740.0 : max(1, workbench ? geometry.size.height : 440)
            let fit = workbench && model.entrancePreviewActive ? min(1, geometry.size.width / width, geometry.size.height / height) : 1
            if workbench {
                ZStack {
                    renderer(credentials: credentials, draft: draft)
                        .frame(width: width, height: height)
                        .scaleEffect(fit, anchor: .topLeading)
                        .frame(width: width * fit, height: height * fit, alignment: .topLeading)
                }.frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            } else {
                ScrollView(.horizontal) { renderer(credentials: credentials, draft: draft).frame(width: width, height: height) }
            }
        }
    }
    private func rendererIdentity(_ credentials: LocalAuthoringSession) -> String {
        // The view and its identity must use the same credential snapshot. The
        // model access UUID fences other tasks but is published separately.
        // Only a one-way digest enters SwiftUI identity; never the bearer token.
        let fields = [credentials.origin, credentials.tenantId.lowercased(), credentials.principalId.lowercased(), credentials.token]
        let digest = SHA256.hash(data: Data(fields.joined(separator: "\0").utf8))
        return identity.uuidString + digest.map { String(format: "%02x", $0) }.joined()
    }
    private func renderer(credentials: LocalAuthoringSession, draft: NativeExhibitionDraft) -> some View {
        NativeViewport(credentials: credentials, draft: draft, selection: model.viewportSelection, presentation: model.viewportPresentation, mode: model.viewportMode, view: model.viewportView, cameraSequence: model.viewportCameraSequence, transparent: model.viewportTransparent, disabled: !editable || model.entrancePreviewActive || model.busy || model.pending || model.formDirty || model.positionDirty || model.surfaceMaterialPreviewing || model.lightPreviewing || model.benchPreviewing || model.plaquePreviewing || model.furnishingPreviewing || model.spaceLightingPreviewing, interactionDisabled: !editable || model.entrancePreviewActive || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.surfaceMaterialPreviewing || model.lightPreviewing || model.benchPreviewing || model.plaquePreviewing || model.furnishingPreviewing || model.spaceLightingPreviewing || model.placingArtwork, controller: controller, onSelect: { if controller.editingContextActive { model.selectViewport($0) } }, onCandidate: { candidate, base in let context = controller.editingContextGeneration; Task { guard controller.editingContextActive, controller.editingContextGeneration == context else { return }; await model.acceptViewportCandidate(candidate, base: base) } }, onPlacementTarget: { model.receivePlacementTarget($0) }, onPlacementCancel: { model.placementCancelled($0) })
            .id(rendererIdentity(credentials))
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .allowsHitTesting(!model.architectureDirty && !model.architectureApplying && !model.placementApplying)
            .disabled(model.architectureDirty || model.architectureApplying)
    }
    private var workbenchViewTools: some View {
        HStack(spacing: 8) {
            Picker("편집 화면", selection: Binding(get: { controller.planSupported ? model.viewportPresentation : .threeD }, set: { value in
                if value == .threeD || controller.planSupported { model.viewportPresentation = value }
            })) {
                Text("3D 공간").tag(ViewportPresentation.threeD)
                Text("2D 평면").tag(ViewportPresentation.plan).disabled(!controller.planSupported)
            }.pickerStyle(.segmented).frame(maxWidth: 220)
                .disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
                .accessibilityHint("같은 전시와 선택을 유지하며 편집 화면을 전환합니다.")
            if editable {
                Picker("편집 조작", selection: $model.viewportMode) { Text("이동").tag("translate"); Text("회전").tag("rotate"); Text("크기").tag("dimensions") }
                    .labelsHidden().fixedSize().disabled(model.placingArtwork || model.entrancePreviewActive)
            }
            Spacer(minLength: 0)
            Menu {
                Button("전체 공간") { model.requestViewportView("isometric") }.disabled(model.placingArtwork || model.entrancePreviewActive || (controller.planSupported && model.viewportPresentation == .plan))
                Button("위에서 보기") { model.requestViewportView("top") }.disabled(model.placingArtwork || model.entrancePreviewActive || (controller.planSupported && model.viewportPresentation == .plan))
                Button("정면 보기") { model.requestViewportView("front") }.disabled(model.placingArtwork || model.entrancePreviewActive || (controller.planSupported && model.viewportPresentation == .plan))
                Toggle("공간을 투명하게 보기", isOn: $model.viewportTransparent).disabled(model.placingArtwork || model.entrancePreviewActive)
                Divider()
                Button("편집 화면 다시 열기") {
                    if model.entrancePreviewActive { model.recoverEntrancePreviewHost { identity = UUID() } }
                    else { identity = UUID() }
                }.disabled(model.entrancePreviewActive ? !model.canRecoverEntranceHost : (model.placingArtwork || model.busy || model.pending || model.architectureApplying))
            } label: { Label("보기", systemImage: "eye") }
                .menuStyle(.borderlessButton).fixedSize()
            if editable {
                Menu {
                    Button("벽과 바닥 만들기") { Task { await controller.perform(.whiteCube(roomId: model.roomId), model: model) } }
                    Button("공간 추가") { Task { await controller.perform(.addRoom(name: "새 공간"), model: model) } }
                    Button("조명 추가") { Task { await controller.perform(.addLight(roomId: model.roomId), model: model) } }
                    Button("벤치 추가") { model.beginBench() }.disabled(!model.canAddBench)
                    Button("명판 추가") { model.beginPlaque() }.disabled(!model.canAddPlaque)
                } label: { Label("추가", systemImage: "plus") }
                    .menuStyle(.borderlessButton).fixedSize()
                    .disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
            }
        }.controlSize(.small).padding(.horizontal, 12).padding(.top, 8)
    }

}
