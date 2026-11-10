// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class RoomAudioSettingsTests: XCTestCase {
    private let trackId = "5fd33e2b-a4ab-480d-986d-d82e17de175a"

    func testRoomEditPreservesOtherExperienceAndDistinguishesNoTrackFromOmission() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let otherRoom = "083db44d-0417-47a1-a294-a1cb4f554d39"
        var candidate = draft.candidate
        var rooms = draft.rooms
        var other = rooms[0]
        other["id"] = .string(otherRoom)
        rooms.append(other)
        candidate["rooms"] = .array(rooms)
        let namespace = ConcreteFootsteps.experienceNamespace
        let otherAudio: DocumentValue = .object(["roomId": .string(otherRoom), "reverb": .number(0.25), "bgmOn": .bool(true)])
        let defaults: DocumentValue = .object(["soundOn": .bool(true)])
        candidate["extensions"] = .object([
            "example.unknown/unchanged": .object(["keep": .bool(true)]),
            namespace: .object([
                "version": .integer(1), "footsteps": .array([]), "voices": .array([]),
                "annotations": .array([]), "rooms": .array([otherAudio]), "translations": .array([]),
                "audioDefaults": defaults,
            ]),
        ])
        try draft.replaceCandidate(candidate)
        XCTAssertEqual(try RoomAudioSettings().proposedCandidate(in: draft, roomId: room, approvedTrack: nil), candidate)
        let settings = try RoomAudioSettings(fields: [
            "reverb": .number(0.35), "footstepsOn": .bool(true), "reverbOn": .bool(false), "bgm": .null,
        ])
        let edited = try settings.proposedCandidate(in: draft, roomId: room, approvedTrack: nil)
        let experience = edited["extensions"][namespace]
        XCTAssertEqual(experience["audioDefaults"], defaults)
        XCTAssertEqual(experience["rooms"].items?.first, otherAudio)
        XCTAssertEqual(edited["extensions"]["example.unknown/unchanged"], candidate["extensions"]["example.unknown/unchanged"])
        try draft.replaceCandidate(edited)
        XCTAssertEqual(try RoomAudioSettings.read(in: draft, roomId: room), settings)
        let inherited = try RoomAudioSettings(fields: ["reverb": .number(0.35), "bgmOn": .bool(false)])
        let next = try inherited.proposedCandidate(in: draft, roomId: room, approvedTrack: nil)
        try draft.replaceCandidate(next)
        XCTAssertEqual(try RoomAudioSettings.read(in: draft, roomId: room), inherited)
        XCTAssertThrowsError(try settings.proposedCandidate(in: draft, roomId: "unknown", approvedTrack: nil))
    }

    func testOnlyCurrentApprovedExactExportableSnapshotCanBeAssigned() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        var media: DocumentValue = .object([
            "id": .string(trackId), "path": .string("media/\(trackId)/audio.wav"), "mime": .string("audio/wav"),
            "bytes": .integer(1200), "sha256": .string(String(repeating: "f", count: 64)),
            "rights": .object(["creditLine": .string("Fixture WAV"),
                               "permissions": .object(["display": .bool(true), "export": .bool(true)])]),
        ])
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        func entry(state: String = "approved", revoked: Bool = false) -> ApprovedRoomAudioTrack.CatalogItem {
            .init(id: trackId, state: state, revision: 1, revoked: revoked, mediaAsset: media)
        }
        XCTAssertNil(entry(state: "validated").eligible(at: now))
        XCTAssertNil(entry(revoked: true).eligible(at: now))
        let approved = try XCTUnwrap(entry().eligible(at: now))
        media["rights"]["permissions"]["export"] = .bool(false)
        XCTAssertNil(entry().eligible(at: now))
        media["rights"]["permissions"]["export"] = .bool(true)
        media["rights"]["expiresAt"] = .string("2020-01-01T00:00:00Z")
        XCTAssertNil(entry().eligible(at: now))

        var settings = RoomAudioSettings()
        settings.bgm = .asset(trackId)
        XCTAssertThrowsError(try settings.proposedCandidate(in: draft, roomId: room, approvedTrack: nil))
        let edited = try settings.proposedCandidate(in: draft, roomId: room, approvedTrack: approved)
        XCTAssertEqual(edited["mediaAssets"], .array([approved.mediaAsset]))
        try draft.replaceCandidate(edited)
        XCTAssertEqual(try settings.proposedCandidate(in: draft, roomId: room, approvedTrack: approved), edited)
        var conflicted = edited
        conflicted["mediaAssets"] = .array([media])
        try draft.replaceCandidate(conflicted)
        XCTAssertThrowsError(try settings.proposedCandidate(in: draft, roomId: room, approvedTrack: approved))
    }
}
