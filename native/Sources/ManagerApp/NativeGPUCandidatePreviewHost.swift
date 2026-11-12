// SPDX-License-Identifier: Apache-2.0
import AppKit
import Foundation
import ManagerCore
import SwiftUI
import WebKit
#if DEBUG
import OSLog
#endif

/// Owns a visible WK operation until the matching renderer confirms drain.
/// An interrupted or failed cleanup never becomes a drain receipt.
@MainActor
final class NativeGPUCandidatePreviewController: NSObject, ObservableObject, Identifiable, WKNavigationDelegate, WKScriptMessageHandler {
    let id = UUID()
    @Published private(set) var presented = false
    @Published private(set) var drained = false
    @Published private(set) var approvalAvailable = false
    @Published private(set) var message = "감상용 파일을 준비하고 있습니다."
    private let credentials: LocalAuthoringSession
    private let assets: Set<String>
    private let checkCurrent: @MainActor () async throws -> Void
    private let transfer: NativeGPUModelPreparationTransfer
    private let session: AuthoringSession
    private let manifest: String
    private var approvalAuthority: NativeGPUPreviewApprovalAuthority?
    private var lifecycle: NativeGPUCandidatePreviewLifecycle
    private var web: WKWebView?
    private var page: URL?
    private var beginIssued = false
    private var cancelled = false
    private var cancelInFlight = false
    private var task: Task<Void, Never>?

    #if DEBUG
    private var decodeObservations = NativeGPUDecodeObservationLimiter()
    private enum StartupStage: String {
        case setup, ruleCompilation, cookie, pageMethodWait, capability, visibility
        case currentBefore, beginCall, readyShape, currentAfter, approvalAuthority
    }
    #endif

    init(credentials: LocalAuthoringSession, assets: Set<String>, request: NativeGPUCandidatePreviewRequest,
         transfer: NativeGPUModelPreparationTransfer, session: AuthoringSession, manifest: String,
         checkCurrent: @escaping @MainActor () async throws -> Void) {
        self.credentials = credentials; self.assets = assets; lifecycle = .init(request: request); self.checkCurrent = checkCurrent
        self.transfer = transfer; self.session = session; self.manifest = manifest
    }

