// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class PlacedExitDoorSettingsTests: XCTestCase {
    func testOnlyThePreparedDoorOnItsWallGetsIndependentDestinations() throws {
        func id() -> String { UUID().uuidString.lowercased() }
        func vector(_ values: Double...) -> DocumentValue { .array(values.map(DocumentValue.number)) }
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let wall = id(), door = id(), sign = id(), barrier = id(), passive = id(), offWall = id()
        func furnishing(_ id: String, kind: String, hash: String) -> DocumentValue {
            .object(["id": .string(id), "version": .integer(1), "kind": .string(kind),
                     "model": .object(["bytes": .integer(8684), "mime": .string("model/gltf-binary"), "sha256": .string(hash)])])
        }
        func placement(_ id: String, furnishing: String, z: Double = 4) -> DocumentValue {
            .object(["id": .string(id), "roomId": .string(room), "furnishingId": .string(furnishing),
                     "furnishingVersion": .integer(1), "position": vector(0, 0, z), "yaw": .integer(0), "scale": .integer(1)])
        }
        let audio: DocumentValue = .array([.object(["roomId": .string(room), "reverb": .number(0.2), "bgm": .null])])
        let other: DocumentValue = .object(["witness": .bool(true)])
        var candidate = draft.candidate
        candidate["surfaces"] = .array([.object(["id": .string(wall), "roomId": .string(room), "type": .string("wall"),
            "dimensions": .object(["width": .integer(8), "height": .integer(4)]),
            "transform": .object(["position": vector(0, 2, 4), "scale": vector(1, 1, 1), "rotation": vector(0, 0, 0, 1)])])])
        candidate["extensions"] = .object([
            WorkspaceFurnishingPlacement.namespace: .object([
                "furnishings": .array([furnishing(barrier, kind: "barrier", hash: PlacedExitDoorSettings.modelSHA256),
                                       furnishing(passive, kind: "info-stand", hash: PlacedExitDoorSettings.modelSHA256)]),
                "placements": .array([placement(door, furnishing: barrier), placement(sign, furnishing: passive),
                                       placement(offWall, furnishing: barrier, z: 3.5)])]),
            ConcreteFootsteps.experienceNamespace: .object([
                "version": .integer(1), "footsteps": .array([]), "voices": .array([]), "annotations": .array([]),
                "rooms": audio, "translations": .array([]), "audioDefaults": .object(["soundOn": .bool(false)])]),
            "test/untouched": other
        ])
        try draft.replaceCandidate(candidate)
        XCTAssertEqual(PlacedExitDoorSettings.targets(in: draft).map(\.id), [door])
        let primary = try PlacedExitDoorSettings.Destination(name: " ExhibitOS ", url: "https://EXHIBITOS.org:443")
        XCTAssertEqual(primary.name, "ExhibitOS")
        XCTAssertEqual(primary.url, "https://exhibitos.org/")
        let row = try PlacedExitDoorSettings(placementId: door, surfaceId: wall, primary: primary, additional: [])
        try draft.replaceCandidate(row.candidate(in: draft))
        XCTAssertEqual(try PlacedExitDoorSettings.read(in: draft, placementId: door), row)
        XCTAssertEqual(draft.candidate["extensions"][ConcreteFootsteps.experienceNamespace]["rooms"], audio)
        XCTAssertEqual(draft.candidate["extensions"]["test/untouched"], other)
        XCTAssertThrowsError(try PlacedExitDoorSettings(placementId: sign, surfaceId: wall, primary: primary, additional: []).candidate(in: draft))
        XCTAssertThrowsError(try PlacedExitDoorSettings(placementId: door, surfaceId: wall, primary: primary, additional: [primary]))
        XCTAssertThrowsError(try PlacedExitDoorSettings.Destination(name: "Credential", url: "https://owner@example.org/"))
        XCTAssertThrowsError(try PlacedExitDoorSettings.Destination(name: "Plaintext", url: "http://example.org/"))
    }
}
