// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import WebKit
import ManagerCore

struct NativeStaticViewerSheet: View {
    let entry: NativeStaticViewerEntry
    let close: () -> Void
    @State private var failed = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("내보낸 전시").font(.headline)
                Text("리비전 " + String(entry.revisionSha256.prefix(12)))
                    .foregroundStyle(.secondary)
                    .help(entry.revisionSha256)
                Spacer()
                Button("닫기", action: close).keyboardShortcut(.cancelAction)
            }
            .padding()
            Divider()
            StaticViewerWebView(entry: entry, failed: $failed)
                .overlay {
                    if failed {
                        ContentUnavailableView("내보낸 전시를 열 수 없습니다", systemImage: "exclamationmark.triangle",
                            description: Text("창을 닫고 저장 폴더의 파일을 확인한 뒤 다시 여세요."))
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            .background(.background)
                    }
                }
        }
        .frame(minWidth: 700, idealWidth: 1000, minHeight: 440, idealHeight: 700)
    }
}

private struct StaticViewerWebView: NSViewRepresentable {
    let entry: NativeStaticViewerEntry
    @Binding var failed: Bool

    func makeCoordinator() -> Coordinator { Coordinator(origin: entry.url, failed: $failed) }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = context.coordinator
        var request = URLRequest(url: entry.url, cachePolicy: .reloadIgnoringLocalCacheData)
        request.setValue(entry.authorization, forHTTPHeaderField: "Authorization")
        view.load(request)
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {}
    static func dismantleNSView(_ view: WKWebView, coordinator: Coordinator) {
        view.navigationDelegate = nil
        view.stopLoading()
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        private let origin: URL
        @Binding private var failed: Bool

        init(origin: URL, failed: Binding<Bool>) {
            self.origin = origin
            _failed = failed
        }

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            guard action.targetFrame?.isMainFrame == true, let url = action.request.url,
                  url.scheme == origin.scheme, url.host == origin.host, url.port == origin.port,
                  url.path == origin.path || url.path == "/" else { return .cancel }
            return .allow
        }

        func webView(_ webView: WKWebView, decidePolicyFor response: WKNavigationResponse) async -> WKNavigationResponsePolicy {
            guard let http = response.response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
                failed = true
                return .cancel
            }
            return .allow
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            if (error as NSError).code != NSURLErrorCancelled { failed = true }
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            if (error as NSError).code != NSURLErrorCancelled { failed = true }
        }

        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { failed = true }
    }
}
