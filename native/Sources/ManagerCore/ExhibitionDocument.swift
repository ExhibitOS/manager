// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Opaque extension fields and approved artwork snapshots survive native edits.
/// Integers are kept distinct from floating point values; no Codable subset may
/// silently discard fields that the current native form doesn't display.
public indirect enum DocumentValue: Codable, Equatable, Sendable {
    case object([String: DocumentValue]), array([DocumentValue]), string(String), integer(Int64), number(Double), bool(Bool), null
    public static func == (lhs: Self, rhs: Self) -> Bool {
        switch (lhs, rhs) {
        case (.object(let a), .object(let b)): a == b
        case (.array(let a), .array(let b)): a == b
        case (.string(let a), .string(let b)): a == b
        case (.integer(let a), .integer(let b)): a == b
        case (.number(let a), .number(let b)): a == b
        case (.integer(let a), .number(let b)), (.number(let b), .integer(let a)): Int64(exactly: b) == a
        case (.bool(let a), .bool(let b)): a == b
        case (.null, .null): true
        default: false
        }
    }
    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Int64.self) { self = .integer(v) }
        else if let v = try? c.decode(Double.self), v.isFinite { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([DocumentValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: DocumentValue].self)) }
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .object(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .integer(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .bool(let v): try c.encode(v)
        case .null: try c.encodeNil()
        }
    }
    public subscript(_ key: String) -> Self {
        get { if case .object(let value) = self { return value[key] ?? .null }; return .null }
        set { guard case .object(var value) = self else { return }; value[key] = newValue; self = .object(value) }
    }
    public var string: String? { if case .string(let v) = self { return v }; return nil }
    public var items: [Self]? { if case .array(let v) = self { return v }; return nil }
    public var double: Double? { switch self { case .integer(let v): Double(v); case .number(let v): v; default: nil } }
    public var integer: Int64? { if case .integer(let v) = self { return v }; return nil }
    public static func vector(_ values: [Double]) -> Self { .array(values.map { .number($0) }) }
}

