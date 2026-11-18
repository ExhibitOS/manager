// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Editable fields of one Viewer Experience v1 room row.
public struct RoomAudioSettings: Equatable, Sendable {
    public enum Track: Hashable, Sendable {
        case inherit, noTrack, asset(String)
    }

    public var reverb: Double = 0
    public var footstepsOn: Bool?
    public var reverbOn: Bool?
    public var bgmOn: Bool?
    public var bgm: Track = .inherit

    public init() {}

    init(fields: [String: DocumentValue]) throws {
        guard Set(fields.keys).subtracting(["footstepsOn", "reverbOn", "bgmOn", "bgm"]) == ["reverb"],
              let reverb = fields["reverb"]?.double, (0...1).contains(reverb) else { throw Self.invalid() }
        self.reverb = reverb
        footstepsOn = try Self.optionalBool(fields["footstepsOn"])
        reverbOn = try Self.optionalBool(fields["reverbOn"])
        bgmOn = try Self.optionalBool(fields["bgmOn"])
        switch fields["bgm"] {
        case nil: bgm = .inherit
        case .some(.null): bgm = .noTrack
        case .some(.object(let reference)):
            guard Set(reference.keys) == ["assetId"], let id = reference["assetId"]?.string,
                  UUID(uuidString: id) != nil else { throw Self.invalid() }
            bgm = .asset(id.lowercased())
        default: throw Self.invalid()
        }
    }

    public static func read(in draft: NativeExhibitionDraft, roomId: String) throws -> Self {
        let room = try Room(in: draft, id: roomId)
        guard let index = room.index, case .object(var fields) = room.rows[index] else {
            return Self()
        }
        fields.removeValue(forKey: "roomId")
        return try Self(fields: fields)
    }

    func proposedCandidate(in draft: NativeExhibitionDraft, roomId: String,
                           approvedTrack: ApprovedRoomAudioTrack?) throws -> DocumentValue {
        guard (0...1).contains(reverb) else { throw Self.invalid() }
        var room = try Room(in: draft, id: roomId)
        var candidate = draft.candidate
        var fields: [String: DocumentValue] = [
            "roomId": .string(room.id),
            "reverb": .number(reverb)
        ]
        fields["footstepsOn"] = footstepsOn.map(DocumentValue.bool)
        fields["reverbOn"] = reverbOn.map(DocumentValue.bool)
        fields["bgmOn"] = bgmOn.map(DocumentValue.bool)
        switch bgm {
        case .inherit: break
        case .noTrack: fields["bgm"] = .null
        case .asset(let id):
            guard let approvedTrack, approvedTrack.id.lowercased() == id.lowercased(),
                  var media = candidate["mediaAssets"].items else {
                throw AuthoringFailure("ROOM_AUDIO_TRACK_UNAVAILABLE")
            }
            let matches = media.filter { $0["id"].string?.lowercased() == id.lowercased() }
            if matches.isEmpty {
                guard media.count < 32 else { throw AuthoringFailure("ROOM_AUDIO_MEDIA_LIMIT") }
                media.append(approvedTrack.mediaAsset)
                candidate["mediaAssets"] = .array(media)
            } else if matches != [approvedTrack.mediaAsset] {
                throw AuthoringFailure("ROOM_AUDIO_TRACK_UNAVAILABLE")
            }
            fields["bgm"] = .object(["assetId": .string(approvedTrack.id)])
        }
        if let index = room.index {
            room.rows[index] = .object(fields)
        } else {
            guard fields.count > 2 || reverb != 0 else { return candidate }
            guard room.rows.count < 32 else { throw Self.invalid() }
            room.rows.append(.object(fields))
        }
        room.experience["rooms"] = .array(room.rows)
        room.extensions[ConcreteFootsteps.experienceNamespace] = .object(room.experience)
        candidate["extensions"] = .object(room.extensions)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }

    private struct Room {
        let id: String
        let index: Int?
        var rows: [DocumentValue]
        var extensions: [String: DocumentValue]
        var experience: [String: DocumentValue]

