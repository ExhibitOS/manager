// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeAutomationCaptionStandsTests: XCTestCase {
    func testCaptionRuntimeKeepsTheExistingOrdinaryCommandAndOutputScope() throws {
        let manifest = AuthoringAPI.r209CaptionStandsOrdinaryManifest
        XCTAssertTrue(AuthoringAPI.supportsR209CaptionStandsOrdinarySuccessor(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsR209CaptionStandsOrdinarySuccessor(manifest: manifest.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        for operation in ["workspace-furnishing-place", "workspace-furnishing-model-check", "save", "reopen-saved", "export-static-saved-head"] {
            XCTAssertNoThrow(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest), operation)
        }
        for operation in ["workspace-info-stand-replace", "workspace-furnishing-intake-create", "workspace-ceiling-rail-insert",
                          "workspace-frame-attach", "export-static-native-original", "export-oex"] {
            XCTAssertThrowsError(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest), operation)
        }
    }

    func testExactApprovedPlacementRejectsChangedFitModelAndIntake() throws {
        let room = "d295d832-8ec0-40b2-8d9d-72d6a1ac3666"
        let item = "a358b4d2-8f6c-5c0f-8421-a06e90624a94"
        let id = "e81bf2a0-7cec-53d3-9bec-884c202faa33"
        let position = [-3.45167856, 0, -2.20167856]
        let yaw = 0.7853981633974483
        let model = NativeAutomationCaptionStands.model
        let approved = WorkspaceFurnishingPlacement(id: id, roomId: room, furnishingId: item,
            furnishingVersion: 1, position: position, yaw: yaw, scale: 1)
        let shifted = WorkspaceFurnishingPlacement(id: id, roomId: room, furnishingId: item,
            furnishingVersion: 1, position: position, yaw: -yaw, scale: 1)
        let unauthorised = WorkspaceFurnishingPlacement(id: item, roomId: room, furnishingId: item,
            furnishingVersion: 1, position: position, yaw: yaw, scale: 1)
        XCTAssertNoThrow(try NativeAutomationCaptionStands.checkPlacement(approved, model: model, intake: nil))
        XCTAssertThrowsError(try NativeAutomationCaptionStands.checkPlacement(shifted, model: model, intake: nil))
        XCTAssertThrowsError(try NativeAutomationCaptionStands.checkPlacement(unauthorised, model: model, intake: nil))
        XCTAssertThrowsError(try NativeAutomationCaptionStands.checkPlacement(approved, model: nil, intake: nil))
        XCTAssertThrowsError(try NativeAutomationCaptionStands.checkPlacement(approved, model: model, intake: item))
    }
}