public struct NativeExhibitionDraft: Codable, Equatable, Sendable {
    public private(set) var document: DocumentValue
    public var id: String { document["exhibitionId"].string! }
    public var title: String { document["candidate"]["title"].string ?? "" }
    public var candidate: DocumentValue { document["candidate"] }
    public var rooms: [DocumentValue] { candidate["rooms"].items ?? [] }
    public var placements: [DocumentValue] { candidate["placements"].items ?? [] }
    public var artworks: [DocumentValue] { candidate["artworks"].items ?? [] }
    public init(document: DocumentValue) throws {
        // Envelope/identity admission only. Platform's existing full OES and
        // Studio validators remain authoritative on save and viewport entry.
        let c = document["candidate"]
        guard document["kind"].string == "exhibition-draft", document["schemaVersion"].string == "1.0.0-draft.1",
              let id = document["exhibitionId"].string, UUID(uuidString: id) != nil,
              let draftId = document["id"].string, UUID(uuidString: draftId) != nil,
              let version = document["editVersion"].integer, version > 0, version <= 2_147_483_647,
              c["kind"].string == "exhibition-revision", c["schemaVersion"].string == "1.0.0-draft.1",
              c["id"].string?.lowercased() == id.lowercased(), let revision = c["revisionId"].string, UUID(uuidString: revision) != nil,
              c["units"].string == "meter", c["coordinates"].string == "right-handed-y-up",
              c["title"].string != nil, c["rooms"].items != nil, c["artworks"].items != nil, c["placements"].items != nil else { throw AuthoringFailure("EXHIBITION_DOCUMENT_INVALID") }
        self.document = document
    }
    public init(from decoder: Decoder) throws { try self.init(document: DocumentValue(from: decoder)) }
    public func encode(to encoder: Encoder) throws { try document.encode(to: encoder) }
    public static func new(title: String = "새 전시", now: Date = Date()) throws -> Self {
        let timestamp = ISO8601DateFormatter().string(from: now), id = UUID().uuidString.lowercased()
        var candidate: [String: DocumentValue] = ["schemaVersion": .string("1.0.0-draft.1"), "kind": .string("exhibition-revision"), "id": .string(id), "revisionId": .string(UUID().uuidString.lowercased()), "revision": .integer(1), "createdAt": .string(timestamp), "title": .string(title), "units": .string("meter"), "coordinates": .string("right-handed-y-up")]
        candidate["rooms"] = .array([.object(["id": .string(UUID().uuidString.lowercased()), "name": .string("첫 공간"), "dimensions": .object(["width": .number(12), "height": .number(4), "depth": .number(8)]), "transform": identity])])
        for key in ["surfaces", "openings", "artworks", "placements", "lights", "navigation", "mediaAssets", "audioZones", "annotations", "scripts"] { candidate[key] = .array([]) }
        candidate["accessibility"] = .object(["alternativeView": .string("list"), "keyboardNavigation": .bool(true), "reducedMotion": .bool(true), "stationaryNavigation": .bool(true), "routeIds": .array([]), "artworkDescriptions": .array([])])
        return try Self(document: .object(["schemaVersion": .string("1.0.0-draft.1"), "kind": .string("exhibition-draft"), "id": .string(UUID().uuidString.lowercased()), "exhibitionId": .string(id), "editVersion": .integer(1), "createdAt": .string(timestamp), "updatedAt": .string(timestamp), "candidate": .object(candidate)]))
    }
    public static var identity: DocumentValue { .object(["position": .vector([0, 0, 0]), "rotation": .vector([0, 0, 0, 1]), "scale": .vector([1, 1, 1])]) }
    private mutating func change(_ candidate: DocumentValue) throws {
        guard let version = document["editVersion"].integer, version < 2_147_483_647 else { throw AuthoringFailure("REVISION_LIMIT") }
        document["candidate"] = candidate
        document["editVersion"] = .integer(version + 1)
        document["updatedAt"] = .string(ISO8601DateFormatter().string(from: Date()))
    }
    /// Used for native undo/redo; document identity is never replaced.
    public mutating func replaceCandidate(_ candidate: DocumentValue) throws {
        guard candidate["id"] == self.candidate["id"], candidate["revisionId"] == self.candidate["revisionId"], candidate["kind"] == self.candidate["kind"], candidate["schemaVersion"] == self.candidate["schemaVersion"] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        try change(candidate)
    }
    public mutating func rename(_ title: String) throws {
        guard !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, title.count <= 512 else { throw AuthoringFailure("TITLE_INVALID") }
        var c = candidate; c["title"] = .string(title); try change(c)
    }
    public mutating func updateRoom(_ id: String, name: String, width: Double, height: Double, depth: Double) throws {
        guard !name.isEmpty, name.count <= 512, [width, height, depth].allSatisfy({ $0.isFinite && $0 > 0 && $0 <= 1_000_000 }), var rows = candidate["rooms"].items, let i = rows.firstIndex(where: { $0["id"].string == id }) else { throw AuthoringFailure("ROOM_INVALID") }
        rows[i]["name"] = .string(name); rows[i]["dimensions"] = .object(["width": .number(width), "height": .number(height), "depth": .number(depth)])
        var c = candidate; c["rooms"] = .array(rows); try change(c)
    }
    public mutating func addApprovedArtwork(_ artwork: DocumentValue, roomId: String, position: [Double]) throws {
        var pose = Self.identity; pose["position"] = .vector(position)
        try addApprovedArtwork(artwork, roomId: roomId, placementId: UUID().uuidString.lowercased(), transform: pose)
    }
    /// The same approved-add mutation serves numeric and explicit visual placement.
    /// A visual proposal supplies only the pinned placement identity and unit pose.
    public mutating func addApprovedArtwork(_ artwork: DocumentValue, roomId: String, placementId: String, transform: DocumentValue) throws {
        guard let revision = artwork["revisionId"].string, UUID(uuidString: revision) != nil,
              let asset = artwork["primaryAssetId"].string, UUID(uuidString: asset) != nil,
              artwork["kind"].string == "artwork", rooms.contains(where: { $0["id"].string == roomId }),
              UUID(uuidString: placementId) != nil, !placements.contains(where: { $0["id"].string?.lowercased() == placementId.lowercased() }),
              case .object(let fields) = transform, Set(fields.keys) == Set(["position", "rotation", "scale"]),
              let position = transform["position"].items, position.count == 3,
              position.allSatisfy({ $0.double.map { $0.isFinite && abs($0) <= 1_000_000 } == true }),
              let rotation = transform["rotation"].items, rotation.count == 4,
              rotation.allSatisfy({ $0.double.map { $0.isFinite && abs($0) <= 1 } == true }),
              abs(rotation.reduce(0) { $0 + pow($1.double!, 2) } - 1) <= 1e-6,
              transform["scale"] == .vector([1, 1, 1]) else { throw AuthoringFailure("PLACEMENT_INVALID") }
        var c = candidate, inventory = artworks
        if let old = inventory.first(where: { $0["revisionId"].string == revision }) {
            guard old == artwork else { throw AuthoringFailure("ARTWORK_REVISION_CHANGED") }
        } else { inventory.append(artwork) }
        let description = artwork["metadata"]["description"].string?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let alternative = description.isEmpty ? (artwork["metadata"]["title"].string ?? "작품") : description
        var rows = placements
        rows.append(.object(["id": .string(placementId), "roomId": .string(roomId), "artworkRevisionId": .string(revision), "assetId": .string(asset), "transform": transform]))
        var descriptions = c["accessibility"]["artworkDescriptions"].items ?? []
        descriptions.append(.object(["placementId": .string(placementId), "text": .string(alternative)]))
        c["accessibility"]["artworkDescriptions"] = .array(descriptions)
        c["artworks"] = .array(inventory); c["placements"] = .array(rows); try change(c)
    }
    public mutating func movePlacement(_ id: String, position: [Double]) throws {
        guard position.count == 3, position.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }), var rows = candidate["placements"].items, let i = rows.firstIndex(where: { $0["id"].string == id }) else { throw AuthoringFailure("PLACEMENT_INVALID") }
        rows[i]["transform"]["position"] = .vector(position)
        var c = candidate; c["placements"] = .array(rows); try change(c)
    }
}
public struct NativeStudioResponse: Decodable, Sendable {
    public let revision: Int
    public let etag: String
    public let draft: NativeExhibitionDraft

