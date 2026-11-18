// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
@testable import ManagerCore

final class RoomAudioSettingsTests: XCTestCase {
    private let trackId = "5fd33e2b-a4ab-480d-986d-d82e17de175a"

    func testMP3RetryUsesRetainedFilesAndRejectsChangedPins() throws {
        let profile = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: profile, withIntermediateDirectories: false,
                                                attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: profile) }
        let requestId = UUID().uuidString.lowercased()
        let directory = profile.appendingPathComponent("native-room-audio-" + requestId)
        let scope: DocumentValue = .object(["exhibitionId": .string(UUID().uuidString)])
        // A prepared checkpoint, independent of the system MP3 decoder.
        let original = Data([0xff, 0xfb, 0x90, 0x64])
        let wav = Data([
            82, 73, 70, 70, 38, 0, 0, 0, 87, 65, 86, 69, 102, 109, 116, 32,
            16, 0, 0, 0, 1, 0, 1, 0, 64, 31, 0, 0, 128, 62, 0, 0,
            2, 0, 16, 0, 100, 97, 116, 97, 2, 0, 0, 0, 0, 0
        ])
        func digest(_ data: Data) -> String {
            SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        }
        let hash = digest(original)
        XCTAssertNil(try NativeRoomAudioPreparation.resume(profile: profile, requestID: requestId,
                                                           scope: scope, originalHash: hash))
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false,
                                                attributes: [.posixPermissions: 0o700])
        let retainedOriginal = directory.appendingPathComponent("original.mp3")
        try original.write(to: retainedOriginal)
        try wav.write(to: directory.appendingPathComponent("prepared.wav"))
        let checkpoint: DocumentValue = .object([
            "binding": .object(["recipe": .string("core-audio-mp3-pcm16le-source-format-v1"),
                                "scope": scope, "originalSha256": .string(hash), "originalBytes": .integer(4)]),
            "wavSha256": .string(digest(wav))
        ])
        try JSONEncoder().encode(checkpoint).write(to: directory.appendingPathComponent("preparation.json"))
        let retained = try NativeRoomAudioPreparation.resume(profile: profile, requestID: requestId,
                                                             scope: scope, originalHash: hash)
        XCTAssertEqual(retained?.wav.data, wav)
        XCTAssertThrowsError(try NativeRoomAudioPreparation.resume(profile: profile, requestID: requestId,
                                                                  scope: .object([:]), originalHash: hash))
        XCTAssertThrowsError(try NativeRoomAudioPreparation.resume(profile: profile, requestID: requestId,
                                                                  scope: scope, originalHash: digest(wav)))
        try Data([0xff, 0xfb, 0x90, 0x65]).write(to: retainedOriginal)
        XCTAssertThrowsError(try NativeRoomAudioPreparation.resume(profile: profile, requestID: requestId,
                                                                  scope: scope, originalHash: hash))
    }

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

    private func voiceFixture() throws -> (NativeDocumentState, NativeRoomAudioReceipt) {
        var draft = try FrameFitSynthetic.draft()
        var candidate = draft.candidate
        candidate["extensions"][ConcreteFootsteps.experienceNamespace] = .object([
            "version": .integer(1), "footsteps": .array([]), "voices": .array([]),
            "annotations": .array([]), "translations": .array([]),
            "rooms": .array([.object(["roomId": draft.rooms[0]["id"], "reverb": .number(0.3), "bgmOn": .bool(false)])]),
            "audioDefaults": .object(["soundOn": .bool(false)])])
        try draft.replaceCandidate(candidate)
        let state = NativeDocumentState(format: 1, revision: 7, workspaceId: FrameFitSynthetic.owner,
            tenantId: FrameFitSynthetic.owner, principalId: FrameFitSynthetic.owner, draft: draft,
            etag: "\"studio-r1-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let media: DocumentValue = .object(["id": .string(trackId), "path": .string("media/\(trackId)/audio.wav"),
            "mime": .string("audio/wav"), "bytes": .integer(44), "sha256": .string(String(repeating: "f", count: 64)),
            "rights": .object(["creditLine": .string("Synthetic recording"),
                "permissions": .object(["display": .bool(true), "export": .bool(true)])])])
        let receipt = try NativeRoomAudioReceipt(.object(["id": .string(trackId), "state": .string("approved"),
            "revision": .integer(1), "revoked": .bool(false), "durationSeconds": .number(1), "mediaAsset": media]))
        return (state, receipt)
    }
    private func voiceReply(_ request: PlacementVoiceRequest, candidate: DocumentValue) -> DocumentValue {
        var fields: [String: DocumentValue] = ["placementId": .string(request.placementId), "candidate": candidate]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { fields[key] = request.value[key] }
        return .object(fields)
    }
    func testPlacementVoiceNarrowReplyPreservesBGMAndRetainsOriginalSaveIntent() throws {
        let (state, audio) = try voiceFixture()
        let binding = try NativePlacementVoice(placementId: FrameFitSynthetic.placement, locale: "ko", transcript: "합성 설명", approvedAudio: audio)
        let request = try PlacementVoiceRequest(binding: binding, document: state)
        var expected = try XCTUnwrap(state.draft?.candidate)
        expected["mediaAssets"] = .array([audio.mediaAsset])
        expected["extensions"][ConcreteFootsteps.experienceNamespace]["voices"] = .array([.object([
            "placementId": .string(FrameFitSynthetic.placement), "assetId": .string(trackId),
            "locale": .string("ko"), "transcript": .string("합성 설명")])])
        XCTAssertEqual(request.expected, expected)
        let reply = voiceReply(request, candidate: expected), preview = try request.checkedReply(reply)
        let saveId = "99999999-9999-4999-8999-999999999999"
        let staged = try preview.applying(to: state, requestId: saveId)
        XCTAssertEqual(staged.pending?.requestId, saveId)
        XCTAssertEqual(staged.pending?.origin, NativeAppliedDraftSave.origin)
        XCTAssertEqual(staged.pending?.etag, state.etag)
        XCTAssertEqual(staged.pending?.draft, staged.draft)
        XCTAssertEqual(staged.draft?.candidate, expected)
        var stale = state; stale.revision += 1
        XCTAssertThrowsError(try preview.applying(to: stale, requestId: saveId))
        var bad = reply; bad["candidate"]["rooms"] = .array([])
        XCTAssertThrowsError(try request.checkedReply(bad))
        bad = reply; var assets = try XCTUnwrap(bad["candidate"]["mediaAssets"].items)
        assets[0]["rights"]["permissions"]["export"] = .bool(false); bad["candidate"]["mediaAssets"] = .array(assets)
        XCTAssertThrowsError(try request.checkedReply(bad))
        bad = reply; bad["principalId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try request.checkedReply(bad))
        bad = reply; bad["unrequested"] = .bool(true)
        XCTAssertThrowsError(try request.checkedReply(bad))
    }
    func testPlacementVoiceCaseFoldUpsertReusesExactMediaAndNoopDoesNotStageSave() throws {
        var (state, audio) = try voiceFixture()
        var draft = try XCTUnwrap(state.draft), candidate = draft.candidate
        candidate["mediaAssets"] = .array([audio.mediaAsset])
        candidate["extensions"][ConcreteFootsteps.experienceNamespace]["voices"] = .array([.object([
            "placementId": .string(FrameFitSynthetic.placement), "assetId": .string(trackId),
            "locale": .string("KO"), "transcript": .string("old")])])
        try draft.replaceCandidate(candidate); state.draft = draft; state.savedEditVersion = draft.document["editVersion"].integer
        let binding = try NativePlacementVoice(placementId: FrameFitSynthetic.placement, locale: "ko", transcript: "new", approvedAudio: audio)
        let request = try PlacementVoiceRequest(binding: binding, document: state)
        XCTAssertEqual(request.expected["mediaAssets"].items?.count, 1)
        XCTAssertEqual(request.expected["extensions"][ConcreteFootsteps.experienceNamespace]["voices"].items?.count, 1)
        try draft.replaceCandidate(request.expected); state.draft = draft; state.savedEditVersion = draft.document["editVersion"].integer
        let repeatRequest = try PlacementVoiceRequest(binding: binding, document: state)
        let preview = try repeatRequest.checkedReply(voiceReply(repeatRequest, candidate: draft.candidate))
        XCTAssertFalse(preview.changed)
        XCTAssertNil(try preview.applying(to: state, requestId: UUID().uuidString.lowercased()).pending)
    }
    func testPlacementVoiceRejectsMediaConflictMissingPlacementAndDuplicateLocale() throws {
        var (state, audio) = try voiceFixture()
        let binding = try NativePlacementVoice(placementId: FrameFitSynthetic.placement, locale: "ko", transcript: "Synthetic", approvedAudio: audio)
        var draft = try XCTUnwrap(state.draft), candidate = draft.candidate
        var conflicting = audio.mediaAsset; conflicting["sha256"] = .string(String(repeating: "a", count: 64))
        candidate["mediaAssets"] = .array([conflicting]); try draft.replaceCandidate(candidate)
        state.draft = draft; state.savedEditVersion = draft.document["editVersion"].integer
        XCTAssertThrowsError(try PlacementVoiceRequest(binding: binding, document: state))
        candidate["mediaAssets"] = .array([audio.mediaAsset]); candidate["placements"] = .array([])
        try draft.replaceCandidate(candidate); state.draft = draft; state.savedEditVersion = draft.document["editVersion"].integer
        XCTAssertThrowsError(try PlacementVoiceRequest(binding: binding, document: state))
        (state, _) = try voiceFixture(); draft = try XCTUnwrap(state.draft); candidate = draft.candidate
        let row: DocumentValue = .object(["placementId": .string(FrameFitSynthetic.placement), "assetId": .string(trackId), "locale": .string("ko"), "transcript": .string("old")])
        var upper = row; upper["locale"] = .string("KO")
        candidate["extensions"][ConcreteFootsteps.experienceNamespace]["voices"] = .array([row, upper])
        try draft.replaceCandidate(candidate); state.draft = draft; state.savedEditVersion = draft.document["editVersion"].integer
        XCTAssertThrowsError(try PlacementVoiceRequest(binding: binding, document: state))
    }
    func testPlacementVoiceRequiresApprovalCurrentRightsAndUTF16Bounds() throws {
        let (_, audio) = try voiceFixture()
        for mode in ["validated", "revoked", "no-export", "expired"] {
            var value = audio.value
            if mode == "validated" { value["state"] = .string("validated") }
            if mode == "revoked" { value["revoked"] = .bool(true) }
            if mode == "no-export" { value["mediaAsset"]["rights"]["permissions"]["export"] = .bool(false) }
            if mode == "expired" { value["mediaAsset"]["rights"]["expiresAt"] = .string("2020-01-01T00:00:00Z") }
            let receipt = try NativeRoomAudioReceipt(value)
            XCTAssertThrowsError(try NativePlacementVoice(placementId: FrameFitSynthetic.placement, locale: "ko", transcript: "Synthetic", approvedAudio: receipt), mode)
        }
        for locale in ["", "ko\n", "ko_KR"] {
            XCTAssertThrowsError(try NativePlacementVoice(placementId: FrameFitSynthetic.placement, locale: locale, transcript: "Synthetic", approvedAudio: audio))
        }
        XCTAssertThrowsError(try NativePlacementVoice(placementId: FrameFitSynthetic.placement, locale: "ko", transcript: String(repeating: "😀", count: 8193), approvedAudio: audio))
    }
}
