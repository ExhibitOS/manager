// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeFurnishingThumbnailRulesTests: XCTestCase {
    func testDisplayPermissionAndBothTimeBoundariesAreRequired() throws {
        let now = try XCTUnwrap(ISO8601DateFormatter().date(from: "2026-11-11T12:00:00Z"))
        var rights = ArtworkMetadata.Rights()
        XCTAssertNoThrow(try NativeFurnishingThumbnailRules.requireDisplay(rights, now: now))
        rights.permissions.display = false
        XCTAssertThrowsError(try NativeFurnishingThumbnailRules.requireDisplay(rights, now: now))
        rights.permissions.display = true
        rights.validFrom = "2026-11-11T12:00:00.001Z"
        XCTAssertThrowsError(try NativeFurnishingThumbnailRules.requireDisplay(rights, now: now))
        rights.validFrom = "2026-11-11T12:00:00Z"
        rights.expiresAt = "2026-11-11T12:00:00Z"
        XCTAssertThrowsError(try NativeFurnishingThumbnailRules.requireDisplay(rights, now: now))
        rights.expiresAt = "2026-11-11T12:00:00.001Z"
        XCTAssertNoThrow(try NativeFurnishingThumbnailRules.requireDisplay(rights, now: now))
        rights.validFrom = "invalid"
        XCTAssertThrowsError(try NativeFurnishingThumbnailRules.requireDisplay(rights, now: now))
        rights.validFrom = nil; rights.expiresAt = "invalid"
        XCTAssertThrowsError(try NativeFurnishingThumbnailRules.requireDisplay(rights, now: now))
    }
}
