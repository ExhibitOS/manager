// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeThumbnailPreviewTraceTests: XCTestCase {
    private let id = "09a8fdc8-0b72-4db7-85ea-ec9098a01ea3"

    func testOptInNeedsExactV2AndWritableAcceptance() throws {
        func acceptance(_ capabilities: [String], bounded: Bool = false) throws -> ViewportAcceptance {
            let raw = try JSONSerialization.data(withJSONObject: [
                "accepted": true, "boundedReadOnly": bounded, "capabilities": capabilities,
            ])
            return try JSONDecoder().decode(ViewportAcceptance.self, from: raw)
        }
        let rendered = ["artwork-thumbnail-v1", "rendered-display-preview-v1"]
        let v2 = "artwork-thumbnail-outcome-v2"
        let trace = "artwork-thumbnail-preview-trace-v1"
        XCTAssertFalse(try acceptance(rendered + [v2]).supportsArtworkThumbnailPreviewTrace)
        XCTAssertFalse(try acceptance(rendered + [trace]).supportsArtworkThumbnailPreviewTrace)
        XCTAssertFalse(try acceptance(rendered + [v2, trace], bounded: true).supportsArtworkThumbnailPreviewTrace)
        XCTAssertTrue(try acceptance(rendered + [v2, trace]).supportsArtworkThumbnailPreviewTrace)
    }

    func testExactIDShapeAndClosedBoundedWebTrace() throws {
        XCTAssertTrue(NativeThumbnailPreviewTrace.validRequestID(id))
        XCTAssertFalse(NativeThumbnailPreviewTrace.validRequestID(id.uppercased()))
        XCTAssertFalse(NativeThumbnailPreviewTrace.validRequestID("09a8fdc8-0b72-0db7-85ea-ec9098a01ea3"))

        func sidecar(_ terminal: [String: Any]) -> [String: Any] {
            ["version": 1, "requestId": id, "anchorUnixMs": 123,
             "dispatchElapsedMs": 10, "remainingBudgetEstimateMs": 9990, "terminal": terminal]
        }
        let headers: [String: Any] = ["kind": "headers", "elapsedMs": 20, "status": 200]
        let raw = sidecar(headers)
        XCTAssertEqual(NativeThumbnailPreviewTrace.webTrace(raw, requestID: id)?["terminal"]["status"], .integer(200))
        XCTAssertNotNil(NativeThumbnailPreviewTrace.webTrace(
            sidecar(["kind": "cancelled", "elapsedMs": 20, "reason": "timeout"]), requestID: id))
        XCTAssertNotNil(NativeThumbnailPreviewTrace.webTrace(sidecar(["kind": "failed", "elapsedMs": 20]), requestID: id))
        XCTAssertNil(NativeThumbnailPreviewTrace.webTrace(raw, requestID: UUID().uuidString.lowercased()))
        for invalid in [
            ["kind": "headers", "elapsedMs": 20, "status": 600],
            ["kind": "headers", "elapsedMs": 20],
            ["kind": "failed", "elapsedMs": 9],
            ["kind": "cancelled", "elapsedMs": 20, "reason": "raw error"],
            ["kind": "headers", "elapsedMs": 20, "status": 200, "url": "private"],
        ] as [[String: Any]] {
            XCTAssertNil(NativeThumbnailPreviewTrace.webTrace(sidecar(invalid), requestID: id))
        }
        var malformed = raw
        malformed["version"] = true
        XCTAssertNil(NativeThumbnailPreviewTrace.webTrace(malformed, requestID: id))
        malformed = raw
        malformed["remainingBudgetEstimateMs"] = 10_001
        XCTAssertNil(NativeThumbnailPreviewTrace.webTrace(malformed, requestID: id))
    }
}
