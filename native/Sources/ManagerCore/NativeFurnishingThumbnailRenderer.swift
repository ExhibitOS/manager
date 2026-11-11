// SPDX-License-Identifier: Apache-2.0
import AppKit
import Foundation
import WebKit

/// A single-operation host for the authenticated product viewport. The caller
/// must recheck the live session, saved frame, descriptor and rights after await.
@MainActor
final class NativeFurnishingThumbnailRenderer: NSObject, WKNavigationDelegate {
    struct Result: Sendable {
        let request: ViewportFurnishingThumbnailRequest
        let pngData: Data
        let pngSha256: String
    }

    private var web: WKWebView?
    private var page: URL?
    private var started = false
    private var stopped = false

    func cancel() {
        stopped = true
        web?.navigationDelegate = nil
        web?.stopLoading()
        web?.loadHTMLString("", baseURL: nil)
    }

    private func checkActive() throws {
        try Task.checkCancellation()
        guard !stopped else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
    }

    func render(credentials: LocalAuthoringSession, frame: ViewportFrame,
                furnishing: WorkspaceFurnishing) async throws -> Result {
        guard !started else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        started = true
        return try await withTaskCancellationHandler {
            try await renderOnce(credentials: credentials, frame: frame, furnishing: furnishing)
        } onCancel: {
            Task { @MainActor in self.cancel() }
        }
    }

    private func renderOnce(credentials: LocalAuthoringSession, frame: ViewportFrame,
                            furnishing: WorkspaceFurnishing) async throws -> Result {
        try checkActive()
        guard let workspace = frame.workspaceId, UUID(uuidString: workspace) != nil,
              let etag = frame.baseETag, !etag.isEmpty,
              let page = URL(string: credentials.origin + "/native-viewport.html"),
              page.scheme == "http", page.host == "127.0.0.1", page.user == nil, page.password == nil,
              let port = page.port, port >= 1024,
              credentials.origin == "http://127.0.0.1:\(port)",
              credentials.tenantId == frame.tenantId, credentials.principalId == frame.principalId,
              credentials.token.utf8.count == 43,
              credentials.token.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95 }) else {
            throw AuthoringFailure("LOCAL_SCOPE_INVALID")
        }
        let request = try ViewportFurnishingThumbnailRequest(frame: frame, furnishing: furnishing)
        let timeout = Task { @MainActor [weak self] in
            do { try await Task.sleep(for: .seconds(60)) } catch { return }
            self?.cancel()
        }
        defer {
            timeout.cancel()
            web?.stopLoading(); web?.navigationDelegate = nil
            web?.configuration.userContentController.removeAllContentRuleLists()
            web = nil; self.page = nil; stopped = true
        }
        _ = NSApplication.shared
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        let rules = try Self.resourceRules(origin: credentials.origin, tenantId: frame.tenantId)
        let name = "ExhibitOSFurnishingThumbnail-" + UUID().uuidString
        let rule = try await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: name, encodedContentRuleList: rules)
        try? await WKContentRuleListStore.default().removeContentRuleList(forIdentifier: name)
        try checkActive()
        guard let rule else { throw AuthoringFailure("VIEWPORT_POLICY_UNAVAILABLE") }
        configuration.userContentController.add(rule)
        let headers = ["Set-Cookie": "exhibitos_local_session=\(credentials.token); Path=/; HttpOnly; SameSite=Strict"]
        guard let cookie = HTTPCookie.cookies(withResponseHeaderFields: headers, for: page).first,
              cookie.isHTTPOnly else { throw AuthoringFailure("LOCAL_SESSION_INVALID") }
        await configuration.websiteDataStore.httpCookieStore.setCookie(cookie)
        try checkActive()
        let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 256, height: 256), configuration: configuration)
        self.web = web; self.page = page; web.navigationDelegate = self
        web.load(URLRequest(url: page, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 15))
        let deadline = ContinuousClock.now.advanced(by: .seconds(30))
        while true {
            try checkActive()
            guard ContinuousClock.now < deadline else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
            if !web.isLoading, web.url == page,
               (try? await web.callAsyncJavaScript("return typeof window.exhibitosViewport?.accept === 'function';", arguments: [:], in: nil, contentWorld: .page)) as? Bool == true { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        try checkActive()
        let frameJSON = String(decoding: try JSONEncoder().encode(frame), as: UTF8.self)
        let accepted = try await web.callAsyncJavaScript("return await window.exhibitosViewport.accept(JSON.parse(frameJSON));", arguments: ["frameJSON": frameJSON], in: nil, contentWorld: .page)
        try checkActive()
        guard let fields = accepted as? [String: Any], JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        let acceptance = try JSONDecoder().decode(ViewportAcceptance.self, from: JSONSerialization.data(withJSONObject: fields))
        guard acceptance.supportsFurnishingThumbnail else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let json = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
        let response = try await web.callAsyncJavaScript("return await window.exhibitosViewport.furnishingThumbnailV1(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
        try checkActive()
        return try Self.checkedReply(response, request: request)
    }

    static func checkedReply(_ response: Any, request: ViewportFurnishingThumbnailRequest) throws -> Result {
        let keys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "furnishingId", "furnishingVersion", "modelSha256", "modelBytes", "recipeId", "toolId", "pngBase64", "pngSha256", "width", "height"]
        guard let fields = response as? [String: Any], Set(fields.keys) == keys,
              let png = fields["pngBase64"] as? String, png.utf8.count <= 699_052,
              JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        let bytes = try JSONSerialization.data(withJSONObject: fields)
        guard bytes.count <= 710_000 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        let data = try JSONDecoder().decode(ViewportFurnishingThumbnailReply.self, from: bytes).pngData(matching: request)
        return Result(request: request, pngData: data, pngSha256: nativeImportSHA256(data))
    }

    static func resourceRules(origin: String, tenantId: String) throws -> String {
        try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenantId, workspaceFurnishings: true)
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
        navigationAction.targetFrame?.isMainFrame == true && navigationAction.request.url == page ? .allow : .cancel
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { cancel() }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { cancel() }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { cancel() }
}
