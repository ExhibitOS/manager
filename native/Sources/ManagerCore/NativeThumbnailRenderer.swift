// SPDX-License-Identifier: Apache-2.0
import AppKit
import Foundation
import WebKit

/// An operation-owned host for the product renderer. It has no editing controls,
/// persistent browser store, capture flags, or caller-supplied JavaScript.
@MainActor
final class NativeThumbnailRenderer: NSObject, WKNavigationDelegate {
    private var web: WKWebView?
    private var page: URL?
    private var failed = false

    func cancel() {
        failed = true
        web?.navigationDelegate = nil
        web?.stopLoading()
        // Replacing the page also terminates an in-flight async JS callback.
        web?.loadHTMLString("", baseURL: nil)
    }

    func render(credentials: LocalAuthoringSession, frame: ViewportFrame,
                artworkId: String, revision: Int) async throws -> (ViewportThumbnailRequest, ViewportThumbnailReply) {
        guard web == nil, let page = URL(string: credentials.origin + "/native-viewport.html"),
              page.scheme == "http", page.host == "127.0.0.1", page.user == nil, page.password == nil,
              let port = page.port, port >= 1024,
              credentials.tenantId == frame.tenantId, credentials.principalId == frame.principalId,
              credentials.token.range(of: "^[A-Za-z0-9_-]{43}$", options: .regularExpression) != nil else {
            throw AuthoringFailure("LOCAL_SCOPE_INVALID")
        }
        _ = NSApplication.shared
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        let rules = try ViewportResourcePolicy.encodedRules(origin: credentials.origin, tenantId: frame.tenantId)
        let name = "ExhibitOSThumbnail-" + UUID().uuidString
        let rule = try await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: name, encodedContentRuleList: rules)
        try? await WKContentRuleListStore.default().removeContentRuleList(forIdentifier: name)
        guard let rule else { throw AuthoringFailure("VIEWPORT_POLICY_UNAVAILABLE") }
        configuration.userContentController.add(rule)
        let headers = ["Set-Cookie": "exhibitos_local_session=\(credentials.token); Path=/; HttpOnly; SameSite=Strict"]
        guard let cookie = HTTPCookie.cookies(withResponseHeaderFields: headers, for: page).first,
              cookie.isHTTPOnly else { throw AuthoringFailure("LOCAL_SESSION_INVALID") }
        await configuration.websiteDataStore.httpCookieStore.setCookie(cookie)
        let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 256, height: 256), configuration: configuration)
        self.web = web; self.page = page; failed = false
        web.navigationDelegate = self
        let timeout = Task { @MainActor [weak self] in
            do { try await Task.sleep(for: .seconds(60)) } catch { return }
            self?.cancel()
        }
        defer {
            timeout.cancel()
            web.stopLoading(); web.navigationDelegate = nil
            configuration.userContentController.removeAllContentRuleLists()
            self.web = nil; self.page = nil
        }
        web.load(URLRequest(url: page, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 15))
        let deadline = ContinuousClock.now.advanced(by: .seconds(30))
        while true {
            try Task.checkCancellation()
            guard !failed, ContinuousClock.now < deadline else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
            if !web.isLoading, web.url == page,
               (try? await web.callAsyncJavaScript("return typeof window.exhibitosViewport?.accept === 'function';", arguments: [:], in: nil, contentWorld: .page)) as? Bool == true { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        let frameJSON = String(decoding: try JSONEncoder().encode(frame), as: UTF8.self)
        let accepted = try await web.callAsyncJavaScript("return await window.exhibitosViewport.accept(JSON.parse(frameJSON));", arguments: ["frameJSON": frameJSON], in: nil, contentWorld: .page)
        guard let fields = accepted as? [String: Any], JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        let acceptance = try JSONDecoder().decode(ViewportAcceptance.self, from: JSONSerialization.data(withJSONObject: fields))
        guard acceptance.accepted, acceptance.supportsRenderedArtworkThumbnail else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let request = try ViewportThumbnailRequest(frame: frame, artworkId: artworkId, expectedRevision: revision)
        let json = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
        let response = try await web.callAsyncJavaScript("return await window.exhibitosViewport.thumbnail(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
        try Task.checkCancellation()
        guard !failed else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        guard let fields = response as? [String: Any], let png = fields["pngBase64"] as? String,
              png.utf8.count <= 699_052, JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        let data = try JSONSerialization.data(withJSONObject: fields)
        guard data.count <= 710_000 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        let reply = try JSONDecoder().decode(ViewportThumbnailReply.self, from: data)
        _ = try reply.renderedThumbnail(matching: request)
        return (request, reply)
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
        navigationAction.targetFrame?.isMainFrame == true && navigationAction.request.url == page ? .allow : .cancel
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { failed = true }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { failed = true }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { failed = true }
}
