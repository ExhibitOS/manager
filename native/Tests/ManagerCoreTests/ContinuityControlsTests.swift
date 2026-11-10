// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class ContinuityControlsTests: XCTestCase {
    let wall = "11111111-1111-4111-8111-111111111111"
    let floor = "22222222-2222-4222-8222-222222222222"
    let light = "33333333-3333-4333-8333-333333333333"
    let placement = "44444444-4444-4444-8444-444444444444"
    func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let room = draft.rooms[0]["id"]
        candidate["surfaces"] = .array([
            .object(["id": .string(wall), "roomId": room, "type": .string("wall")]),
            .object(["id": .string(floor), "roomId": room, "type": .string("floor")])])
        candidate["lights"] = .array([.object(["id": .string(light), "roomId": room, "type": .string("spot"),
            "unit": .string("candela"), "color": .vector([1, 1, 1]), "intensity": .integer(14),
            "beamAngle": .number(.pi / 3), "transform": NativeExhibitionDraft.identity, "castsShadow": .bool(false)])])
        candidate["placements"] = .array([.object(["id": .string(placement), "roomId": room])])
        candidate["extensions"] = .object(["org.example/opaque": .object(["retain": .integer(19)])])
        try draft.replaceCandidate(candidate); return draft
    }
    func testLightAimRetargetAndPointClearOnlySelectedTargetWithExactDetailsEnvelope() throws {
        let draft = try fixture(); var fields = try LightFormBuffer(draft: draft, selectedLightId: light)
        fields.aimMode = "placement-center"; fields.targetPlacementId = placement; fields.range = "12"; fields.penumbra = "0.35"
        let command = try fields.command(), candidate = try command.proposedCandidate(in: draft, selectedLightId: light)
        XCTAssertTrue(command.usesLightDetails)
        XCTAssertEqual(candidate["lights"].items![0]["targetPlacementId"], .string(placement))
        XCTAssertEqual(candidate["placements"], draft.candidate["placements"])
        XCTAssertEqual(candidate["extensions"]["org.example/opaque"], draft.candidate["extensions"]["org.example/opaque"])
        let binding = candidate["extensions"][NativeLightFixture.namespace]["lights"].items![0]
        XCTAssertEqual(binding["range"], .number(12)); XCTAssertEqual(binding["penumbra"], .number(0.35))
        var applied = draft; try applied.replaceCandidate(candidate)
        fields = try LightFormBuffer(draft: applied, selectedLightId: light)
        XCTAssertFalse(fields.advancedDetailsChanged)
        XCTAssertEqual(try fields.command().proposedCandidate(in: applied, selectedLightId: light), candidate)
        fields.aimMode = "point"; fields.aimY = "1"
        let point = try fields.command().proposedCandidate(in: applied, selectedLightId: light)
        XCTAssertEqual(point["lights"].items![0]["targetPlacementId"], .null)
        XCTAssertEqual(point["extensions"][NativeLightFixture.namespace]["lights"].items![0]["aim"], .object(["kind": .string("point"), "position": .vector([0, 1, 0])]))
        fields.range = ""; fields.penumbra = ""
        let cleared = try fields.command().proposedCandidate(in: applied, selectedLightId: light)
        XCTAssertEqual(cleared["extensions"][NativeLightFixture.namespace]["lights"].items![0]["range"], .null)
        XCTAssertEqual(cleared["extensions"][NativeLightFixture.namespace]["lights"].items![0]["penumbra"], .null)
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(applied))
        XCTAssertEqual(reopened.candidate, candidate)
    }
    func testLightDetailsRejectForeignRoomStaleSelectionAndInvalidRangesWithoutErasingInput() throws {
        var draft = try fixture(), candidate = draft.candidate
        var placements = candidate["placements"].items!; placements[0]["roomId"] = .string(UUID().uuidString.lowercased()); candidate["placements"] = .array(placements)
        try draft.replaceCandidate(candidate)
        var fields = try LightFormBuffer(draft: draft, selectedLightId: light)
        fields.aimMode = "placement-center"; fields.targetPlacementId = placement
        XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedLightId: light))
        fields.aimMode = "point"; fields.aimY = "-"
        XCTAssertThrowsError(try fields.command()); XCTAssertEqual(fields.aimY, "-")
        fields.aimY = "1"; fields.range = "50.01"
        XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedLightId: light))
        fields.range = "12"; fields.penumbra = "1.01"
        XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedLightId: light))
        fields.penumbra = ".35"
        XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedLightId: UUID().uuidString))
        XCTAssertEqual(draft.candidate, candidate)
    }
    func testWallRequestBindsSelectedSurfaceAndRejectsStaleOrUnrelatedCandidate() throws {
        let draft = try fixture()
        let settings = try WallFinishSettings(surfaceId: wall, depth: 0.12, preset: "dark-edge", trimWidth: 0.04, baseboardHeight: 0.1, ceilingBand: true)
        let command = try WallFinishCommand(surfaceId: wall, settings: settings)
        let frame = ViewportFrame(epoch: UUID().uuidString, sequence: 7, tenantId: UUID().uuidString, principalId: UUID().uuidString,
            draft: draft, selection: .init(kind: "surface", id: wall), mode: "translate", view: "isometric", disabled: false)
        let request = try WallFinishViewportRequest(frame: frame, command: command)
        let candidate = try command.proposedCandidate(in: draft, selectedSurfaceId: wall)
        XCTAssertEqual(candidate["surfaces"], draft.candidate["surfaces"])
        XCTAssertEqual(candidate["placements"], draft.candidate["placements"])
        XCTAssertEqual(candidate["extensions"]["org.example/opaque"], draft.candidate["extensions"]["org.example/opaque"])
        var response: DocumentValue = .object(["version": request.value["version"], "epoch": request.value["epoch"], "sequence": request.value["sequence"],
            "tenantId": request.value["tenantId"], "principalId": request.value["principalId"], "requestId": request.value["requestId"],
            "surfaceId": .string(wall), "candidate": candidate])
        let preview = try request.checkedReply(response)
        XCTAssertEqual(try preview.apply(to: draft, command: command, selectedSurfaceId: wall).candidate, candidate)
        XCTAssertThrowsError(try preview.apply(to: draft, command: command, selectedSurfaceId: floor))
        response["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try request.checkedReply(response))
        response["candidate"] = candidate; response["requestId"] = .string(UUID().uuidString)
        XCTAssertThrowsError(try request.checkedReply(response))
        XCTAssertThrowsError(try command.proposedCandidate(in: draft, selectedSurfaceId: floor))
    }
    func testWallRemovalPreservesOtherBindingsAndUnsupportedInputRefuses() throws {
        var draft = try fixture(); var fields = try WallFinishFormBuffer(draft: draft, surfaceId: wall)
        fields.enabled = true; fields.depth = "-"
        XCTAssertThrowsError(try fields.command()); XCTAssertEqual(fields.depth, "-")
        fields.depth = ".12"; let command = try fields.command()
        try draft.replaceCandidate(command.proposedCandidate(in: draft, selectedSurfaceId: wall))
        let removal = try WallFinishCommand(surfaceId: wall, settings: nil)
        let next = try removal.proposedCandidate(in: draft, selectedSurfaceId: wall)
        XCTAssertEqual(next["extensions"][WallFinishSettings.namespace]["walls"], .array([]))
        XCTAssertEqual(next["lights"], draft.candidate["lights"])
        var future = draft.candidate; future["extensions"][WallFinishSettings.namespace]["future"] = .bool(true)
        try draft.replaceCandidate(future)
        XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedSurfaceId: wall))
        XCTAssertEqual(draft.candidate, future)
    }
    func testConcreteAtomicallyDeclaresFamilyWithoutChangingAppearanceOrOtherExperience() throws {
        let base = try fixture(); let target = FloorSettings.Target(kind: .surface, id: floor)
        let candidate = try ConcreteFootsteps.proposedCandidate(in: base, target: target, resourcesAvailable: true)
        XCTAssertEqual(candidate["surfaces"], base.candidate["surfaces"])
        XCTAssertEqual(candidate["extensions"][FloorSettings.namespace], .null)
        XCTAssertEqual(candidate["extensions"][ConcreteFootsteps.resourceNamespace], .object(["version": .integer(1), "families": .array([.string("concrete")])]))
        XCTAssertEqual(candidate["extensions"][ConcreteFootsteps.experienceNamespace]["footsteps"], .array([.object(["surfaceId": .string(floor), "material": .string("concrete")])]))
        var changed = base; try changed.replaceCandidate(candidate)
        var withOtherExperience = candidate
        withOtherExperience["extensions"][ConcreteFootsteps.experienceNamespace]["rooms"] = .array([.object(["roomId": base.rooms[0]["id"], "reverb": .number(0.2)])])
        withOtherExperience["extensions"][ConcreteFootsteps.experienceNamespace]["audioDefaults"] = .object(["soundOn": .bool(true)])
        try changed.replaceCandidate(withOtherExperience)
        let repeated = try ConcreteFootsteps.proposedCandidate(in: changed, target: target, resourcesAvailable: true)
        XCTAssertEqual(repeated, withOtherExperience, "No duplicate family, floor entry or extra gain")
        XCTAssertEqual(base.candidate["extensions"][ConcreteFootsteps.experienceNamespace], .null)
    }
    func testConcretePreservesAppearanceOnlyFloorBindingWithAbsentSound() throws {
        var draft = try fixture(); let target = FloorSettings.Target(kind: .surface, id: floor)
        let settings = try FloorSettings.applying(preset: .wood, defaults: FloorSettings.defaults(.wood), fields: [.texturePack], target: target, prior: nil, candidate: draft.candidate)
        XCTAssertEqual(settings.value["sound"], .null)
        try draft.setFloor(settings, target: target)
        let before = draft.candidate
        let next = try ConcreteFootsteps.proposedCandidate(in: draft, target: target, resourcesAvailable: true)
        XCTAssertEqual(next["extensions"][FloorSettings.namespace], before["extensions"][FloorSettings.namespace])
        XCTAssertEqual(next["extensions"][ConcreteFootsteps.experienceNamespace]["footsteps"].items?[0]["material"], .string("concrete"))
        XCTAssertEqual(draft.candidate, before)
    }
    func testConcreteRefusesRuntimeLossCompetingFloorAndMalformedFamilyWithoutMutation() throws {
        var draft = try fixture(); let target = FloorSettings.Target(kind: .surface, id: floor)
        XCTAssertThrowsError(try ConcreteFootsteps.proposedCandidate(in: draft, target: target, resourcesAvailable: false))
        XCTAssertThrowsError(try ConcreteFootsteps.proposedCandidate(in: draft, target: .init(kind: .roomBase, id: draft.rooms[0]["id"].string!), resourcesAvailable: true))
        let settings = try FloorSettings.applying(preset: .wood, defaults: FloorSettings.defaults(.wood), fields: Set(FloorSettings.Field.allCases), target: target, prior: nil, candidate: draft.candidate)
        try draft.setFloor(settings, target: target); let saved = draft
        XCTAssertThrowsError(try ConcreteFootsteps.proposedCandidate(in: draft, target: target, resourcesAvailable: true)); XCTAssertEqual(draft, saved)
        draft = try fixture(); var candidate = draft.candidate
        candidate["extensions"][ConcreteFootsteps.resourceNamespace] = .object(["version": .integer(1), "families": .array([.string("wood"), .string("concrete")])])
        try draft.replaceCandidate(candidate)
        XCTAssertThrowsError(try ConcreteFootsteps.proposedCandidate(in: draft, target: target, resourcesAvailable: true)); XCTAssertEqual(draft.candidate, candidate)
        XCTAssertTrue(NativeFloorAudioRuntime.supports(manifestSHA256: NativeFloorAudioRuntime.personalManifestSHA256, ready: true))
        XCTAssertFalse(NativeFloorAudioRuntime.supports(manifestSHA256: NativeFloorAudioRuntime.personalManifestSHA256, ready: false))
    }
}
