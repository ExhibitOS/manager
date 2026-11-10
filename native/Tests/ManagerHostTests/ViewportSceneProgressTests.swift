// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerApp

final class ViewportSceneProgressTests: XCTestCase {
    func testShowsOnlyMeasuredArtworkStageAfterSceneIsReady() {
        XCTAssertEqual(ViewportSceneProgress.reported(ready: false, failed: false,
            loadedCount: 0, failedCount: 0, total: 4), .preparing)
        XCTAssertEqual(ViewportSceneProgress.reported(ready: true, failed: false,
            loadedCount: 1, failedCount: 1, total: 4), .artworks(processed: 2, total: 4))
        XCTAssertNil(ViewportSceneProgress.reported(ready: true, failed: false,
            loadedCount: 3, failedCount: 1, total: 4))
        XCTAssertNil(ViewportSceneProgress.reported(ready: true, failed: false,
            loadedCount: 0, failedCount: 0, total: 0))
    }

    func testFailedSceneAndInvalidCountsCannotPresentCompletion() {
        XCTAssertNil(ViewportSceneProgress.reported(ready: false, failed: true,
            loadedCount: 0, failedCount: 0, total: 4))
        XCTAssertEqual(ViewportSceneProgress.reported(ready: true, failed: false,
            loadedCount: 4, failedCount: 1, total: 4), .preparing)
    }
}
