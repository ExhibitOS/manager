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
        let surfacePaths = [
            "surface-material-textures/museum-matte-paint/1/color-01fee8fd9374.png",
            "surface-material-textures/museum-matte-paint/1/normal-10246edc0ab3.png",
            "surface-material-textures/museum-matte-paint/1/roughness-b62c048f8819.png",
            "surface-material-textures/fine-lime-plaster/1/color-73294b172675.png",
            "surface-material-textures/fine-lime-plaster/1/normal-36d7e70a9317.png",
            "surface-material-textures/fine-lime-plaster/1/roughness-bb774369bf82.png",
            "surface-material-textures/fine-gallery-concrete/1/color-634a67d2a076.png",
            "surface-material-textures/fine-gallery-concrete/1/normal-124a39f69958.png",
            "surface-material-textures/fine-gallery-concrete/1/roughness-d773c26d56f0.png"
        ]
        let floorPaths = [
            "floor-textures/Carpet004/1/ao-cba7d7a73cc9.png",
            "floor-textures/Carpet004/1/color-df90d6a3be6b.png",
            "floor-textures/Carpet004/1/normal-gl-956a4ad49737.png",
            "floor-textures/Carpet004/1/roughness-6a60aa6770e6.png",
            "floor-textures/Marble006/1/color-d44e0a2b0402.png",
            "floor-textures/Marble006/1/normal-gl-a61a93414470.png",
            "floor-textures/Marble006/1/roughness-cd5898bc49d9.png",
            "floor-textures/Tiles074/1/color-99d35940d5ae.png",
            "floor-textures/Tiles074/1/normal-gl-89ad085427ad.png",
            "floor-textures/Tiles074/1/roughness-538f7d7c0645.png",
            "floor-textures/WoodFloor051/1/color-68bad517cc4a.png",
            "floor-textures/WoodFloor051/1/normal-gl-23d13c247759.png",
            "floor-textures/WoodFloor051/1/roughness-8c578dfb7c61.png"
        ]
        let paths = surfacePaths + floorPaths
        XCTAssertEqual(expressions.filter { $0.pattern.contains("surface") }.count, surfacePaths.count)
        XCTAssertEqual(expressions.filter { $0.pattern.contains("floor") }.count, floorPaths.count)
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
    func testViewportCheckRouteIsLimitedToOriginTenantAndCanonicalExhibitionID() async throws {
        let origin = "http://127.0.0.1:13200", tenant = UUID().uuidString.lowercased(), exhibition = UUID().uuidString.lowercased()
        let encoded = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant)
        let store = WKContentRuleListStore.default()!, identifier = "ExhibitOSViewportCheck-" + UUID().uuidString
        _ = try await store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: encoded)
        try await store.removeContentRuleList(forIdentifier: identifier)
        let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
        let filters = try rules.dropFirst().map { try NSRegularExpression(pattern: XCTUnwrap(($0["trigger"] as? [String: Any])?["url-filter"] as? String)) }
        func allows(_ url: String) -> Bool { filters.contains { $0.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)) != nil } }
        let path = "/api/v1/tenants/\(tenant)/studio/exhibitions/\(exhibition)/native-viewport-check"
        XCTAssertTrue(allows(origin + path))
        XCTAssertTrue(allows(origin + path.replacingOccurrences(of: exhibition, with: exhibition.uppercased())))
        var badVersion = Array(exhibition); badVersion[14] = "0"
        var badVariant = Array(exhibition); badVariant[19] = "0"
        for denied in ["http://127.0.0.1:13201" + path, "https://127.0.0.1:13200" + path,
                       origin + path.replacingOccurrences(of: tenant, with: UUID().uuidString.lowercased()),
                       origin + path.replacingOccurrences(of: exhibition, with: "not-a-uuid"),
                       origin + path.replacingOccurrences(of: exhibition, with: String(badVersion)),
                       origin + path.replacingOccurrences(of: exhibition, with: String(badVariant)),
                       origin + path + "?extra=1", origin + path + "/extra",
                       origin + path.replacingOccurrences(of: "/native-viewport-check", with: "%2Fnative-viewport-check"),
                       origin + path.replacingOccurrences(of: "native-viewport-check", with: "publish"),
                       origin + "/api/v1/tenants/\(tenant)/studio/exhibitions/\(exhibition)"] {
            XCTAssertFalse(allows(denied), denied)
        }
    }
    func testCaptureSavedSourceReadIsBoundToOneExhibitionAndDefaultStillBlocksIt() async throws {
        let origin = "http://127.0.0.1:13200", tenant = UUID().uuidString.lowercased(), exhibition = UUID().uuidString.lowercased()
        let path = "/api/v1/tenants/\(tenant)/studio/exhibitions/\(exhibition)"
        let encoded = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant, savedExhibitionId: exhibition)
        let store = WKContentRuleListStore.default()!, identifier = "ExhibitOSCaptureRules-" + UUID().uuidString
        let compiled = try await store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: encoded)
        XCTAssertNotNil(compiled)
        try await store.removeContentRuleList(forIdentifier: identifier)
        func filters(_ encoded: String) throws -> [NSRegularExpression] {
            let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
            return try rules.dropFirst().map { try NSRegularExpression(pattern: XCTUnwrap(($0["trigger"] as? [String: Any])?["url-filter"] as? String)) }
        }
        let capture = try filters(encoded)
        func allows(_ url: String, rules: [NSRegularExpression]) -> Bool {
            rules.contains { $0.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)) != nil }
        }
        XCTAssertTrue(allows(origin + path, rules: capture))
        for denied in ["http://127.0.0.1:13201" + path, "https://127.0.0.1:13200" + path,
                       "https://attacker.invalid" + path,
                       origin + path.replacingOccurrences(of: tenant, with: UUID().uuidString.lowercased()),
                       origin + path.replacingOccurrences(of: exhibition, with: UUID().uuidString.lowercased()),
                       origin + path + "?extra=1", origin + path + "/", origin + path + "/publish",
                       origin + path.replacingOccurrences(of: "/studio/exhibitions/", with: "/studio%2Fexhibitions/")] {
            XCTAssertFalse(allows(denied, rules: capture), denied)
        }
        let restored = try filters(ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant))
        XCTAssertFalse(allows(origin + path, rules: restored))
        var badVersion = Array(exhibition); badVersion[14] = "0"
        var badVariant = Array(exhibition); badVariant[19] = "0"
        for invalid in ["invalid", String(badVersion), String(badVariant)] {
            XCTAssertThrowsError(try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant, savedExhibitionId: invalid))
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
    func testPrivateFloorAddsOnlyExactOakMapRulesWhenEnabled() throws {
        let origin = "http://127.0.0.1:13200"
        let tenant = UUID().uuidString.lowercased()
        func filters(privateFloor: Bool) throws -> Set<String> {
            let encoded = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant, privateFloor: privateFloor)
            let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
            return Set(try rules.map { rule in
                let trigger = try XCTUnwrap(rule["trigger"] as? [String: Any])
                return try XCTUnwrap(trigger["url-filter"] as? String)
            })
        }
        let legacy = try filters(privateFloor: false)
        let paired = try filters(privateFloor: true)
        XCTAssertTrue(legacy.isSubset(of: paired))
        let paths = ["color-4e001eaaba46.png", "normal-21bddb22f10b.png", "roughness-f889803f9d84.png"]
        let expected = Set(paths.map {
            "^http://127\\.0\\.0\\.1:13200/" + NSRegularExpression.escapedPattern(for: "floor-textures/quiet-oak-herringbone/1/" + $0) + "$"
        })
        XCTAssertEqual(paired.subtracting(legacy), expected)
    }

    func testWorkspaceMaterialRulesRequireQualifiedSameTenantAndExactColorPaths() async throws {
        let origin = "http://127.0.0.1:13200"
        let tenant = UUID().uuidString.lowercased()
        let material = "1cbbc396-a6b8-4a44-88fe-7f7a7ffc4fd1"
        let collection = "/api/v1/tenants/\(tenant)/workspace/materials"
        let version = "\(collection)/\(material)/versions/1"
        func rules(_ encoded: String) throws -> Set<String> {
            let rows = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
            return Set(try rows.dropFirst().map { row in
                let trigger = try XCTUnwrap(row["trigger"] as? [String: Any])
                return try XCTUnwrap(trigger["url-filter"] as? String)
            })
        }
        let base = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant)
        let scoped = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant, workspaceMaterials: true)
        let store = WKContentRuleListStore.default()!
        let identifier = "ExhibitOSWorkspaceColor-" + UUID().uuidString
        let compiled = try await store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: scoped)
        XCTAssertNotNil(compiled)
        try await store.removeContentRuleList(forIdentifier: identifier)
        let added = try rules(scoped).subtracting(rules(base))
        XCTAssertEqual(added.count, 2)
        let expressions = try added.map { try NSRegularExpression(pattern: $0) }
        func allows(_ url: String) -> Bool {
            expressions.contains { $0.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)) != nil }
        }
        for path in [version, version + "/maps/color", String(version.dropLast()) + "2147483647/maps/color"] {
            XCTAssertTrue(allows(origin + path), path)
        }
        for url in [
            "http://127.0.0.1:13201" + version, "https://127.0.0.1:13200" + version,
            "http://localhost:13200" + version, origin + version.replacingOccurrences(of: tenant, with: UUID().uuidString.lowercased()),
            origin + collection, origin + version.replacingOccurrences(of: material, with: "not-a-uuid"),
            origin + version.replacingOccurrences(of: material, with: material.uppercased()),
            origin + String(version.dropLast()) + "0", origin + String(version.dropLast()) + "01",
            origin + String(version.dropLast()) + "12345678901", origin + version + "/maps/normal",
            origin + version + "/maps/color/", origin + version + "?version=2", origin + version + "/maps/color?extra=1",
            origin + collection.replacingOccurrences(of: "materials", with: "material-intakes")
        ] {
            XCTAssertFalse(allows(url), url)
        }
    }

    func testWorkspaceFurnishingReadsAreLimitedToQualifiedViewportAndCapture() async throws {
        let origin = "http://127.0.0.1:13200", tenant = UUID().uuidString.lowercased()
        let furnishing = "a0000000-0000-4000-8000-000000000001", exhibition = UUID().uuidString.lowercased()
        let path = "/api/v1/tenants/\(tenant)/workspace/furnishings/\(furnishing)/versions/1"
        let source = origin + "/api/v1/tenants/\(tenant)/studio/exhibitions/\(exhibition)"

        func filters(_ encoded: String) throws -> Set<String> {
            let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
            XCTAssertEqual((rules.first?["action"] as? [String: String])?["type"], "block")
            return Set(try rules.dropFirst().map { rule in
                let trigger = try XCTUnwrap(rule["trigger"] as? [String: Any])
                XCTAssertEqual(trigger["url-filter-is-case-sensitive"] as? Bool, true)
                return try XCTUnwrap(trigger["url-filter"] as? String)
            })
        }

        let disabled = try filters(ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant))
        let enabled = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant, workspaceFurnishings: true)
        let capture = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant, savedExhibitionId: exhibition, workspaceFurnishings: true)
        let inactiveCapture = try filters(ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant, savedExhibitionId: exhibition))
        let enabledFilters = try filters(enabled), captureFilters = try filters(capture)
        XCTAssertEqual(enabledFilters.subtracting(disabled).count, 2)
        XCTAssertEqual(captureFilters.subtracting(enabledFilters), inactiveCapture.subtracting(disabled))
        let store = WKContentRuleListStore.default()!
        for encoded in [enabled, capture] {
            let identifier = "ExhibitOSWorkspaceFurnishing-" + UUID().uuidString
            let compiled = try await store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: encoded)
            XCTAssertNotNil(compiled)
            try await store.removeContentRuleList(forIdentifier: identifier)
        }

        func allows(_ url: String, _ rules: Set<String>) -> Bool {
            rules.contains { url.range(of: $0, options: .regularExpression) != nil }
        }
        for route in ["/display-check", "/model"] {
            let url = origin + path + route
            XCTAssertTrue(allows(url, enabledFilters))
            XCTAssertTrue(allows(url, captureFilters))
            XCTAssertFalse(allows(url, disabled))
            XCTAssertFalse(allows(url, inactiveCapture))
        }
        XCTAssertFalse(allows(source, enabledFilters))
        XCTAssertTrue(allows(source, captureFilters))
        for url in [
            "http://127.0.0.1:13201" + path + "/model", "http://localhost:13200" + path + "/model",
            "https://127.0.0.1:13200" + path + "/model", "https://example.invalid" + path + "/model",
            origin + path.replacingOccurrences(of: tenant, with: UUID().uuidString.lowercased()) + "/model",
            origin + path.replacingOccurrences(of: furnishing, with: furnishing.uppercased()) + "/model",
            origin + path.replacingOccurrences(of: furnishing, with: "a0000000-0000-0000-0000-000000000001") + "/model",
            origin + path.replacingOccurrences(of: "/versions/1", with: "/versions/2") + "/model",
            origin + path.replacingOccurrences(of: "/versions/1", with: "/versions/01") + "/model",
            origin + path.replacingOccurrences(of: "/workspace/furnishings/", with: "/workspace/furnishing-intakes/") + "/model",
            origin + path, origin + path + "/model/", origin + path + "/model?extra=1",
            origin + path + "/display", origin + path + "/complete"
        ] {
            XCTAssertFalse(allows(url, enabledFilters), url)
            XCTAssertFalse(allows(url, captureFilters), url)
        }
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
