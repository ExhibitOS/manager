// SPDX-License-Identifier: Apache-2.0
#if DEBUG
import XCTest
@testable import ManagerCore

final class NativeGPUDecodeObservationTests: XCTestCase {
    private func call() -> [String: Any] {
        ["version": 1, "phase": "call", "callId": 0, "imageIndex": 2,
         "bytes": 4, "byteOffset": 2, "bufferBytes": 6, "blobBytes": 4,
         "blobType": "image/png", "disposed": false, "visibility": "visible",
         "hasFocus": true, "secureContext": false, "nativeViewportPath": true]
    }
    func testCallRejectsUnexpectedTypesAndBounds() {
        let changes: [(String, Any)] = [("version", true), ("callId", 0.5), ("imageIndex", 32),
            ("bytes", -1), ("bytes", 67_108_865), ("byteOffset", 3), ("blobBytes", 3),
            ("blobType", "image/jpeg"), ("disposed", 0), ("visibility", "other"),
            ("nativeViewportPath", 1), ("extra", "unexpected")]
        var limiter = NativeGPUDecodeObservationLimiter()
        for (key, value) in changes {
            var object = call(); object[key] = value
            XCTAssertNil(limiter.accept(object), key)
        }
        var missing = call(); missing.removeValue(forKey: "bytes")
        XCTAssertNil(limiter.accept(missing))
        XCTAssertEqual(limiter.accepted, 0)
        XCTAssertNotNil(limiter.accept(call()))
        XCTAssertEqual(limiter.accepted, 1)
    }
    func testOutcomeShapesAndHashAreClosed() {
        var limiter = NativeGPUDecodeObservationLimiter()
        let common: [String: Any] = ["version": 1, "phase": "settled", "callId": 1, "imageIndex": 2]
        var settled = common; settled["outcome"] = "success"; settled["disposed"] = false
        XCTAssertNotNil(limiter.accept(settled))
        settled["errorName"] = "Error"; XCTAssertNil(limiter.accept(settled))
        settled["outcome"] = "decoder-error"; XCTAssertNotNil(limiter.accept(settled))
        settled["errorName"] = "private error"; XCTAssertNil(limiter.accept(settled))
        settled.removeValue(forKey: "errorName"); XCTAssertNil(limiter.accept(settled))
        var hash = common; hash["phase"] = "hash"; hash["outcome"] = "success"
        hash["sha256"] = String(repeating: "a", count: 64); XCTAssertNotNil(limiter.accept(hash))
        hash["sha256"] = String(repeating: "A", count: 64); XCTAssertNil(limiter.accept(hash))
        hash["outcome"] = "observation-failed"; XCTAssertNil(limiter.accept(hash))
        hash.removeValue(forKey: "sha256"); XCTAssertNotNil(limiter.accept(hash))
    }
    func testOnlyAcceptedRecordsConsumeLimitAndOversizedBodyRejected() {
        var limiter = NativeGPUDecodeObservationLimiter()
        var oversized = call(); oversized["unknown"] = String(repeating: "a", count: 4096)
        XCTAssertNil(limiter.accept(oversized))
        for _ in 0..<12 { XCTAssertNotNil(limiter.accept(call())) }
        XCTAssertEqual(limiter.accepted, 12)
        XCTAssertNil(limiter.accept(call()))
    }
}
#endif