    /// A comparison is a user-reviewed value, not permission to silently adopt a
    /// newer or different server response. The caller must re-read before use.
    public func requireSameComparison(_ current: NativeStudioResponse) throws {
        guard revision == current.revision, etag == current.etag, draft == current.draft else {
            throw AuthoringFailure("EXHIBITION_COMPARISON_STALE", status: 409)
        }
    }
}
public struct NativeExhibitionSummary: Decodable, Sendable, Identifiable {
    public let id: String
    public let revision: Int
    public let title: String
}
public struct NativeStudioArtwork: Decodable, Sendable {
    public let artwork: DocumentValue
    public let previewUrl: String
}

/// One private current draft, base ETag and exact pending save, bound by Rust to
/// the Mac workspace/principal. Saved edit version distinguishes unsent edits.
public struct NativeDocumentState: Codable, Sendable {
    public struct Pending: Codable, Sendable {
        public let requestId: String
        public let etag: String?
        public let draft: NativeExhibitionDraft
        public init(requestId: String = UUID().uuidString.lowercased(), etag: String?, draft: NativeExhibitionDraft) { self.requestId = requestId; self.etag = etag; self.draft = draft }
    }
    public let format: Int
    public var revision: UInt64
    public let workspaceId: String
    public let tenantId: String
    public let principalId: String
    public var draft: NativeExhibitionDraft?
    public var etag: String?
    public var savedEditVersion: Int64?
    public var pending: Pending?
    public var dirty: Bool { pending != nil || (draft != nil && draft?.document["editVersion"].integer != savedEditVersion) }
}