        init(in draft: NativeExhibitionDraft, id: String) throws {
            let matchingRooms = draft.rooms.filter { $0["id"].string?.lowercased() == id.lowercased() }
            guard matchingRooms.count == 1, let canonicalId = matchingRooms[0]["id"].string,
                  UUID(uuidString: canonicalId) != nil else { throw RoomAudioSettings.invalid() }
            switch draft.candidate["extensions"] {
            case .null: extensions = [:]
            case .object(let fields): extensions = fields
            default: throw RoomAudioSettings.invalid()
            }
            if let value = extensions[ConcreteFootsteps.experienceNamespace] {
                guard case .object(let fields) = value, fields["version"] == .integer(1),
                      ["footsteps", "voices", "annotations", "rooms", "translations"].allSatisfy({
                          fields[$0]?.items != nil
                      }) else { throw RoomAudioSettings.invalid() }
                experience = fields
            } else {
                experience = ["version": .integer(1), "footsteps": .array([]), "voices": .array([]),
                              "annotations": .array([]), "rooms": .array([]), "translations": .array([])]
            }
            guard let existing = experience["rooms"]?.items, existing.count <= 32 else {
                throw RoomAudioSettings.invalid()
            }
            let matches = existing.indices.filter { existing[$0]["roomId"].string?.lowercased() == id.lowercased() }
            guard matches.count <= 1 else { throw RoomAudioSettings.invalid() }
            self.id = canonicalId
            index = matches.first
            rows = existing
        }
    }

    private static func optionalBool(_ value: DocumentValue?) throws -> Bool? {
        guard let value else { return nil }
        guard case .bool(let enabled) = value else { throw invalid() }
        return enabled
    }

    private static func invalid() -> AuthoringFailure { AuthoringFailure("VIEWER_EXPERIENCE_INVALID") }
}

public struct ApprovedRoomAudioTrack: Identifiable, Sendable {
    public let id: String
    public let creditLine: String
    let mediaAsset: DocumentValue

    struct CatalogItem: Decodable, Sendable {
        let id: String
        let state: String
        let revision: Int
        let revoked: Bool
        let mediaAsset: DocumentValue

        func eligible(at date: Date) -> ApprovedRoomAudioTrack? {
            let rights = mediaAsset["rights"]
            guard UUID(uuidString: id) != nil, state == "approved", revision == 1, !revoked,
                  mediaAsset["id"] == .string(id), mediaAsset["mime"] == .string("audio/wav"),
                  mediaAsset["path"] == .string("media/\(id)/audio.wav"),
                  let bytes = mediaAsset["bytes"].integer, (44...12_582_912).contains(bytes),
                  let digest = mediaAsset["sha256"].string, nativeImportDigest(digest),
                  rights["permissions"]["display"] == .bool(true), rights["permissions"]["export"] == .bool(true),
                  let credit = rights["creditLine"].string,
                  case .object(let fields) = rights else { return nil }
            let formatter = ISO8601DateFormatter()
            let fractional = ISO8601DateFormatter()
            fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            func dateBound(_ key: String, fallback: Date) -> Date? {
                guard let field = fields[key] else { return fallback }
                guard let value = field.string, value.hasSuffix("Z") else { return nil }
                return fractional.date(from: value) ?? formatter.date(from: value)
            }
            guard let from = dateBound("validFrom", fallback: .distantPast),
                  let until = dateBound("expiresAt", fallback: .distantFuture),
                  from <= date, date < until else { return nil }
            return ApprovedRoomAudioTrack(id: id, creditLine: credit.isEmpty ? "WAV" : credit, mediaAsset: mediaAsset)
        }
    }
}

/// An exact server audio record, kept separate from assigning it to a room.
public struct NativeRoomAudioReceipt: Sendable {
    public let value: DocumentValue
    public let id: String
    public var mediaAsset: DocumentValue { value["mediaAsset"] }
    public var state: String { value["state"].string! }
    init(_ value: DocumentValue, expectedId: String? = nil, expectedMedia: DocumentValue? = nil) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == ["id", "state", "revision", "revoked", "durationSeconds", "mediaAsset"],
              let id = value["id"].string, UUID(uuidString: id) != nil,
              expectedId == nil || expectedId == id,
              ["uploading", "validated", "approved"].contains(value["state"].string ?? ""),
              value["revision"] == .integer(1), case .bool = value["revoked"],
              value["mediaAsset"]["id"] == .string(id),
              value["mediaAsset"]["path"] == .string("media/\(id)/audio.wav"),
              value["mediaAsset"]["mime"] == .string("audio/wav"),
              let count = value["mediaAsset"]["bytes"].integer, (44...12_582_912).contains(count),
              let hash = value["mediaAsset"]["sha256"].string, nativeImportDigest(hash),
              expectedMedia == nil || expectedMedia == value["mediaAsset"] else {
            throw AuthoringFailure("ROOM_AUDIO_RESPONSE_INVALID")
        }
        if value["state"] != .string("uploading") {
            guard let duration = value["durationSeconds"].double, duration > 0, duration <= 60 else {
                throw AuthoringFailure("ROOM_AUDIO_RESPONSE_INVALID")
            }
        }
        self.value = value; self.id = id
    }
    func requireAssignableRights() throws {
        let item = ApprovedRoomAudioTrack.CatalogItem(id: id, state: "approved", revision: 1,
            revoked: value["revoked"] != .bool(false), mediaAsset: mediaAsset)
        guard item.eligible(at: Date()) != nil else { throw AuthoringFailure("ROOM_AUDIO_TRACK_UNAVAILABLE") }
    }
}

