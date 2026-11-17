// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class NativeOEXAutomationFailureTests: XCTestCase {
    func testFailureReportsOnlyExistingProgressAndWhitelistedTransportFields() throws {
        let observation = NativeOEXAutomationObservation()
        observation.update(.receiving(bytes: 262144, total: 1000000))
        let id = "10000000-0000-4000-8000-000000000001"
        let error = NSError(domain: NSURLErrorDomain, code: -1005, userInfo: [NSLocalizedDescriptionKey: "private-token-and-url"])
        let value = NativeAutomationFailure.value(observation.failure(error, operationId: id))
        XCTAssertEqual(value["code"], .string("AUTOMATION_OEX_EXPORT_FAILED"))
        XCTAssertEqual(value["retryAllowed"], .bool(false))
        XCTAssertEqual(value["oexFailure"], .object(["phase": .string("receiving"), "errorDomain": .string("url"),
            "errorCode": .integer(-1005), "operationId": .string(id), "receivedBytes": .integer(262144), "totalBytes": .integer(1000000)]))
        let encoded = try JSONEncoder().encode(value)
        XCTAssertFalse(String(decoding: encoded, as: UTF8.self).contains("private-token-and-url"))
    }

    func testUnknownErrorsAndInvalidIdentityDoNotExposeArbitraryDetails() throws {
        let failure = NativeOEXAutomationFailure(NSError(domain: "private-domain", code: 123456,
            userInfo: ["cookie": "secret"]), phase: .requesting, operationId: "not-an-id", receivedBytes: -1, totalBytes: Int.max)
        XCTAssertEqual(NativeAutomationFailure.value(failure)["oexFailure"],
                       .object(["phase": .string("requesting"), "errorDomain": .string("other")]))
        let observation = NativeOEXAutomationObservation()
        observation.update(.checkingPackage)
        XCTAssertEqual(observation.phase, .checkingPackage)
        observation.update(.verifying)
        XCTAssertEqual(observation.phase, .verifying)
        observation.writing()
        XCTAssertEqual(observation.phase, .writing)
    }
}
