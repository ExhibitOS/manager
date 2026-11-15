// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeStaticExportWaitTests: XCTestCase {
    func testDetailPreparationCanExceedOldClientTimeoutButRemainsBounded() {
        let started = ContinuousClock().now
        let wait = NativeStaticExportWait(started: started, nativeOriginal: true)
        XCTAssertTrue(wait.allowsPolling(at: started.advanced(by: .seconds(125))))
        XCTAssertTrue(wait.allowsPolling(at: started.advanced(by: .seconds(600))))
        XCTAssertTrue(wait.allowsPolling(at: started.advanced(by: .milliseconds(719_999))))
        XCTAssertFalse(wait.allowsPolling(at: started.advanced(by: .seconds(720))))
        XCTAssertFalse(wait.allowsPolling(at: started.advanced(by: .seconds(721))))
    }

    func testOrdinaryExportRetainsEarlierWallClockBound() {
        let started = ContinuousClock().now
        let wait = NativeStaticExportWait(started: started, nativeOriginal: false)
        XCTAssertTrue(wait.allowsPolling(at: started.advanced(by: .milliseconds(119_999))))
        XCTAssertFalse(wait.allowsPolling(at: started.advanced(by: .seconds(120))))
        XCTAssertFalse(wait.allowsPolling(at: started.advanced(by: .seconds(125))))
    }
}
