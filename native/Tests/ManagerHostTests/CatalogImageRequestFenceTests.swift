// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerApp

@MainActor
final class CatalogImageRequestFenceTests: XCTestCase {
    func testDelayedUnselectedRendererCannotRefillAfterSelectionSave() async {
        let fence = CatalogImageRequestFence()
        let revision = "synthetic-artwork:2", other = "other-artwork:2"
        let old = fence.generation(for: revision)
        var release: CheckedContinuation<Void, Never>?
        let delayedRenderer = Task { @MainActor in
            await withCheckedContinuation { release = $0 }
            return fence.isCurrent(old, for: revision) ? "old-rendered-image" : nil
        }
        while release == nil { await Task.yield() }
        // The selected binding changes while the original artwork revision stays 2.
        fence.invalidate(revision)
        let selectedRead = fence.generation(for: revision)
        release?.resume()
        let obsolete = await delayedRenderer.value
        XCTAssertNil(obsolete, "An earlier nil-selection renderer cannot repopulate the cleared cache")
        XCTAssertTrue(fence.isCurrent(selectedRead, for: revision), "The queued selected-PNG read can fill the cache")
        XCTAssertTrue(fence.isCurrent(0, for: other), "Other cards keep their cache and in-flight requests")
        fence.invalidate(revision)
        XCTAssertFalse(fence.isCurrent(selectedRead, for: revision), "A second selection invalidates an in-flight body read too")
    }
}