    func attach(_ web: WKWebView) {
        guard self.web == nil, !cancelled else { return }
        self.web = web; web.navigationDelegate = self
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(applicationHidden(_:)), name: NSApplication.didHideNotification, object: nil)
        center.addObserver(self, selector: #selector(windowUnavailable(_:)), name: NSWindow.didMiniaturizeNotification, object: nil)
        center.addObserver(self, selector: #selector(windowUnavailable(_:)), name: NSWindow.willCloseNotification, object: nil)
        task = Task { @MainActor [weak self] in await self?.start() }
    }

    @objc private func applicationHidden(_ notification: Notification) { requestClose() }
    @objc private func windowUnavailable(_ notification: Notification) {
        guard let window = notification.object as? NSWindow, window === web?.window else { return }
        requestClose()
    }

    private func invalidate(_ text: String) {
        approvalAuthority?.revoke(); approvalAvailable = false
        lifecycle.invalidate(); presented = false; message = text
    }

    private func start() async {
        guard let web else { return }
        #if DEBUG
        Logger(subsystem: "org.exhibitos.manager.validation", category: "GPUCandidatePreviewStartup")
            .notice("NativeGPUPreviewStartupObservationReady")
        var stage = StartupStage.setup
        var readyAccepted: Bool?
        var readyKeys = "unavailable"
        var lastMethodAvailable: Bool?
        #endif
        do {
            let req = lifecycle.request.value
            guard let page = URL(string: credentials.origin + "/native-viewport.html"),
                  credentials.tenantId == req["tenantId"].string, credentials.principalId == req["principalId"].string,
                  credentials.token.utf8.count == 43,
                  credentials.token.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95 }) else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
            self.page = page
            let rules = try NativeGPUCandidatePreviewPolicy.encodedRules(origin: credentials.origin, request: lifecycle.request, verifiedAssets: assets)
            let name = "ExhibitOSGPUPreview-" + UUID().uuidString
            #if DEBUG
            stage = .ruleCompilation
            #endif
            let rule = try await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: name, encodedContentRuleList: rules)
            try? await WKContentRuleListStore.default().removeContentRuleList(forIdentifier: name)
            guard !cancelled, let rule else { throw AuthoringFailure("GPU_MODEL_PREPARATION_UNAVAILABLE") }
            web.configuration.userContentController.add(rule)
            #if DEBUG
            stage = .cookie
            #endif
            let headers = ["Set-Cookie": "exhibitos_local_session=\(credentials.token); Path=/; HttpOnly; SameSite=Strict"]
            guard let cookie = HTTPCookie.cookies(withResponseHeaderFields: headers, for: page).first, cookie.isHTTPOnly else { throw AuthoringFailure("LOCAL_SESSION_INVALID") }
            await web.configuration.websiteDataStore.httpCookieStore.setCookie(cookie)
            guard !cancelled else { return }
            web.load(URLRequest(url: page, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 15))
            #if DEBUG
            stage = .pageMethodWait
            #endif
            let deadline = ContinuousClock.now.advanced(by: .seconds(30))
            while true {
                try Task.checkCancellation()
                guard !cancelled, ContinuousClock.now < deadline else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                if !web.isLoading, web.url == page {
                    let methodAvailable = (try? await web.callAsyncJavaScript("return typeof window.exhibitosViewport?.gpuCandidatePreview === 'function';", arguments: [:], in: nil, contentWorld: .page)) as? Bool
                    #if DEBUG
                    lastMethodAvailable = methodAvailable
                    #endif
                    if methodAvailable == true { break }
                }
                try await Task.sleep(for: .milliseconds(50))
            }
            #if DEBUG
            stage = .capability
            #endif
            let cap = try await web.callAsyncJavaScript("return await window.exhibitosViewport.gpuCandidatePreview({version:1,phase:'capability'});", arguments: [:], in: nil, contentWorld: .page)
            let capability = try Self.value(cap)
            guard capability == .object(["version": .integer(1), "capability": .string(NativeGPUCandidatePreviewRequest.capability), "profile": .string(NativeGPUModelPreparation.profile)]),
                  !cancelled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
            #if DEBUG
            stage = .visibility
            #endif
            while web.window?.isVisible != true || web.isHiddenOrHasHiddenAncestor || web.bounds.width <= 0 || web.bounds.height <= 0 {
                try Task.checkCancellation()
                guard !cancelled, ContinuousClock.now < deadline else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                try await Task.sleep(for: .milliseconds(50))
            }
            #if DEBUG
            stage = .currentBefore
            #endif
            try await checkCurrent()
            guard !cancelled else { return }
            let json = String(decoding: try JSONEncoder().encode(lifecycle.request.value), as: UTF8.self)
            beginIssued = true
            #if DEBUG
            stage = .beginCall
            #endif
            let response = try await web.callAsyncJavaScript("return await window.exhibitosViewport.gpuCandidatePreview(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
            #if DEBUG
            stage = .readyShape
            if let fields = response as? [String: Any] {
                let allowed: Set<String> = ["version", "epoch", "sequence", "requestId", "workspaceId", "tenantId", "principalId", "artworkId", "artworkRevision", "jobId", "generation", "receiptSHA256", "source", "candidate", "capability", "reviewRevision", "rendered", "drawGeneration", "targetFormat"]
                let keys = Set(fields.keys)
                readyKeys = keys.intersection(allowed).sorted().joined(separator: ",") + (keys.subtracting(allowed).isEmpty ? "" : ",other")
            }
            #endif
            let ready = try Self.value(response)
            let accepted = !cancelled && lifecycle.request.acceptsReady(ready)
            #if DEBUG
            readyAccepted = accepted
            #endif
            guard accepted, web.window?.isVisible == true, !web.isHiddenOrHasHiddenAncestor else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            #if DEBUG
            stage = .currentAfter
            #endif
            try await checkCurrent()
            guard !cancelled, web.window?.isVisible == true, !web.isHiddenOrHasHiddenAncestor, lifecycle.acceptReady(ready) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            #if DEBUG
            stage = .approvalAuthority
            #endif
            approvalAuthority = try NativeGPUPreviewApprovalAuthority(lifecycle: lifecycle, transfer: transfer, manifest: manifest, session: session)
            presented = true; approvalAvailable = lifecycle.request.reviewRevision == Int64(transfer.request.revision)
            message = "감상용 파일을 표시했습니다."
        } catch {
            if cancelled { return }
            #if DEBUG
            var failure: String
            if let error = error as? AuthoringFailure {
                let bytes = error.code.utf8
                let code = (1...80).contains(bytes.count) && bytes.allSatisfy {
                    (65...90).contains($0) || (48...57).contains($0) || $0 == 95
                } ? error.code : "other"
                failure = "authoring:\(code)"
            } else {
                let error = error as NSError
                let domain = ["WKErrorDomain", "NSURLErrorDomain", "NSCocoaErrorDomain"].contains(error.domain) ? error.domain : "other"
                failure = "ns:\(domain):\(error.code)"
                if domain == "WKErrorDomain" {
                    let exception: String
                    if let message = error.userInfo["WKJavaScriptExceptionMessage"] as? String {
                        let code = message.hasPrefix("Error: ") ? String(message.dropFirst(7)) : message
                        let bytes = code.utf8
                        exception = (1...80).contains(bytes.count) && bytes.allSatisfy {
                            (65...90).contains($0) || (48...57).contains($0) || $0 == 95
                        } ? code : "other"
                    } else { exception = "absent" }
                    failure += ":exception=\(exception)"
                }
            }
            let shape = stage == .readyShape ? " accept=\(readyAccepted.map { $0 ? "true" : "false" } ?? "unknown") keys=\(readyKeys)" : ""
            let method = stage == .pageMethodWait ? " methodAvailable=\(lastMethodAvailable.map { $0 ? "true" : "false" } ?? "unknown")" : ""
            let detail = "stage=\(stage.rawValue) error=\(failure) beginIssued=\(beginIssued) isLoading=\(web.isLoading) pageMatches=\(page != nil && web.url == page) visible=\(web.window?.isVisible == true && !web.isHiddenOrHasHiddenAncestor)\(method)\(shape)"
            Logger(subsystem: "org.exhibitos.manager.validation", category: "GPUCandidatePreviewStartup")
                .error("NativeGPUPreviewStartupFailure \(detail, privacy: .public)")
            #endif
            invalidate("감상용 파일을 표시하지 못했습니다. 작업 상태를 확인하세요.")
            if beginIssued { await cancelAndDrain() } else { finishWithoutBegin() }
        }
    }

    func claimApproval() -> NativeGPUPreviewApprovalAuthority? {
        guard approvalAvailable, presented, !cancelled, !drained, lifecycle.rendered,
              let web, web.window?.isVisible == true, !web.isHiddenOrHasHiddenAncestor,
              let authority = approvalAuthority, authority.claim() else { return nil }
        approvalAvailable = false; message = "승인 결과를 확인하고 있습니다."
        return authority
    }

    func approvalFinished(verified: Bool, uncertain: Bool) {
        approvalAvailable = false
        message = verified ? "감상용 파일을 승인했습니다." : uncertain
            ? "승인 결과를 확인하지 못했습니다. 작업을 보존하고 상태를 다시 확인하세요."
            : "승인하지 못했습니다. 미리보기를 다시 열어 확인하세요."
    }

    func requestClose() {
        guard !drained, !cancelInFlight else { return }
        cancelled = true; invalidate("미리보기를 종료하고 있습니다.")
        task?.cancel()
        Task { @MainActor [weak self] in
            guard let self else { return }
            if self.beginIssued { await self.cancelAndDrain() } else { self.finishWithoutBegin() }
        }
    }

    private func finishWithoutBegin() {
        guard !beginIssued else { return }
        finishDrained()
    }

    private func finishDrained() {
        presented = false; drained = true
        NotificationCenter.default.removeObserver(self)
        web?.stopLoading(); web?.navigationDelegate = nil
        web?.configuration.userContentController.removeScriptMessageHandler(forName: "exhibitosViewport")
        #if DEBUG
        web?.configuration.userContentController.removeScriptMessageHandler(forName: "exhibitosGPUDecodeObservation")
        #endif
        web?.configuration.userContentController.removeAllContentRuleLists()
        web = nil; page = nil; message = "미리보기가 종료됐습니다."
    }

    private func cancelAndDrain() async {
        guard !drained, !cancelInFlight else { return }
        cancelInFlight = true
        defer { cancelInFlight = false }
        cancelled = true; invalidate("미리보기를 종료하고 있습니다.")
        guard let web else { message = "종료 확인이 필요합니다."; return }
        do {
            let json = String(decoding: try JSONEncoder().encode(lifecycle.request.cancel), as: UTF8.self)
            let reply = try await web.callAsyncJavaScript("return await window.exhibitosViewport.gpuCandidatePreview(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
            guard lifecycle.acceptCancelled(try Self.value(reply)) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            finishDrained()
        } catch { if !drained { message = "종료를 확인하지 못했습니다. 작업을 보존하고 다시 확인하세요." } }
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        #if DEBUG
        if message.name == "exhibitosGPUDecodeObservation" {
            guard let web, let page, message.webView === web, message.frameInfo.isMainFrame,
                  message.frameInfo.request.url == page, web.url == page,
                  message.frameInfo.securityOrigin.protocol == page.scheme,
                  message.frameInfo.securityOrigin.host == page.host,
                  message.frameInfo.securityOrigin.port == (page.port ?? (page.scheme == "https" ? 443 : 80)),
                  let observation = decodeObservations.accept(message.body) else { return }
            let visible = web.window?.isVisible == true && !web.isHiddenOrHasHiddenAncestor
            Logger(subsystem: "org.exhibitos.manager.validation", category: "GPUCandidatePreviewStartup")
                .notice("GPUDecodeObservation controller=\(self.id.uuidString, privacy: .public) receiptPageMatches=\(web.url == page, privacy: .public) receiptIsLoading=\(web.isLoading, privacy: .public) receiptWindowVisible=\(visible, privacy: .public) receiptBeginIssued=\(self.beginIssued, privacy: .public) receiptCancelled=\(self.cancelled, privacy: .public) receiptDrained=\(self.drained, privacy: .public) event=\(observation, privacy: .public)")
            return
        }
        #endif
        guard message.name == "exhibitosViewport", message.frameInfo.isMainFrame, message.webView === web,
              let value = try? Self.value(message.body),
              lifecycle.acceptEnded(value) else { return }
        approvalAuthority?.revoke(); approvalAvailable = false; presented = false
        if lifecycle.drained { finishDrained() }
        else { self.message = "미리보기 종료를 확인하고 있습니다." }
    }
    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
        navigationAction.targetFrame?.isMainFrame == true && navigationAction.request.url == page ? .allow : .cancel
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { requestClose() }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { requestClose() }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { requestClose() }
    private static func value(_ any: Any) throws -> DocumentValue {
        guard JSONSerialization.isValidJSONObject(any) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        let data = try JSONSerialization.data(withJSONObject: any)
        guard data.count <= 64 * 1024 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return try JSONDecoder().decode(DocumentValue.self, from: data)
    }
}

