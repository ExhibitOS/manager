// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewportCameraInteractionTests: XCTestCase {
    private func frame(sequence: Int = 19, disabled: Bool = false,
                       presentation: ViewportPresentation = .threeD) throws -> ViewportFrame {
        ViewportFrame(epoch: "11111111-1111-4111-8111-111111111111", sequence: sequence,
            tenantId: "22222222-2222-4222-8222-222222222222", principalId: "33333333-3333-4333-8333-333333333333",
            draft: try NativeExhibitionDraft.new(), selection: nil, mode: "rotate", view: "isometric",
            disabled: disabled, cameraSequence: 4, presentation: presentation)
    }
    private func reply(_ request: ViewportCameraInteractionRequest) -> DocumentValue {
        .object(["epoch": .string(request.epoch), "sequence": .integer(Int64(request.sequence)),
                 "requestId": .string(request.requestId), "mode": .string(request.mode.rawValue), "accepted": .bool(true)])
    }
    func testClosedExistingWireUsesFrameSequenceNotCameraSequenceOrObjectRotation() throws {
        let request = try ViewportCameraInteractionRequest(frame: frame(), mode: .cameraRotate)
        let value = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request))
        guard case .object(let fields) = value else { return XCTFail("object required") }
        XCTAssertEqual(Set(fields.keys), ["epoch", "sequence", "requestId", "mode"])
        XCTAssertEqual(value["sequence"], .integer(19))
        XCTAssertEqual(value["mode"], .string("camera-rotate"))
        XCTAssertTrue(request.matches(reply(request)))
    }
    func testEveryMappingAckIdentityAndExactBooleanIsRequired() throws {
        let request = try ViewportCameraInteractionRequest(frame: frame(), mode: .cameraRotate)
        for (key, wrong) in [("epoch", DocumentValue.string(UUID().uuidString.lowercased())),
                             ("sequence", .integer(4)), ("requestId", .string(UUID().uuidString.lowercased())),
                             ("mode", .string("object")), ("accepted", .bool(false)), ("accepted", .integer(1))] {
            var value = reply(request); value[key] = wrong
            XCTAssertFalse(request.matches(value), key)
        }
        var extra = reply(request); extra["frameToken"] = .string("not-on-wire")
        XCTAssertFalse(request.matches(extra))
        guard case .object(var fields) = reply(request) else { return XCTFail("object required") }
        fields.removeValue(forKey: "requestId"); let missing = DocumentValue.object(fields)
        XCTAssertFalse(request.matches(missing))
    }
    func testPlanDisabledInvalidSequenceAndRequestIdentityAreRejected() throws {
        XCTAssertThrowsError(try ViewportCameraInteractionRequest(frame: frame(disabled: true), mode: .cameraRotate))
        XCTAssertThrowsError(try ViewportCameraInteractionRequest(frame: frame(presentation: .plan), mode: .cameraRotate))
        for sequence in [0, -1, 9_007_199_254_740_992] {
            XCTAssertThrowsError(try ViewportCameraInteractionRequest(frame: frame(sequence: sequence), mode: .cameraRotate))
        }
        XCTAssertThrowsError(try ViewportCameraInteractionRequest(frame: frame(), mode: .object, requestId: "bad"))
    }
    func testLegacyOrCapabilityOnlyAcceptanceCannotEnableCameraMode() throws {
        func accepted(_ value: String) throws -> ViewportAcceptance {
            try JSONDecoder().decode(ViewportAcceptance.self, from: Data(value.utf8))
        }
        XCTAssertFalse(try accepted("{\"accepted\":true}").supportsCameraInteraction)
        XCTAssertFalse(try accepted("{\"accepted\":true,\"capabilities\":[\"cameraInteractionV1\"]}").supportsCameraInteraction)
        XCTAssertFalse(try accepted("{\"accepted\":false,\"capabilities\":[\"cameraInteractionV1\"],\"cameraInteractionMode\":\"object\"}").supportsCameraInteraction)
        let current = try accepted("{\"accepted\":true,\"capabilities\":[\"cameraInteractionV1\"],\"cameraInteractionMode\":\"camera-rotate\"}")
        XCTAssertTrue(current.supportsCameraInteraction)
        XCTAssertEqual(current.cameraInteractionMode, .cameraRotate)
    }
    func testResetBodyOnlyAcceptsExistingObjectResetReasons() {
        for reason in ["hidden", "unavailable"] {
            XCTAssertTrue(ViewportCameraInteractionReset.valid(.object(["mode": .string("object"), "reason": .string(reason)])))
        }
        XCTAssertFalse(ViewportCameraInteractionReset.valid(.object(["mode": .string("camera-rotate"), "reason": .string("hidden")])))
        XCTAssertFalse(ViewportCameraInteractionReset.valid(.object(["mode": .string("object"), "reason": .string("success")])))
        XCTAssertFalse(ViewportCameraInteractionReset.valid(.object(["mode": .string("object"), "reason": .string("hidden"), "accepted": .bool(true)])))
    }
}