/// One approved audio snapshot bound to an existing artwork placement.
/// Preparation is read-only; applying retains the original ordinary Save intent.
public struct NativePlacementVoice: Sendable {
    public let placementId: String
    public let locale: String
    public let transcript: String
    public let approvedAudio: NativeRoomAudioReceipt

    public init(placementId: String, locale: String, transcript: String,
                approvedAudio: NativeRoomAudioReceipt) throws {
        guard UUID(uuidString: placementId) != nil,
              Self.validLocale(locale), !transcript.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              transcript.utf16.count <= 16_384, approvedAudio.state == "approved" else {
            throw AuthoringFailure("PLACEMENT_VOICE_INVALID")
        }
        try approvedAudio.requireAssignableRights()
        self.placementId = placementId; self.locale = locale; self.transcript = transcript
        self.approvedAudio = approvedAudio
    }

    public func prepare(using api: AuthoringAPI, in document: NativeDocumentState) async throws -> NativePlacementVoicePreview {
        guard !document.dirty, document.pending == nil, let draft = document.draft,
              let etag = document.etag else { throw AuthoringFailure("ROOM_AUDIO_SAVE_REQUIRED") }
        // Reuse the existing native-session, owner, ordinary policy and saved-head guard.
        let currentAudio = try await api.roomAudioStatus(approvedAudio.id,
            expectedMedia: approvedAudio.mediaAsset, in: document)
        guard currentAudio.state == "approved" else { throw AuthoringFailure("AUDIO_APPROVAL_CHANGED") }
        try currentAudio.requireAssignableRights()
        let saved = try await api.exhibition(draft.id)
        guard saved.draft == draft, saved.etag == etag else { throw AuthoringFailure("EXHIBITION_COMPARISON_STALE") }
        let request = try PlacementVoiceRequest(binding: self, document: document)
        let reply = try await api.nativeAuthoringProposal(exhibitionId: draft.id, studioRevision: saved.revision,
            etag: etag, proposal: request.value)
        let after = try await api.roomAudioStatus(approvedAudio.id,
            expectedMedia: approvedAudio.mediaAsset, in: document)
        guard after.value == currentAudio.value else { throw AuthoringFailure("AUDIO_APPROVAL_CHANGED") }
        return try request.checkedReply(reply)
    }

    static func validLocale(_ value: String) -> Bool {
        value.range(of: #"\A[A-Za-z]{2,8}(?:-[A-Za-z0-9]{1,8}){0,4}\z"#,
                    options: .regularExpression) != nil
    }
}

public struct NativePlacementVoicePreview: Sendable {
    let initial: NativeDocumentState
    let approvedAudio: NativeRoomAudioReceipt
    public let candidate: DocumentValue
    public var changed: Bool { candidate != initial.draft?.candidate }

    /// The existing helper persists this exact draft and original Save ID together.
    /// No caller-provided candidate or rights can replace the checked proposal.
    public func applying(to current: NativeDocumentState, requestId: String) throws -> NativeDocumentState {
        guard current.format == initial.format, current.revision == initial.revision,
              current.workspaceId == initial.workspaceId, current.tenantId == initial.tenantId,
              current.principalId == initial.principalId, current.draft == initial.draft,
              current.etag == initial.etag, current.savedEditVersion == initial.savedEditVersion,
              current.pending == nil, !current.dirty, var draft = current.draft else {
            throw AuthoringFailure("EXHIBITION_COMPARISON_STALE")
        }
        try approvedAudio.requireAssignableRights()
        guard changed else { return current }
        try draft.replaceCandidate(candidate)
        return try NativeAppliedDraftSave.stage(current, draft: draft, requestId: requestId)
    }
}

/// Closed request and complete expected delta, analogous to the existing typed light helpers.
struct PlacementVoiceRequest: Sendable {
    let initial: NativeDocumentState
    let approvedAudio: NativeRoomAudioReceipt
    let value: DocumentValue
    let expected: DocumentValue
    let placementId: String