struct NativeGPUCandidatePreviewView: NSViewRepresentable {
    @ObservedObject var controller: NativeGPUCandidatePreviewController
    final class Coordinator {
        let controller: NativeGPUCandidatePreviewController
        init(_ controller: NativeGPUCandidatePreviewController) { self.controller = controller }
    }
    func makeCoordinator() -> Coordinator { Coordinator(controller) }
    func makeNSView(context: NSViewRepresentableContext<NativeGPUCandidatePreviewView>) -> WKWebView {
        let config = WKWebViewConfiguration(); config.websiteDataStore = .nonPersistent()
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        config.userContentController.add(controller, name: "exhibitosViewport")
        #if DEBUG
        config.userContentController.add(controller, name: "exhibitosGPUDecodeObservation")
        #endif
        let web = WKWebView(frame: .zero, configuration: config)
        web.setAccessibilityLabel("감상용 3D 파일 미리보기")
        controller.attach(web)
        return web
    }
    func updateNSView(_ web: WKWebView, context: NSViewRepresentableContext<NativeGPUCandidatePreviewView>) {}
    static func dismantleNSView(_ web: WKWebView, coordinator: Coordinator) {
        // The controller retains the operation and its unknown drain state.
        coordinator.controller.requestClose()
        web.navigationDelegate = nil
    }
}

struct NativeGPUCandidatePreviewSheet: View {
    @ObservedObject var controller: NativeGPUCandidatePreviewController
    let approve: () -> Void
    let dismiss: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("감상용 파일 미리보기").font(.headline)
            NativeGPUCandidatePreviewView(controller: controller)
                .frame(minWidth: 480, minHeight: 360)
            Text(controller.message).foregroundStyle(.secondary)
            HStack {
                Spacer()
                if controller.approvalAvailable { Button("감상용 파일 승인", action: approve).buttonStyle(.borderedProminent) }
                if controller.drained {
                    Button("닫기", action: dismiss)
                } else {
                    Button("미리보기 종료") { controller.requestClose() }
                    Button("창 닫기") { controller.requestClose(); dismiss() }
                }
            }
        }.padding(20)
    }
}
