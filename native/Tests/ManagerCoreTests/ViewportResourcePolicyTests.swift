// SPDX-License-Identifier: Apache-2.0
import XCTest
import WebKit
import CryptoKit
import Darwin
@testable import ManagerCore
@MainActor
final class ViewportResourcePolicyTests: XCTestCase {
    func testRefinedBenchModelHasOnlyExactSameOriginTenantRule() async throws {
        let origin = "http://127.0.0.1:13200", tenant = UUID().uuidString.lowercased()
        let path = "/api/v1/tenants/\(tenant)/studio/furnishings/sculpture-bench-refined/1/model"
        let encoded = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant)
        let store = WKContentRuleListStore.default()!, id = "ExhibitOSRefinedBench-" + UUID().uuidString
        let compiled = try await store.compileContentRuleList(forIdentifier: id, encodedContentRuleList: encoded)
        XCTAssertNotNil(compiled)
        try await store.removeContentRuleList(forIdentifier: id)
        let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
        let patterns = try rules.dropFirst().map { rule -> NSRegularExpression in
            let trigger = try XCTUnwrap(rule["trigger"] as? [String: Any])
            XCTAssertEqual(trigger["url-filter-is-case-sensitive"] as? Bool, true)
            return try NSRegularExpression(pattern: XCTUnwrap(trigger["url-filter"] as? String))
        }
        func allows(_ url: String) -> Bool {
            patterns.contains { $0.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)) != nil }
        }
        XCTAssertTrue(allows(origin + path))
        XCTAssertTrue(allows(origin + "/api/v1/auth/session"))
        for denied in [
            origin + path + "?x=1", origin + path + "/", origin + path + "/extra",
            origin + path.replacingOccurrences(of: "/1/model", with: "/2/model"),
            origin + path.replacingOccurrences(of: "/1/model", with: "/01/model"),
            origin + path.replacingOccurrences(of: "sculpture-bench-refined", with: "backless-oak-bench"),
            origin + path.replacingOccurrences(of: "sculpture-bench-refined", with: "SCULPTURE-bench-refined"),
            origin + path.replacingOccurrences(of: tenant, with: UUID().uuidString.lowercased()),
            "http://127.0.0.1:13201" + path, "http://localhost:13200" + path,
            "https://127.0.0.1:13200" + path, "https://example.invalid" + path
        ] { XCTAssertFalse(allows(denied), denied) }
    }
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
    func testOptInIsolatedWKPNGDecodeDiagnostic() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let input = env["EXHIBITOS_WK_PNG_PATH"], let expected = env["EXHIBITOS_WK_PNG_SHA256"],
              let output = env["EXHIBITOS_WK_PNG_OUTPUT_DIR"], !input.isEmpty, !expected.isEmpty, !output.isEmpty else {
            throw XCTSkip("Requires explicit owned PNG, SHA-256 and diagnostic output directory")
        }
        func canonical(_ path: String) -> Bool {
            path.hasPrefix("/") && URL(fileURLWithPath: path).standardizedFileURL.resolvingSymlinksInPath().path == path
        }
        guard canonical(input), canonical(output), expected.count == 64,
              expected.allSatisfy({ "0123456789abcdef".contains($0) }) else {
            throw CocoaError(.fileReadInvalidFileName)
        }
        let inputFD = Darwin.open(input, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
        guard inputFD >= 0 else { throw CocoaError(.fileReadNoPermission) }
        defer { Darwin.close(inputFD) }
        var source = stat(), named = stat()
        guard fstat(inputFD, &source) == 0, lstat(input, &named) == 0,
              (source.st_mode & S_IFMT) == S_IFREG, source.st_uid == geteuid(), source.st_nlink == 1,
              source.st_size > 0, source.st_size <= 1_048_576,
              source.st_dev == named.st_dev, source.st_ino == named.st_ino else {
            throw CocoaError(.fileReadNoPermission)
        }
        let directoryFD = Darwin.open(output, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard directoryFD >= 0 else { throw CocoaError(.fileReadNoPermission) }
        defer { Darwin.close(directoryFD) }
        var directory = stat(), directoryName = stat()
        guard fstat(directoryFD, &directory) == 0, lstat(output, &directoryName) == 0,
              (directory.st_mode & S_IFMT) == S_IFDIR, directory.st_uid == geteuid(),
              (directory.st_mode & 0o7777) == 0o700,
              directory.st_dev == directoryName.st_dev, directory.st_ino == directoryName.st_ino else {
            throw CocoaError(.fileReadNoPermission)
        }
        let handle = FileHandle(fileDescriptor: inputFD, closeOnDealloc: false)
        let png = try handle.read(upToCount: 1_048_577) ?? Data()
        var after = stat()
        guard fstat(inputFD, &after) == 0, after.st_size == source.st_size,
              after.st_mtimespec.tv_sec == source.st_mtimespec.tv_sec,
              after.st_mtimespec.tv_nsec == source.st_mtimespec.tv_nsec,
              after.st_uid == source.st_uid, after.st_nlink == 1,
              after.st_ctimespec.tv_sec == source.st_ctimespec.tv_sec,
              after.st_ctimespec.tv_nsec == source.st_ctimespec.tv_nsec,
              png.count == source.st_size,
              SHA256.hash(data: png).map({ String(format: "%02x", $0) }).joined() == expected else {
            throw CocoaError(.fileReadCorruptFile)
        }
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        let web = WKWebView(frame: .zero, configuration: config)
        defer { web.stopLoading() }
        let script = #"""
        const context = { userAgent: navigator.userAgent.slice(0, 512), scheme: location.protocol,
          secureContext: isSecureContext, visibility: document.visibilityState, contentWorld: 'page' };
        let phase = 'wire', wireVerified = false, blobSize = null, bytes = null;
        try {
          const binary = atob(encoded);
          const png = Uint8Array.from(binary, c => c.charCodeAt(0));
          bytes = png.length;
          let roundtrip = ''; for (const byte of png) roundtrip += String.fromCharCode(byte);
          if (png.length !== expectedLength || btoa(roundtrip) !== encoded) {
            return { context, decode: { status: 'wire-mismatch' } };
          }
          wireVerified = true;
          const blob = new Blob([png], {type:'image/png'});
          blobSize = blob.size;
          if (blob.size !== expectedLength) return { context, decode: { status: 'blob-size-mismatch' } };
          let expired = false, timer;
          phase = 'decode';
          const pending = createImageBitmap(new Blob([png],{type:'image/png'}),{imageOrientation:'none',premultiplyAlpha:'none',colorSpaceConversion:'none'})
            .then(bitmap => { if (expired) { bitmap.close(); return null; } return bitmap; });
          const timeout = new Promise(resolve => { timer = setTimeout(() => { expired = true; resolve(null); }, 15000); });
          let bitmap;
          try { bitmap = await Promise.race([pending, timeout]); } finally { clearTimeout(timer); }
          if (!bitmap) return { context, decode: { status: 'timeout', bytes: png.length, blobSize: blob.size } };
          try { return { context, decode: { status: 'success', bytes: png.length, blobSize: blob.size,
            width: bitmap.width, height: bitmap.height } }; } finally { bitmap.close(); }
        } catch (error) {
          const name = typeof error?.name === 'string' && /^[A-Za-z]{1,40}$/.test(error.name) ? error.name : 'UnknownError';
          return { context, decode: { status: phase === 'decode' ? 'decoder-error' : 'script-error',
            name, bytes, wireVerified, blobSize } };
        }
        """#
        let resultData: Data = await withCheckedContinuation { continuation in
            var completed = false
            func finish(_ value: Data) {
                guard !completed else { return }
                completed = true
                continuation.resume(returning: value)
            }
            Task { @MainActor in
                do {
                    let value = try await web.callAsyncJavaScript(script,
                        arguments: ["encoded": png.base64EncodedString(), "expectedLength": png.count],
                        in: nil, contentWorld: .page)
                    if let value = value as? [String: Any], JSONSerialization.isValidJSONObject(value),
                       let snapshot = try? JSONSerialization.data(withJSONObject: value) {
                        finish(snapshot)
                    } else {
                        finish(Data(#"{"decode":{"status":"host-invalid-result"}}"#.utf8))
                    }
                } catch { finish(Data(#"{"decode":{"status":"host-failure"}}"#.utf8)) }
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 20) {
                finish(Data(#"{"decode":{"status":"host-timeout"}}"#.utf8))
            }
        }
        let result = try XCTUnwrap(JSONSerialization.jsonObject(with: resultData) as? [String: Any])
        let status = (result["decode"] as? [String: Any])?["status"] as? String ?? "host-invalid-result"
        let completed = status == "success" || status == "decoder-error"
        let report: [String: Any] = [
            "diagnosticExecution": completed ? "completed" : status, "result": result,
            "inputSHA256": expected, "inputBytes": png.count,
            "productQualification": false, "renderQualification": false,
            "os": ProcessInfo.processInfo.operatingSystemVersionString,
            "isolation": ["websiteDataStore": "nonPersistent", "javaScriptCanOpenWindowsAutomatically": false,
                          "window": false, "contentRules": false, "cookies": false,
                          "productionDifferences": "isolated blank page, no production window, rules or cookies"]
        ]
        let encoded = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
        var finalDirectory = stat()
        guard lstat(output, &finalDirectory) == 0, finalDirectory.st_dev == directory.st_dev,
              finalDirectory.st_ino == directory.st_ino, finalDirectory.st_uid == geteuid(),
              (finalDirectory.st_mode & 0o7777) == 0o700 else { throw CocoaError(.fileWriteNoPermission) }
        let outFD = openat(directoryFD, "diagnostic.json", O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard outFD >= 0 else { throw CocoaError(.fileWriteFileExists) }
        defer { Darwin.close(outFD) }
        try encoded.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let count = Darwin.write(outFD, raw.baseAddress!.advanced(by: offset), raw.count - offset)
                guard count > 0 else { throw CocoaError(.fileWriteUnknown) }
                offset += count
            }
        }
        XCTAssertTrue(completed, "Diagnostic capture did not complete: \(status)")
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
        let multimap = try ViewportResourcePolicy.encodedRules(origin: origin, tenantId: tenant,
            workspaceMaterials: true, workspaceMaterialMultimap: true)
        let multimapId = "ExhibitOSWorkspaceMultimap-" + UUID().uuidString
        let compiledMultimap = try await store.compileContentRuleList(forIdentifier: multimapId, encodedContentRuleList: multimap)
        XCTAssertNotNil(compiledMultimap)
        try await store.removeContentRuleList(forIdentifier: multimapId)
        let multimapExtra = try rules(multimap).subtracting(rules(scoped))
        XCTAssertEqual(multimapExtra, Set(added.filter { $0.hasSuffix("/maps/color$") }.flatMap { color in
            ["normal", "roughness"].map { color.replacingOccurrences(of: "/maps/color$", with: "/maps/\($0)$") }
        }))
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
