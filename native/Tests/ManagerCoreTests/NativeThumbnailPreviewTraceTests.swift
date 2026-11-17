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
        let valid = NativeThumbnailPreviewTrace.webTrace(raw, requestID: id)
        XCTAssertEqual(valid.disposition, "valid")
        XCTAssertEqual(valid.trace, .object([
            "version": .integer(1), "requestId": .string(id), "anchorUnixMs": .integer(123),
            "dispatchElapsedMs": .integer(10), "remainingBudgetEstimateMs": .integer(9990),
            "terminal": .object(["kind": .string("headers"), "elapsedMs": .integer(20), "status": .integer(200)]),
        ]))
        XCTAssertNotNil(NativeThumbnailPreviewTrace.webTrace(
            sidecar(["kind": "cancelled", "elapsedMs": 20, "reason": "timeout"]), requestID: id).trace)
        XCTAssertNotNil(NativeThumbnailPreviewTrace.webTrace(sidecar(["kind": "failed", "elapsedMs": 20]), requestID: id).trace)
        XCTAssertEqual(NativeThumbnailPreviewTrace.webTrace(raw, requestID: UUID().uuidString.lowercased()), .rejected)
        for invalid in [
            ["kind": "headers", "elapsedMs": 20, "status": 600],
            ["kind": "headers", "elapsedMs": 20],
            ["kind": "failed", "elapsedMs": 9],
            ["kind": "cancelled", "elapsedMs": 20, "reason": "raw error"],
            ["kind": "headers", "elapsedMs": 20, "status": 200, "url": "private"],
        ] as [[String: Any]] {
            XCTAssertEqual(NativeThumbnailPreviewTrace.webTrace(sidecar(invalid), requestID: id), .rejected)
        }
        var malformed = raw
        malformed["version"] = true
        XCTAssertEqual(NativeThumbnailPreviewTrace.webTrace(malformed, requestID: id), .rejected)
        malformed = raw
        malformed["remainingBudgetEstimateMs"] = 10_001
        let rejected = NativeThumbnailPreviewTrace.webTrace(malformed, requestID: id)
        XCTAssertEqual(rejected, .rejected)
        XCTAssertEqual(rejected.disposition, "rejected")
        XCTAssertNil(rejected.trace)
        let rawNull = NativeThumbnailPreviewTrace.webTrace(NSNull(), requestID: id)
        XCTAssertEqual(rawNull, .rawNull)
        XCTAssertEqual(rawNull.disposition, "raw-null")
        XCTAssertNil(rawNull.trace)
        XCTAssertEqual(NativeThumbnailPreviewTrace.webTrace(nil, requestID: id), .rejected)
    }
}
