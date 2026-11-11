// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeFurnishingThumbnailRendererTests: XCTestCase {
    private let png = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="

    private func fixture() throws -> (ViewportFrame, WorkspaceFurnishing, ViewportFurnishingThumbnailRequest) {
        let id = UUID().uuidString.lowercased(), sha = String(repeating: "a", count: 64)
        let descriptor = Data("""
        {"id":"\(id)","version":1,"kind":"bench","name":"Synthetic","provenance":"Synthetic",
         "rights":{"holder":"Fixture","ownership":"owner","licenseId":"CC0-1.0","creditLine":"",
         "permissions":{"display":true,"download":false,"export":false,"commercial":false}},
         "model":{"mime":"model/gltf-binary","bytes":128,"sha256":"\(sha)"}}
        """.utf8)
        let furnishing = try JSONDecoder().decode(WorkspaceFurnishing.self, from: descriptor)
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1,
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(),
            draft: try NativeExhibitionDraft.new(), selection: nil, mode: "translate", view: "isometric",
            disabled: false, workspaceId: UUID().uuidString.lowercased(), baseETag: "test-etag")
        return (frame, furnishing, try ViewportFurnishingThumbnailRequest(frame: frame, furnishing: furnishing))
    }

    private func fields(_ request: ViewportFurnishingThumbnailRequest) throws -> [String: Any] {
        var result = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as? [String: Any])
        let data = try XCTUnwrap(Data(base64Encoded: png))
        result["pngBase64"] = png; result["pngSha256"] = nativeImportSHA256(data)
        result["width"] = 256; result["height"] = 256
        return result
    }

    func testReplyBindsSourceFrameAndRequiresExactShape() throws {
        let (_, _, request) = try fixture()
        let valid = try fields(request)
        let result = try NativeFurnishingThumbnailRenderer.checkedReply(valid, request: request)
        XCTAssertEqual(result.pngData, Data(base64Encoded: png))
        XCTAssertEqual(result.pngSha256, nativeImportSHA256(result.pngData))
        let mutations: [(String, Any)] = [("epoch", UUID().uuidString), ("sequence", 2),
            ("tenantId", UUID().uuidString), ("principalId", UUID().uuidString),
            ("requestId", UUID().uuidString), ("furnishingId", UUID().uuidString),
            ("furnishingVersion", 2), ("modelSha256", String(repeating: "b", count: 64)),
            ("modelBytes", 129), ("recipeId", "other"), ("toolId", "other"),
            ("pngSha256", String(repeating: "c", count: 64)), ("width", 255)]
        for (key, value) in mutations {
            var changed = valid; changed[key] = value
            XCTAssertThrowsError(try NativeFurnishingThumbnailRenderer.checkedReply(changed, request: request), key)
        }
        var extra = valid; extra["unknown"] = true
        XCTAssertThrowsError(try NativeFurnishingThumbnailRenderer.checkedReply(extra, request: request))
        var missing = valid; missing.removeValue(forKey: "modelBytes")
        XCTAssertThrowsError(try NativeFurnishingThumbnailRenderer.checkedReply(missing, request: request))
    }

    func testCancelledRendererCannotStartOrBeReused() async throws {
        let (frame, furnishing, _) = try fixture()
        let credentials = try JSONDecoder().decode(LocalAuthoringSession.self, from: Data("""
            {"tenantId":"\(frame.tenantId)","principalId":"\(frame.principalId)","origin":"http://127.0.0.1:7777","token":"\(String(repeating: "a", count: 43))"}
            """.utf8))
        let renderer = NativeFurnishingThumbnailRenderer()
        renderer.cancel()
        do { _ = try await renderer.render(credentials: credentials, frame: frame, furnishing: furnishing); XCTFail("cancelled host started") } catch { }
        do { _ = try await renderer.render(credentials: credentials, frame: frame, furnishing: furnishing); XCTFail("host was reused") } catch { }
    }

    func testAcceptanceRequiresFurnishingCapabilityAndOrdinaryFrame() throws {
        func acceptance(_ json: String) throws -> ViewportAcceptance {
            try JSONDecoder().decode(ViewportAcceptance.self, from: Data(json.utf8))
        }
        XCTAssertTrue(try acceptance("""
            {"accepted":true,"boundedReadOnly":false,"capabilities":["furnishing-registration-thumbnail-v1"]}
            """).supportsFurnishingThumbnail)
        XCTAssertFalse(try acceptance("""
            {"accepted":true,"boundedReadOnly":false,"capabilities":["artwork-thumbnail-v1"]}
            """).supportsFurnishingThumbnail)
        XCTAssertFalse(try acceptance("""
            {"accepted":true,"boundedReadOnly":true,"capabilities":["furnishing-registration-thumbnail-v1"]}
            """).supportsFurnishingThumbnail)
    }

    func testPolicyAllowsOnlyBoundTenantFurnishingReads() throws {
        let tenant = UUID().uuidString.lowercased(), furnishing = UUID().uuidString.lowercased()
        let origin = "http://127.0.0.1:13200"
        let encoded = try NativeFurnishingThumbnailRenderer.resourceRules(origin: origin, tenantId: tenant)
        let rules = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [[String: Any]])
        let filters = try rules.dropFirst().map { rule in
            try XCTUnwrap((rule["trigger"] as? [String: Any])?["url-filter"] as? String)
        }
        func allows(_ url: String) -> Bool { filters.contains { url.range(of: $0, options: .regularExpression) != nil } }
        let path = "/api/v1/tenants/\(tenant)/workspace/furnishings/\(furnishing)/versions/1"
        XCTAssertTrue(allows(origin + path + "/display-check"))
        XCTAssertTrue(allows(origin + path + "/model"))
        XCTAssertFalse(allows(origin + path + "/complete"))
        XCTAssertFalse(allows(origin + path.replacingOccurrences(of: tenant, with: UUID().uuidString.lowercased()) + "/model"))
        XCTAssertFalse(allows("http://localhost:13200" + path + "/model"))
        XCTAssertFalse(allows("https://example.invalid" + path + "/display-check"))
        XCTAssertThrowsError(try NativeFurnishingThumbnailRenderer.resourceRules(origin: "https://example.invalid", tenantId: tenant))
    }
}
