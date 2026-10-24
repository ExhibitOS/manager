// SPDX-License-Identifier: Apache-2.0
import XCTest
import WebKit
@testable import ManagerCore
@MainActor
final class ViewportResourcePolicyTests: XCTestCase {
    func testJSONWirePreservesIndependentEmptyDocumentCollectionsInActualWebKit() async throws {
        let web = WKWebView(frame: .zero)
        let json = "{\"surfaces\":[],\"openings\":[],\"lights\":[],\"placements\":[]}"
        let value = try await web.callAsyncJavaScript("const candidate = JSON.parse(documentJSON); candidate.surfaces.push({id:'surface'}); return [candidate.surfaces.length, candidate.openings.length, candidate.lights.length, candidate.placements.length];", arguments: ["documentJSON":json], in: nil, contentWorld: .page)
        XCTAssertEqual(value as? [Int], [1,0,0,0])
        let foundation = try JSONSerialization.jsonObject(with: Data(json.utf8))
        let legacy = try await web.callAsyncJavaScript("candidate.surfaces.push({id:'surface'}); return [candidate.surfaces.length, candidate.openings.length, candidate.lights.length, candidate.placements.length];", arguments: ["candidate":foundation], in: nil, contentWorld: .page)
        // Diagnostic of the old bridge; only JSON wire independence is the contract.
        print("Legacy Foundation array lengths: \(legacy as? [Int] ?? [])")
    }
    func testActualWebKitCompilerAcceptsPinnedSeparateRulesAndRefusesOriginalDisjunction() async throws {
        let tenant = UUID().uuidString.lowercased()
        let encoded = try ViewportResourcePolicy.encodedRules(origin: "http://127.0.0.1:13200", tenantId: tenant)
        let store = WKContentRuleListStore.default()!
        let badId = "ExhibitOSViewportBad-" + UUID().uuidString
        let bad = #"[{"trigger":{"url-filter":"^http://127\\.0\\.0\\.1:13200/(native-viewport\\.html$|assets/.*$)"},"action":{"type":"block"}}]"#
        do { _ = try await store.compileContentRuleList(forIdentifier: badId, encodedContentRuleList: bad); XCTFail("Original internal anchors/disjunction must not be accepted") }
        catch { XCTAssertEqual((error as NSError).domain, "WKErrorDomain") }
        try? await store.removeContentRuleList(forIdentifier: badId)
        let goodId = "ExhibitOSViewportGood-" + UUID().uuidString
        let rules = try await store.compileContentRuleList(forIdentifier: goodId, encodedContentRuleList: encoded)
        XCTAssertNotNil(rules)
        try await store.removeContentRuleList(forIdentifier: goodId)
        let cookie = HTTPCookie.cookies(withResponseHeaderFields: ["Set-Cookie":"exhibitos_local_session=" + String(repeating: "a", count: 43) + "; Path=/; HttpOnly; SameSite=Strict"], for: URL(string: "http://127.0.0.1:13200/native-viewport.html")!).first
        XCTAssertTrue(try XCTUnwrap(cookie).isHTTPOnly)
        let object = try JSONSerialization.jsonObject(with: Data(encoded.utf8)) as! [[String:Any]]
        for rule in object.dropFirst() {
            let trigger = rule["trigger"] as! [String:Any], filter = trigger["url-filter"] as! String
            XCTAssertFalse(filter.contains("|")); XCTAssertTrue(filter.hasPrefix("^http://127\\.0\\.0\\.1:13200/") || filter == "^blob:" || filter == "^data:")
        }
        XCTAssertThrowsError(try ViewportResourcePolicy.encodedRules(origin: "https://attacker.invalid", tenantId: tenant))
        XCTAssertThrowsError(try ViewportResourcePolicy.encodedRules(origin: "http://127.0.0.1:13200", tenantId: "invalid"))
    }
}