    init(binding: NativePlacementVoice, document: NativeDocumentState) throws {
        guard document.format == 1, !document.dirty, document.pending == nil,
              document.etag != nil, let draft = document.draft,
              [document.workspaceId, document.tenantId, document.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
              let media = draft.candidate["mediaAssets"].items, media.count <= 32 else { throw Self.invalid }
        let placements = draft.placements.filter { $0["id"].string?.lowercased() == binding.placementId.lowercased() }
        guard placements.count == 1, let placementId = placements[0]["id"].string else { throw Self.invalid }
        try binding.approvedAudio.requireAssignableRights()
        var candidate = draft.candidate
        var assets = media
        let matches = assets.filter { $0["id"].string?.lowercased() == binding.approvedAudio.id.lowercased() }
        guard matches.isEmpty || matches == [binding.approvedAudio.mediaAsset] else {
            throw AuthoringFailure("AUDIO_APPROVAL_CHANGED")
        }
        if matches.isEmpty {
            guard assets.count < 32 else { throw AuthoringFailure("ROOM_AUDIO_MEDIA_LIMIT") }
            assets.append(binding.approvedAudio.mediaAsset)
        }
        var extensions: [String: DocumentValue]
        switch candidate["extensions"] {
        case .null: extensions = [:]
        case .object(let fields): extensions = fields
        default: throw Self.invalid
        }
        let ns = ConcreteFootsteps.experienceNamespace
        var experience: [String: DocumentValue]
        if let old = extensions[ns] {
            guard case .object(let fields) = old, fields["version"] == .integer(1),
                  ["footsteps", "voices", "annotations", "rooms", "translations"].allSatisfy({ fields[$0]?.items != nil }) else { throw Self.invalid }
            experience = fields
        } else {
            experience = ["version": .integer(1), "footsteps": .array([]), "voices": .array([]),
                          "annotations": .array([]), "rooms": .array([]), "translations": .array([])]
        }
        guard var voices = experience["voices"]?.items, voices.count <= 64 else { throw Self.invalid }
        var keys = Set<String>()
        for row in voices {
            guard case .object(let fields) = row, Set(fields.keys) == ["placementId", "assetId", "locale", "transcript"],
                  let id = row["placementId"].string, UUID(uuidString: id) != nil,
                  let assetId = row["assetId"].string, UUID(uuidString: assetId) != nil,
                  let locale = row["locale"].string, NativePlacementVoice.validLocale(locale),
                  let text = row["transcript"].string, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                  text.utf16.count <= 16_384,
                  keys.insert(id.lowercased() + ":" + locale.lowercased()).inserted else { throw Self.invalid }
        }
        let voice: DocumentValue = .object(["placementId": .string(placementId),
            "assetId": .string(binding.approvedAudio.id), "locale": .string(binding.locale), "transcript": .string(binding.transcript)])
        if let index = voices.firstIndex(where: {
            $0["placementId"].string?.lowercased() == placementId.lowercased()
                && $0["locale"].string?.lowercased() == binding.locale.lowercased()
        }) { voices[index] = voice }
        else { guard voices.count < 64 else { throw Self.invalid }; voices.append(voice) }
        experience["voices"] = .array(voices); extensions[ns] = .object(experience)
        candidate["extensions"] = .object(extensions); candidate["mediaAssets"] = .array(assets)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        self.initial = document; self.approvedAudio = binding.approvedAudio
        self.expected = candidate; self.placementId = placementId
        value = .object(["version": .integer(1), "epoch": .string(UUID().uuidString.lowercased()), "sequence": .integer(1),
            "tenantId": .string(document.tenantId), "principalId": .string(document.principalId),
            "requestId": .string(UUID().uuidString.lowercased()), "type": .string("set-placement-voice"),
            "placementId": .string(placementId), "audioId": .string(binding.approvedAudio.id),
            "locale": .string(binding.locale), "transcript": .string(binding.transcript),
            "expectedMediaAsset": binding.approvedAudio.mediaAsset])
    }

    func checkedReply(_ reply: DocumentValue) throws -> NativePlacementVoicePreview {
        let identity: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
        guard case .object(let fields) = reply, Set(fields.keys) == identity.union(["candidate", "placementId"]),
              try JSONEncoder().encode(reply).count <= 1_100_000,
              identity.allSatisfy({ reply[$0] == value[$0] }), reply["placementId"] == .string(placementId),
              reply["candidate"] == expected else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        return NativePlacementVoicePreview(initial: initial, approvedAudio: approvedAudio, candidate: expected)
    }
    private static var invalid: AuthoringFailure { AuthoringFailure("PLACEMENT_VOICE_INVALID") }
}
