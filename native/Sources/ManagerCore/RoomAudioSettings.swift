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
