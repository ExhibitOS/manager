// SPDX-License-Identifier: Apache-2.0
import AppKit
import Foundation
import ManagerCore
import SwiftUI
import WebKit

/// Owns a visible WK operation until the matching renderer confirms drain.
/// An interrupted or failed cleanup never becomes a drain receipt.
@MainActor
final class NativeGPUCandidatePreviewController: NSObject, ObservableObject, Identifiable, WKNavigationDelegate, WKScriptMessageHandler {
    let id = UUID()
    @Published private(set) var presented = false
    @Published private(set) var drained = false
    @Published private(set) var message = "감상용 파일을 준비하고 있습니다."
    private let credentials: LocalAuthoringSession
    private let assets: Set<String>
    private let checkCurrent: @MainActor () async throws -> Void
    private var lifecycle: NativeGPUCandidatePreviewLifecycle
    private var web: WKWebView?
    private var page: URL?
    private var beginIssued = false
    private var cancelled = false
    private var cancelInFlight = false
    private var task: Task<Void, Never>?

    init(credentials: LocalAuthoringSession, assets: Set<String>, request: NativeGPUCandidatePreviewRequest,
         checkCurrent: @escaping @MainActor () async throws -> Void) {
        self.credentials = credentials; self.assets = assets; lifecycle = .init(request: request); self.checkCurrent = checkCurrent
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
        lifecycle.invalidate(); presented = false; message = text
    }

    private func start() async {
        guard let web else { return }
        do {
            let req = lifecycle.request.value
            guard let page = URL(string: credentials.origin + "/native-viewport.html"),
                  credentials.tenantId == req["tenantId"].string, credentials.principalId == req["principalId"].string,
                  credentials.token.utf8.count == 43,
                  credentials.token.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95 }) else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
            self.page = page
            let rules = try NativeGPUCandidatePreviewPolicy.encodedRules(origin: credentials.origin, request: lifecycle.request, verifiedAssets: assets)
            let name = "ExhibitOSGPUPreview-" + UUID().uuidString
            let rule = try await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: name, encodedContentRuleList: rules)
            try? await WKContentRuleListStore.default().removeContentRuleList(forIdentifier: name)
            guard !cancelled, let rule else { throw AuthoringFailure("GPU_MODEL_PREPARATION_UNAVAILABLE") }
            web.configuration.userContentController.add(rule)
            let headers = ["Set-Cookie": "exhibitos_local_session=\(credentials.token); Path=/; HttpOnly; SameSite=Strict"]
            guard let cookie = HTTPCookie.cookies(withResponseHeaderFields: headers, for: page).first, cookie.isHTTPOnly else { throw AuthoringFailure("LOCAL_SESSION_INVALID") }
            await web.configuration.websiteDataStore.httpCookieStore.setCookie(cookie)
            guard !cancelled else { return }
            web.load(URLRequest(url: page, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 15))
            let deadline = ContinuousClock.now.advanced(by: .seconds(30))
            while true {
                try Task.checkCancellation()
                guard !cancelled, ContinuousClock.now < deadline else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                if !web.isLoading, web.url == page,
                   (try? await web.callAsyncJavaScript("return typeof window.exhibitosViewport?.gpuCandidatePreview === 'function';", arguments: [:], in: nil, contentWorld: .page)) as? Bool == true { break }
                try await Task.sleep(for: .milliseconds(50))
            }
            let cap = try await web.callAsyncJavaScript("return await window.exhibitosViewport.gpuCandidatePreview({version:1,phase:'capability'});", arguments: [:], in: nil, contentWorld: .page)
            let capability = try Self.value(cap)
            guard capability == .object(["version": .integer(1), "capability": .string(NativeGPUCandidatePreviewRequest.capability), "profile": .string(NativeGPUModelPreparation.profile)]),
                  !cancelled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
            while web.window?.isVisible != true || web.isHiddenOrHasHiddenAncestor || web.bounds.width <= 0 || web.bounds.height <= 0 {
                try Task.checkCancellation()
                guard !cancelled, ContinuousClock.now < deadline else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                try await Task.sleep(for: .milliseconds(50))
            }
            try await checkCurrent()
            guard !cancelled else { return }
            let json = String(decoding: try JSONEncoder().encode(lifecycle.request.value), as: UTF8.self)
            beginIssued = true
            let response = try await web.callAsyncJavaScript("return await window.exhibitosViewport.gpuCandidatePreview(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
            let ready = try Self.value(response)
            guard !cancelled, lifecycle.request.acceptsReady(ready), web.window?.isVisible == true, !web.isHiddenOrHasHiddenAncestor else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            try await checkCurrent()
            guard !cancelled, web.window?.isVisible == true, !web.isHiddenOrHasHiddenAncestor, lifecycle.acceptReady(ready) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            presented = true; message = "감상용 파일을 표시했습니다."
        } catch {
            if cancelled { return }
            invalidate("감상용 파일을 표시하지 못했습니다. 작업 상태를 확인하세요.")
            if beginIssued { await cancelAndDrain() } else { finishWithoutBegin() }
        }
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
        guard message.name == "exhibitosViewport", message.frameInfo.isMainFrame, message.webView === web,
              let value = try? Self.value(message.body),
              lifecycle.acceptEnded(value) else { return }
        presented = false
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
    let dismiss: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("감상용 파일 미리보기").font(.headline)
            NativeGPUCandidatePreviewView(controller: controller)
                .frame(minWidth: 480, minHeight: 360)
            Text(controller.message).foregroundStyle(.secondary)
            HStack {
                Spacer()
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
