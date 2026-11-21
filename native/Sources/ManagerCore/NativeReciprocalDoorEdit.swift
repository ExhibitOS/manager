// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeDoorPairViolation: Decodable, Sendable {
    public enum Constraint: String, Decodable, Sendable {
        case reference, wallBounds = "wall-bounds", openingOverlap = "opening-overlap", alignment
        case artworkOverlap = "artwork-overlap", frameFit = "frame-fit", frameConflict = "frame-conflict"
        case structureClearance = "structure-clearance"
    }
    public let constraint: Constraint
    public let openingId: String?
    public let roomId: String?

    public init(from decoder: Decoder) throws {
        let value = try DocumentValue(from: decoder)
        let invalid = DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Invalid door-pair diagnostic"))
        guard case .object(let fields) = value,
              Set(fields.keys).isSubset(of: ["constraint", "openingId", "roomId"]),
              let raw = fields["constraint"]?.string, let parsed = Constraint(rawValue: raw) else { throw invalid }
        func identifier(_ key: String) throws -> String? {
            guard let field = fields[key] else { return nil }
            guard let id = field.string, UUID(uuidString: id) != nil else { throw invalid }
            return id
        }
        constraint = parsed
        openingId = try identifier("openingId")
        roomId = try identifier("roomId")
    }

    public func message(in draft: NativeExhibitionDraft?) -> String {
        let room = roomId.flatMap { id in draft?.rooms.first { $0["id"].string?.lowercased() == id.lowercased() }?["name"].string }
        let reason: String
        switch constraint {
        case .reference: reason = "연결된 두 문을 다시 선택하세요."
        case .wallBounds: reason = "문이 벽 경계를 벗어납니다."
        case .openingOverlap: reason = "다른 문이나 창과 겹칩니다."
        case .alignment: reason = "두 문의 중심이나 바닥 높이가 맞지 않습니다."
        case .artworkOverlap: reason = "문 또는 문틀이 작품과 겹칩니다."
        case .frameFit: reason = "문틀이 개구부의 크기나 벽 두께에 맞지 않습니다."
        case .frameConflict: reason = "기존 문틀 또는 벽체 확장과 함께 적용할 수 없습니다."
        case .structureClearance: reason = "벽체 확장에 필요한 여유 공간이 부족합니다."
        }
        return room.map { "\($0)에서 변경할 수 없습니다. \(reason)" } ?? reason
    }
}

