// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class OEXScopeContinuityTests: XCTestCase {
    func testOnlyActiveFreshImportDefersScopeInvalidation() {
        XCTAssertFalse(oexScopeInterrupted(ready: true, freshOperation: false, importBusy: false))
        XCTAssertFalse(oexScopeInterrupted(ready: false, freshOperation: true, importBusy: true))
        XCTAssertTrue(oexScopeInterrupted(ready: false, freshOperation: false, importBusy: true))
        XCTAssertTrue(oexScopeInterrupted(ready: false, freshOperation: true, importBusy: false))
        XCTAssertTrue(oexScopeInterrupted(ready: false, freshOperation: false, importBusy: false))
    }
}
