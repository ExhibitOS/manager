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
    private static let legacySchema = DocumentValue.string("1.0.0-draft.1")
    private static let qualifiedSchema = DocumentValue.string("1.0.0-draft.2")
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
        let schema = document["schemaVersion"]
        guard document["kind"].string == "exhibition-draft", schema == Self.legacySchema || schema == Self.qualifiedSchema,
              let id = document["exhibitionId"].string, UUID(uuidString: id) != nil,
              let draftId = document["id"].string, UUID(uuidString: draftId) != nil,
              let version = document["editVersion"].integer, version > 0, version <= 2_147_483_647,
              c["kind"].string == "exhibition-revision", c["schemaVersion"] == schema,
              c["id"].string?.lowercased() == id.lowercased(), let revision = c["revisionId"].string, UUID(uuidString: revision) != nil,
              c["units"].string == "meter", c["coordinates"].string == "right-handed-y-up",
              c["title"].string != nil, c["rooms"].items != nil, c["artworks"].items != nil, c["placements"].items != nil else { throw AuthoringFailure("EXHIBITION_DOCUMENT_INVALID") }
        self.document = document
    }
    public init(from decoder: Decoder) throws { try self.init(document: DocumentValue(from: decoder)) }
    public func encode(to encoder: Encoder) throws { try document.encode(to: encoder) }
    public static func new(title: String = "새 전시", now: Date = Date(), roomBaseLight: Bool = false) throws -> Self {
        let timestamp = ISO8601DateFormatter().string(from: now), id = UUID().uuidString.lowercased()
        var candidate: [String: DocumentValue] = ["schemaVersion": .string("1.0.0-draft.1"), "kind": .string("exhibition-revision"), "id": .string(id), "revisionId": .string(UUID().uuidString.lowercased()), "revision": .integer(1), "createdAt": .string(timestamp), "title": .string(title), "units": .string("meter"), "coordinates": .string("right-handed-y-up")]
        let room = RoomBaseLightAction.newRoom(id: UUID().uuidString.lowercased(), name: "첫 공간")
        candidate["rooms"] = .array([room])
        for key in ["surfaces", "openings", "artworks", "placements", "lights", "navigation", "mediaAssets", "audioZones", "annotations", "scripts"] { candidate[key] = .array([]) }
        candidate["accessibility"] = .object(["alternativeView": .string("list"), "keyboardNavigation": .bool(true), "reducedMotion": .bool(true), "stationaryNavigation": .bool(true), "routeIds": .array([]), "artworkDescriptions": .array([])])
        let seed: DocumentValue = .object(candidate)
        let initial = roomBaseLight
            ? try RoomBaseLightAction.appendingLight(to: seed, room: room, lightId: UUID().uuidString.lowercased())
            : seed
        return try Self(document: .object(["schemaVersion": .string("1.0.0-draft.1"), "kind": .string("exhibition-draft"), "id": .string(UUID().uuidString.lowercased()), "exhibitionId": .string(id), "editVersion": .integer(1), "createdAt": .string(timestamp), "updatedAt": .string(timestamp), "candidate": initial]))
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
    /// History holds validated local drafts. Restore both schema versions in
    /// the same edit while keeping the current journal's edit counter.
    public mutating func restoreEditHistory(_ previous: Self) throws {
        guard previous.document["id"] == document["id"], previous.id == id,
              previous.candidate["revisionId"] == candidate["revisionId"] else {
            throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED")
        }
        try change(previous.candidate)
        document["schemaVersion"] = previous.document["schemaVersion"]
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
        let needsQualifiedSchema = artwork["schemaVersion"] == Self.qualifiedSchema
        guard needsQualifiedSchema == (artwork["units"] == .string("unknown")) else {
            throw AuthoringFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
        }
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
        c["artworks"] = .array(inventory); c["placements"] = .array(rows)
        if needsQualifiedSchema { c["schemaVersion"] = Self.qualifiedSchema }
        try change(c)
        document["schemaVersion"] = c["schemaVersion"]
    }

    /// Refresh the placed, approved Hayes text revision without changing its
    /// renderable inventory or treating an authored description as a CMS mirror.
    mutating func refreshHayesArtwork(_ artwork: DocumentValue, placementId: String) throws {
        let invalid = AuthoringFailure("ARTWORK_REVISION_CHANGED")
        var inventory = artworks, rows = placements
        guard let i = inventory.firstIndex(where: { $0["id"] == artwork["id"] }),
              let p = rows.firstIndex(where: { $0["id"].string == placementId }) else { throw invalid }
        let prior = inventory[i], previousRevision = prior["revisionId"], nextRevision = artwork["revisionId"]
        guard prior["revision"] == .integer(1), artwork["revision"] == .integer(2),
              prior["schemaVersion"] == Self.qualifiedSchema, prior["artworkType"].string == "sculpture",
              let next = nextRevision.string, UUID(uuidString: next) != nil, nextRevision != previousRevision,
              inventory.filter({ $0["id"] == prior["id"] }).count == 1,
              !inventory.contains(where: { $0["revisionId"] == nextRevision }),
              rows[p]["artworkRevisionId"] == previousRevision, rows[p]["assetId"] == prior["primaryAssetId"],
              rows.filter({ $0["artworkRevisionId"] == previousRevision }).count == 1,
              let beforeDescription = prior["metadata"]["description"].string,
              let description = artwork["metadata"]["description"].string, description != beforeDescription else { throw invalid }

        // The ordinary approval projection changes only these revision fields
        // and the description. Compare the remaining snapshot in full.
        var comparison = artwork
        for key in ["revision", "revisionId", "createdAt", "provenance"] { comparison[key] = prior[key] }
        comparison["extensions"]["org.exhibitos.studio/cms"]["revision"] = prior["extensions"]["org.exhibitos.studio/cms"]["revision"]
        comparison["metadata"]["description"] = prior["metadata"]["description"]
        guard comparison == prior, artwork["provenance"]["authorship"] == prior["provenance"]["authorship"] else { throw invalid }

        var c = candidate
        guard var descriptions = c["accessibility"]["artworkDescriptions"].items,
              descriptions.filter({ $0["placementId"].string == placementId }).count == 1,
              let d = descriptions.firstIndex(where: { $0["placementId"].string == placementId }),
              descriptions[d]["text"].string == beforeDescription else { throw invalid }
        descriptions[d]["text"] = .string(description)
        inventory[i] = artwork
        rows[p]["artworkRevisionId"] = nextRevision
        c["accessibility"]["artworkDescriptions"] = .array(descriptions)
        c["artworks"] = .array(inventory)
        c["placements"] = .array(rows)
        try change(c)
    }

    /// Adopt the next approved snapshot of the same full model. Keep the
    /// existing placement and primary asset, adding only the approved LOD pair.
    public mutating func refreshApprovedCompactArtwork(_ artwork: DocumentValue, placementId: String) throws {
        let invalid = AuthoringFailure("ARTWORK_REVISION_CHANGED")
        var inventory = artworks, rows = placements
        guard let i = inventory.firstIndex(where: { $0["id"] == artwork["id"] }),
              let p = rows.firstIndex(where: { $0["id"].string == placementId }) else { throw invalid }
        let prior = inventory[i], oldRevision = prior["revisionId"], newRevision = artwork["revisionId"]
        let lodKey = "org.exhibitos.viewer/lod", cmsKey = "org.exhibitos.studio/cms"
        let lod = artwork["extensions"][lodKey]
        guard let revision = prior["revision"].integer, (1..<2_147_483_647).contains(revision),
              artwork["revision"].integer == revision + 1,
              let next = newRevision.string, NativeCompactModelVariantRequest.validID(next), newRevision != oldRevision,
              prior["artworkType"] == .string("sculpture"),
              prior["extensions"][lodKey] == .null,
              let fullId = prior["primaryAssetId"].string, artwork["primaryAssetId"].string == fullId,
              rows[p]["artworkRevisionId"] == oldRevision, rows[p]["assetId"].string == fullId,
              rows.filter({ $0["artworkRevisionId"] == oldRevision }).count == 1,
              inventory.filter({ $0["id"] == prior["id"] }).count == 1,
              !inventory.contains(where: { $0["revisionId"] == newRevision }),
              let priorAssets = prior["assets"].items, let assets = artwork["assets"].items,
              assets.count == priorAssets.count + 1, Array(assets.dropLast()) == priorAssets,
              let coarse = assets.last, let coarseId = coarse["id"].string,
              NativeCompactModelVariantRequest.validID(coarseId), coarseId != fullId,
              coarse["mime"] == .string("model/gltf-binary"), coarse["role"] == .string("model"),
              lod["version"] == .integer(1), lod["profile"] == .string(NativeCompactModelVariantRequest.profile),
              let variants = lod["variants"].items, variants.count == 2,
              variants.contains(where: { $0["detail"] == .string("full") && $0["assetId"].string == fullId }),
              variants.contains(where: { $0["detail"] == .string("coarse") && $0["assetId"].string == coarseId }),
              artwork["extensions"][cmsKey]["revision"].integer == revision + 1,
              prior["extensions"][cmsKey]["revision"].integer == revision,
              case .object(var extensions) = artwork["extensions"],
              case .object(var comparison) = artwork,
              case .object(let original) = prior else { throw invalid }
        extensions.removeValue(forKey: lodKey)
        var cms = artwork["extensions"][cmsKey]
        cms["revision"] = .integer(revision)
        extensions[cmsKey] = cms
        comparison["extensions"] = .object(extensions)
        let changing: Set<String> = ["revision", "revisionId", "createdAt", "provenance", "assets"]
        guard comparison.filter({ !changing.contains($0.key) }) == original.filter({ !changing.contains($0.key) }),
              artwork["provenance"]["authorship"] == prior["provenance"]["authorship"] else { throw invalid }

        inventory[i] = artwork
        rows[p]["artworkRevisionId"] = newRevision
        var nextCandidate = candidate
        nextCandidate["artworks"] = .array(inventory)
        nextCandidate["placements"] = .array(rows)
        try change(nextCandidate)
    }

    public mutating func movePlacement(_ id: String, position: [Double]) throws {
        guard position.count == 3, position.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }), var rows = candidate["placements"].items, let i = rows.firstIndex(where: { $0["id"].string == id }) else { throw AuthoringFailure("PLACEMENT_INVALID") }
        rows[i]["transform"]["position"] = .vector(position)
        var c = candidate; c["placements"] = .array(rows); try change(c)
    }

    /// Change only the explicitly reviewed placement alternative text.
    mutating func setArtworkInterpretation(_ id: String, expectedText: String, text: String) throws {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              text.unicodeScalars.count <= 8192,
              placements.filter({ $0["id"].string == id }).count == 1,
              var rows = candidate["accessibility"]["artworkDescriptions"].items,
              rows.filter({ $0["placementId"].string == id }).count == 1,
              let index = rows.firstIndex(where: { $0["placementId"].string == id }),
              case .object(let fields) = rows[index], Set(fields.keys) == ["placementId", "text"],
              rows[index]["text"].string == expectedText else { throw AuthoringFailure("ARTWORK_INTERPRETATION_CHANGED") }
        rows[index]["text"] = .string(text)
        var next = candidate; next["accessibility"]["artworkDescriptions"] = .array(rows); try change(next)
    }

    mutating func rotatePlacement(_ id: String, rotation: [Double]) throws {
        guard rotation.count == 4, rotation.allSatisfy(\.isFinite),
              abs(rotation.reduce(0) { $0 + $1 * $1 } - 1) <= 1e-6,
              var rows = candidate["placements"].items,
              let index = rows.firstIndex(where: { $0["id"].string == id }) else {
            throw AuthoringFailure("PLACEMENT_INVALID")
        }
        rows[index]["transform"]["rotation"] = .vector(rotation)
        var next = candidate
        next["placements"] = .array(rows)
        try acceptBoundedPlacement(next, selection: .init(kind: "placement", id: id), savedBaseline: self)
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
/// A scoped server observation, never authority to issue a write by itself.
public struct NativeBoundedEditCapability: Decodable, Equatable, Sendable {
    public let version: Int
    public let kind: String
    public let tenantId: String
    public let principalId: String
    public let workspaceId: String
    public let exhibitionId: String
    public let revision: Int
    public let etag: String

    public func matches(workspaceId: String, tenantId: String, principalId: String,
                        exhibitionId: String, revision: Int, etag: String) -> Bool {
        let pairs = [(self.workspaceId, workspaceId), (self.tenantId, tenantId),
                     (self.principalId, principalId), (self.exhibitionId, exhibitionId)]
        return version == 1 && kind == "native-bounded-edit-v1" && revision > 0 && self.revision == revision
            && self.etag == etag && !etag.isEmpty
            && pairs.allSatisfy { UUID(uuidString: $0.0) != nil && UUID(uuidString: $0.1) != nil && $0.0.lowercased() == $0.1.lowercased() }
    }
}

/// Server-derived restriction for a specific saved exhibition.
public struct NativeViewportPolicy: Decodable, Sendable {
    public let exhibitionId: String
    public let revision: Int
    public let etag: String
    public let boundedReadOnly: Bool
    /// Absence is unknown, not proof that a draft has no bounded provenance.
    public let boundedProfile: Bool?
    /// Absence is unknown, not an ordinary-size admission.
    public let boundedEditRequired: Bool?
    public let editingCapability: NativeBoundedEditCapability?
    public let workspaceBenchSeatProfile: String?

    /// A positive server observation for a draft outside bounded provenance.
    public var confirmsOrdinaryDraft: Bool {
        boundedProfile == false && boundedEditRequired == false && !boundedReadOnly && editingCapability == nil
    }
}

/// Admission is only for the first clean ordinary-session edit. A capability
/// observation does not authorize a PUT, or resolve any existing pending save.
public enum NativeBoundedEditAdmission {
    public static func admit(policy: NativeViewportPolicy, document: NativeDocumentState,
                             importState: NativeOEXImportState, session: AuthoringSession,
                             selectedImport: Bool) -> NativeBoundedEditCapability? {
        guard !selectedImport, policy.boundedProfile == true, policy.boundedEditRequired == true, !policy.boundedReadOnly,
              !document.dirty, document.pending == nil, let draft = document.draft,
              let etag = document.etag, let capability = policy.editingCapability,
              importState.intent == nil, importState.jobId == nil, importState.fileName == nil,
              importState.workspaceId == document.workspaceId, importState.tenantId == document.tenantId,
              importState.principalId == document.principalId,
              session.tenantId.lowercased() == document.tenantId.lowercased(),
              session.userId.lowercased() == document.principalId.lowercased(),
              policy.exhibitionId.lowercased() == draft.id.lowercased(), policy.etag == etag,
              capability.matches(workspaceId: document.workspaceId, tenantId: document.tenantId,
                                 principalId: document.principalId, exhibitionId: draft.id,
                                 revision: policy.revision, etag: etag) else { return nil }
        return capability
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
public struct NativeApprovedModelCandidateReplacement: Codable, Equatable, Sendable {
    public static let profile = "approved-model-candidate-v1"
    public let artworkId: String
    public let artworkRevision: Int
    public let importJobId: String
    public let placementId: String

    public init(artworkId: String, artworkRevision: Int, importJobId: String, placementId: String) {
        self.artworkId = artworkId
        self.artworkRevision = artworkRevision
        self.importJobId = importJobId
        self.placementId = placementId
    }
}

public struct NativeProductImageReplacement: Codable, Sendable {
    public let artworkId: String
    public let artworkRevision: Int
    public let importJobId: String
    public let receiptSha256: String

    public init(artworkId: String, artworkRevision: Int, importJobId: String, receiptSha256: String) {
        self.artworkId = artworkId
        self.artworkRevision = artworkRevision
        self.importJobId = importJobId
        self.receiptSha256 = receiptSha256
    }
}

public struct NativeDocumentState: Codable, Sendable {
    public struct Pending: Codable, Sendable {
        public let requestId: String
        public let etag: String?
        public let draft: NativeExhibitionDraft
        /// Older records omit this field. Tagged bounded requests must never
        /// enter the generic retry path after a runtime or session change.
        public let origin: String?
        public let modelCandidate: NativeApprovedModelCandidateReplacement?
        public let productImage: NativeProductImageReplacement?
        public init(requestId: String = UUID().uuidString.lowercased(), etag: String?, draft: NativeExhibitionDraft,
                    origin: String? = nil, modelCandidate: NativeApprovedModelCandidateReplacement? = nil,
                    productImage: NativeProductImageReplacement? = nil) {
            self.requestId = requestId; self.etag = etag; self.draft = draft; self.origin = origin
            self.modelCandidate = modelCandidate
            self.productImage = productImage
        }
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