public struct NativeReciprocalDoorEdit: Sendable {
    public static let profile = "saved-head-reciprocal-door-edit-v1"
    static let operation = "edit-connected-door-pair"
    static let automationPrepare = "reciprocal-door-prepare"
    static let automationApply = "reciprocal-door-apply"
    static let automationCancel = "reciprocal-door-cancel"
    static let automationOperations: Set<String> = [automationPrepare, automationApply, automationCancel]
    private static let binding = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]

    public struct Dimensions: Sendable {
        public let width: Double, height: Double
        public init(width: Double, height: Double) throws {
            guard width.isFinite, height.isFinite, width > 0, height > 0 else {
                throw AuthoringFailure("ARCHITECTURE_FIELDS_INVALID")
            }
            self.width = width; self.height = height
        }
        fileprivate var value: DocumentValue { .object(["width": .number(width), "height": .number(height)]) }
    }

    public enum Frame: Sendable {
        case keep, select(String), remove
        var changes: Bool { if case .keep = self { false } else { true } }
    }

    public let openingId: String
    public let dimensions: Dimensions?
    public let frame: Frame

    public init(openingId: String, dimensions: Dimensions? = nil, frame: Frame = .keep) throws {
        guard UUID(uuidString: openingId) != nil, dimensions != nil || frame.changes else { throw Self.invalid }
        if case .select(let id) = frame, !workspaceFurnishingID(id) { throw Self.invalid }
        self.openingId = openingId; self.dimensions = dimensions; self.frame = frame
    }

    private static var invalid: AuthoringFailure { AuthoringFailure("NATIVE_AUTHORING_PROPOSAL_INVALID") }
    private static func opening(_ id: String, in draft: NativeExhibitionDraft) -> DocumentValue? {
        draft.candidate["openings"].items?.first { $0["id"].string?.lowercased() == id.lowercased() && $0["type"] == .string("door") }
    }

    public static func isConnectedDoor(_ id: String, in draft: NativeExhibitionDraft) -> Bool {
        opening(id, in: draft)?["connectsToOpeningId"].string != nil
    }

    private static func pairedIDs(_ id: String, in draft: NativeExhibitionDraft) -> [String]? {
        guard let first = opening(id, in: draft), let firstID = first["id"].string,
              let peerID = first["connectsToOpeningId"].string, peerID.lowercased() != firstID.lowercased(),
              let peer = opening(peerID, in: draft), let storedPeer = peer["id"].string,
              peer["connectsToOpeningId"].string?.lowercased() == firstID.lowercased() else { return nil }
        return [firstID, storedPeer].sorted { $0.lowercased() < $1.lowercased() }
    }

    public static func rooms(_ id: String, in draft: NativeExhibitionDraft) -> [String] {
        guard let ids = pairedIDs(id, in: draft) else { return [] }
        return ids.compactMap { openingID in
            let wallID = opening(openingID, in: draft)?["surfaceId"].string
            let roomID = draft.candidate["surfaces"].items?.first { $0["id"].string == wallID }?["roomId"].string
            guard let roomID else { return nil }
            return draft.rooms.first { $0["id"].string == roomID }?["name"].string
        }
    }

    public static func hasFrame(_ id: String, in draft: NativeExhibitionDraft) -> Bool {
        pairedIDs(id, in: draft)?.contains { WorkspacePortalFinish.hasAttachment(openingId: $0, in: draft) } ?? false
    }

    func request(in document: NativeDocumentState, requestId: String) throws -> DocumentValue {
        guard let draft = document.draft, Self.pairedIDs(openingId, in: draft) != nil else {
            throw AuthoringFailure("PORTAL_TARGET_STALE")
        }
        var fields: [String: DocumentValue] = [
            "type": .string(Self.operation), "version": .integer(1), "sequence": .integer(1), "epoch": .string(requestId),
            "requestId": .string(requestId), "tenantId": .string(document.tenantId), "principalId": .string(document.principalId),
            "openingId": .string(openingId)
        ]
        if let dimensions { fields["dimensions"] = dimensions.value }
        switch frame {
        case .keep: break
        case .remove: fields["frame"] = .null
        case .select(let id): fields["frame"] = .object(["furnishingId": .string(id), "furnishingVersion": .integer(1)])
        }
        return .object(fields)
    }

    static func checkAutomationEnvelope(_ operation: String, fields: Set<String>, requestId: String) throws {
        let common: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId",
                                   "principalId", "expectedRevision", "expectedEditVersion"]
        let payload: Set<String>
        switch operation {
        case automationPrepare: payload = ["command"]
        case automationApply: payload = ["proposalId", "retainSaveIntent"]
        case automationCancel: payload = ["proposalId"]
        default: throw BridgeFailure("AUTOMATION_OPERATION_UNSUPPORTED")
        }
        guard fields == common.union(payload), UUID(uuidString: requestId)?.uuidString.lowercased() == requestId else {
            throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID")
        }
    }

    static func automationCommand(_ value: DocumentValue) throws -> Self {
        guard case .object(let fields) = value,
              Set(fields.keys).isSubset(of: ["openingId", "dimensions", "frame", "etag"]),
              let opening = value["openingId"].string, value["etag"].string != nil else { throw Self.invalid }
        var size: Dimensions?
        if let dimensions = fields["dimensions"] {
            guard case .object(let fields) = dimensions, Set(fields.keys) == ["width", "height"],
                  let width = dimensions["width"].double, let height = dimensions["height"].double else { throw Self.invalid }
            size = try Dimensions(width: width, height: height)
        }
        let frame: Frame
        switch fields["frame"] {
        case nil: frame = .keep
        case .some(.null): frame = .remove
        case .some(.object(let frameFields)):
            guard Set(frameFields.keys) == ["furnishingId", "furnishingVersion"],
                  frameFields["furnishingVersion"] == .integer(1), let id = frameFields["furnishingId"]?.string else { throw Self.invalid }
            frame = .select(id)
        default: throw Self.invalid
        }
        return try .init(openingId: opening, dimensions: size, frame: frame)
    }

    func checkedReply(_ response: DocumentValue, request: DocumentValue, draft: NativeExhibitionDraft,
                      approvedFrame: WorkspaceFurnishing?) throws -> NativeExhibitionDraft? {
        guard case .object(let fields) = response, Set(fields.keys) == Set(Self.binding + ["openingIds", "candidate"]),
              Self.binding.allSatisfy({ response[$0] == request[$0] }),
              let ids = Self.pairedIDs(openingId, in: draft), response["openingIds"] == .array(ids.map(DocumentValue.string)),
              try JSONEncoder().encode(response).count <= 1_100_000 else { throw Self.invalid }
        let observed = response["candidate"]
        var expected = draft.candidate
        if let dimensions {
            guard var rows = expected["openings"].items else { throw Self.invalid }
            for i in rows.indices where ids.contains(rows[i]["id"].string ?? "") {
                guard let offsets = rows[i]["offset"].items, offsets.count == 2,
                      let oldY = offsets[1].double, let oldHeight = rows[i]["dimensions"]["height"].double else {
                    throw Self.invalid
                }
                let nextY = oldY - oldHeight / 2 + dimensions.height / 2
                rows[i]["dimensions"] = dimensions.value; rows[i]["offset"] = .array([offsets[0], .number(nextY)])
            }
            expected["openings"] = .array(rows)
        }
        if frame.changes {
            let ns = WorkspaceFurnishingPlacement.namespace
            let updated = observed["extensions"][ns]
            try checkFrame(updated, prior: expected["extensions"][ns], pair: ids, approved: approvedFrame)
            if updated != expected["extensions"][ns] {
                var extensions: [String: DocumentValue] = [:]
                if case .object(let fields) = expected["extensions"] { extensions = fields }
                extensions[ns] = updated == .null ? nil : updated
                expected["extensions"] = .object(extensions)
            }
        }
        guard expected == observed else { throw Self.invalid }
        guard expected != draft.candidate else { return nil }
        var updated = draft
        try updated.replaceCandidate(expected)
        return updated
    }

    private func checkFrame(_ next: DocumentValue, prior: DocumentValue, pair: [String], approved: WorkspaceFurnishing?) throws {
        let originalRows = prior["openingAttachments"].items ?? []
        let newRows = next["openingAttachments"].items ?? []
        let IDs = Set(pair.map { $0.lowercased() })
        let selected: (DocumentValue) -> Bool = { IDs.contains($0["openingId"].string?.lowercased() ?? "") }
        let previous = originalRows.filter(selected), changed = newRows.filter(selected)
        guard previous.count <= 1, originalRows.filter({ !selected($0) }) == newRows.filter({ !selected($0) }) else { throw Self.invalid }
        let placed = prior["placements"].items ?? [], frames = prior["frameAttachments"].items ?? []
        let ceilings = prior["ceilingAttachments"].items ?? []
        var expectedAssets = prior["furnishings"].items ?? []
        let unrelated = newRows.filter { !selected($0) } + placed + frames
        if let removed = previous.first?["furnishingId"].string,
           !unrelated.contains(where: { $0["furnishingId"].string?.lowercased() == removed.lowercased() }) {
            expectedAssets.removeAll { $0["id"].string?.lowercased() == removed.lowercased() && $0["kind"] == .string("portal-finish") }
        }
        var expectedVersion = prior["version"].integer ?? 6
        switch frame {
        case .keep: throw Self.invalid
        case .remove:
            guard changed.isEmpty else { throw Self.invalid }
        case .select(let id):
            guard let approved, approved.id == id, approved.version == 1, approved.details.kind == .portalFinish, changed.count == 1,
                  changed[0] == .object(["openingId": changed[0]["openingId"], "furnishingId": .string(id),
                       "furnishingVersion": .integer(1), "modelSha256": .string(approved.details.model.sha256)]) else { throw Self.invalid }
            let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(approved))
            let matching = expectedAssets.filter { $0["id"].string == id }
            guard matching.isEmpty || matching == [descriptor] else { throw Self.invalid }
            if matching.isEmpty { expectedAssets.append(descriptor) }
            expectedVersion = max(expectedVersion, approved.fit == nil ? 6 : 7)
        }
        if expectedVersion == 6 && newRows.isEmpty { expectedVersion = 5 }
        if next == .null {
            guard expectedAssets.isEmpty, placed.isEmpty, frames.isEmpty, ceilings.isEmpty, newRows.isEmpty else { throw Self.invalid }
            return
        }
        guard case .object(let fields) = next, let version = next["version"].integer,
              next["furnishings"].items == expectedAssets, next["placements"].items == placed,
              (next["frameAttachments"].items ?? []) == frames, (next["ceilingAttachments"].items ?? []) == ceilings else { throw Self.invalid }
        if next == prior { return }
        let common: Set<String> = ["version", "furnishings", "placements", "frameAttachments"]
        let keys = common.union(version >= 6 ? ["openingAttachments"] : []).union(version == 11 ? ["ceilingAttachments"] : [])
        guard version == expectedVersion, [5, 6, 7, 9, 10, 11].contains(version), Set(fields.keys) == keys else { throw Self.invalid }
    }
}
