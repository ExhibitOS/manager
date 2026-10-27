// SPDX-License-Identifier: Apache-2.0
import XCTest
import WebKit
@testable import ManagerCore
@MainActor
final class ViewportResourcePolicyTests: XCTestCase {
    func testRegisteredSurfaceMapsAreExactOriginPathsInActualWebKitRules() async throws {
        let origin = "http://127.0.0.1:13200", tenant = UUID().uuidString.lowercased()
        let encoded = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant)
        let store = WKContentRuleListStore.default()!, identifier = "ExhibitOSSurfaceRules-" + UUID().uuidString
        _ = try await store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: encoded)
        try await store.removeContentRuleList(forIdentifier: identifier)
        let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
        XCTAssertEqual((rules.first?["action"] as? [String: String])?["type"], "block")
        let expressions = try rules.dropFirst().map { rule -> NSRegularExpression in
            let trigger = try XCTUnwrap(rule["trigger"] as? [String: Any])
            XCTAssertEqual(trigger["url-filter-is-case-sensitive"] as? Bool, true)
            let pattern = try XCTUnwrap(trigger["url-filter"] as? String)
            XCTAssertFalse(pattern.contains("|"))
            return try NSRegularExpression(pattern: pattern)
        }
        func allows(_ url: String) -> Bool {
            expressions.contains { $0.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)) != nil }
        }
        let paths = [
            "surface-material-textures/museum-matte-paint/1/color-01fee8fd9374.png",
            "surface-material-textures/museum-matte-paint/1/normal-10246edc0ab3.png",
            "surface-material-textures/museum-matte-paint/1/roughness-b62c048f8819.png",
            "surface-material-textures/fine-lime-plaster/1/color-73294b172675.png",
            "surface-material-textures/fine-lime-plaster/1/normal-36d7e70a9317.png",
            "surface-material-textures/fine-lime-plaster/1/roughness-bb774369bf82.png"
        ]
        XCTAssertEqual(expressions.filter { $0.pattern.contains("surface") }.count, paths.count)
        for path in paths {
            XCTAssertTrue(allows(origin + "/" + path))
            for denied in [
                "https://127.0.0.1:13200/" + path,
                "http://127.0.0.1:13201/" + path,
                "http://localhost:13200/" + path,
                "https://attacker.invalid/" + path,
                origin + "/" + path.uppercased(),
                origin + "/" + path + "?extra=1",
                origin + "/" + path + "/extra",
                origin + "/" + path.replacingOccurrences(of: ".png", with: "Xpng"),
                origin + "/" + path.replacingOccurrences(of: "/1/", with: "/2/"),
                origin + "/../" + path,
                origin + "/%2e%2e/" + path
            ] { XCTAssertFalse(allows(denied), denied) }
        }
        for path in ["surface-material-textures/unknown/1/color.png", "floor-textures/unknown/1/color.png",
                     "surface-material-textures/museum-matte-paint/1/Height.png", "api/v1/tenants/\(tenant)/studio/materials/private"] {
            XCTAssertFalse(allows(origin + "/" + path))
        }
        XCTAssertTrue(allows(origin + "/native-viewport.html"))
        XCTAssertTrue(allows(origin + "/api/v1/auth/session"))
        XCTAssertFalse(allows(origin + "/api/v1/tenants/\(UUID().uuidString.lowercased())/studio/artworks/\(UUID().uuidString.lowercased())"))
    }
    func testCatalogReadRoutesStayPinnedToExactOriginTenantAndArtworkPaths() async throws {
        let tenant = UUID().uuidString.lowercased(), artwork = UUID().uuidString.lowercased()
        let origin = "http://127.0.0.1:13200"
        let encoded = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant)
        let store = WKContentRuleListStore.default()!, identifier = "ExhibitOSCatalogRules-" + UUID().uuidString
        let compiled = try await store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: encoded)
        XCTAssertNotNil(compiled)
        try await store.removeContentRuleList(forIdentifier: identifier)
        let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
        XCTAssertEqual((rules.first?["action"] as? [String: String])?["type"], "block")
        func allows(_ url: String) throws -> Bool {
            var allowed = false
            for rule in rules {
                let trigger = try XCTUnwrap(rule["trigger"] as? [String: Any])
                let pattern = try XCTUnwrap(trigger["url-filter"] as? String)
                let expression = try NSRegularExpression(pattern: pattern)
                if expression.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)) != nil {
                    allowed = (rule["action"] as? [String: String])?["type"] == "ignore-previous-rules"
                }
            }
            return allowed
        }
        let path = "/api/v1/tenants/\(tenant)/cms/artworks/\(artwork)"
        for suffix in ["", "/display", "/preview?expectedRevision=1"] {
            XCTAssertTrue(try allows(origin + path + suffix), suffix)
            XCTAssertFalse(try allows("http://127.0.0.1:13201" + path + suffix))
            XCTAssertFalse(try allows("https://attacker.invalid" + path + suffix))
            XCTAssertFalse(try allows(origin + path.replacingOccurrences(of: tenant, with: UUID().uuidString.lowercased()) + suffix))
        }
        for denied in [path + "?extra=1", path + "/display?extra=1", path + "/revisions", path + "/display/export", "/api/v1/tenants/\(tenant)/cms/artworks", "/api/v1/tenants/\(tenant)/cms/artists/\(artwork)"] {
            XCTAssertFalse(try allows(origin + denied), denied)
        }
    }
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
