// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativePairedWallStructure: Equatable, Sendable {
    public enum Side: String, Sendable {
        case positive = "positive-normal"
        case negative = "negative-normal"
    }

    public struct Target: Equatable, Identifiable, Sendable {
        public var id: String { openingId }
        public let openingId: String
        public let peerOpeningId: String
        public let peerRoomName: String
        public let side: Side
        public let extensionMeters: Double?
    }

    public static let profile = "saved-head-paired-wall-structure-v1"
    public static let maximumExtensionMeters = 0.12
    private static let namespace = "org.exhibitos.studio/paired-wall-structure"
    static let set = "set-paired-wall-structure"
    static let remove = "remove-paired-wall-structure"
    private static let binding = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]

    public let target: Target
    public let extensionMeters: Double?

    public init(target: Target, extensionMeters: Double?) throws {
        if let extensionMeters {
            guard extensionMeters.isFinite, extensionMeters > 0,
                  extensionMeters <= Self.maximumExtensionMeters else { throw Self.invalid() }
        } else if target.extensionMeters == nil {
            throw Self.invalid()
        }
        self.target = target
        self.extensionMeters = extensionMeters
    }

    private static func matches(_ value: DocumentValue, _ id: String) -> Bool {
        value.string?.lowercased() == id.lowercased()
    }

    public static func targets(in draft: NativeExhibitionDraft, surfaceId: String) -> [Target] {
        let scene = draft.candidate
        let walls = scene["surfaces"].items ?? []
        let openings = scene["openings"].items ?? []
        let finishes = scene["extensions"][WallFinishSettings.namespace]["walls"].items ?? []
        guard let wall = walls.first(where: { matches($0["id"], surfaceId) && $0["type"] == .string("wall") }),
              finishes.contains(where: { matches($0["surfaceId"], surfaceId) }),
              let side = inwardSide(wall),
              let existing = try? pairs(in: scene) else { return [] }

        return openings.compactMap { opening in
            guard opening["type"] == .string("door"), matches(opening["surfaceId"], surfaceId),
                  let id = opening["id"].string, let peerId = opening["connectsToOpeningId"].string,
                  [id, peerId].allSatisfy({ UUID(uuidString: $0) != nil }), id.lowercased() != peerId.lowercased(),
                  let peer = openings.first(where: { matches($0["id"], peerId) && $0["type"] == .string("door") }),
                  matches(peer["connectsToOpeningId"], id),
                  let peerWallId = peer["surfaceId"].string,
                  let peerWall = walls.first(where: { matches($0["id"], peerWallId) && $0["type"] == .string("wall") }),
                  wall["roomId"] != peerWall["roomId"],
                  finishes.contains(where: { matches($0["surfaceId"], peerWallId) }),
                  let room = draft.rooms.first(where: { $0["id"] == peerWall["roomId"] }),
                  let roomName = room["name"].string,
                  !existing.contains(where: { matches($0["openingId"], peerId) }) else { return nil }
            let installed = existing.first(where: { matches($0["openingId"], id) && matches($0["peerOpeningId"], peerId) })
            return Target(openingId: id, peerOpeningId: peerId, peerRoomName: roomName,
                          side: side, extensionMeters: installed?["extensionMeters"].double)
        }
    }

    // The wall transform is room-local. Its rotation maps local +Z into the room.
    // The server alone admits the actual geometry and clearances.
    private static func inwardSide(_ wall: DocumentValue) -> Side? {
        guard let rotation = wall["transform"]["rotation"].items?.compactMap(\.double), rotation.count == 4,
              let position = wall["transform"]["position"].items?.compactMap(\.double), position.count == 3 else { return nil }
        let x = rotation[0], y = rotation[1], z = rotation[2], w = rotation[3]
        let normal = [2 * (x * z + w * y), 2 * (y * z - w * x), 1 - 2 * (x * x + y * y)]
        let inward = -zip(normal, position).reduce(0.0) { $0 + $1.0 * $1.1 }
        guard inward.isFinite, abs(inward) > 0.00001 else { return nil }
        return inward > 0 ? .positive : .negative
    }

    private static func invalid() -> AuthoringFailure { AuthoringFailure("PAIRED_WALL_STRUCTURE_INVALID") }

    private static func pairs(in scene: DocumentValue) throws -> [DocumentValue] {
        guard case .object(let extensions) = scene["extensions"] else { throw invalid() }
        guard let extensionValue = extensions[namespace] else { return [] }
        guard case .object(let fields) = extensionValue, Set(fields.keys) == ["version", "pairs"],
              fields["version"] == .integer(1), let pairs = fields["pairs"]?.items,
              (1...10).contains(pairs.count) else { throw invalid() }
        return pairs
    }

    func request(in document: NativeDocumentState, requestId: String) -> DocumentValue {
        var fields: [String: DocumentValue] = [
            "type": .string(extensionMeters == nil ? Self.remove : Self.set),
            "version": .integer(1), "sequence": .integer(1), "epoch": .string(requestId),
            "requestId": .string(requestId), "tenantId": .string(document.tenantId),
            "principalId": .string(document.principalId),
            "openingId": .string(target.openingId), "peerOpeningId": .string(target.peerOpeningId)
        ]
        if let extensionMeters {
            fields["side"] = .string(target.side.rawValue)
            fields["extensionMeters"] = .number(extensionMeters)
        }
        return .object(fields)
    }

    func checkedReply(_ reply: DocumentValue, request: DocumentValue, draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft? {
        guard case .object(let fields) = reply, Set(fields.keys) == Set(Self.binding + ["candidate"]),
              Self.binding.allSatisfy({ reply[$0] == request[$0] }),
              try JSONEncoder().encode(reply).count <= 1_100_000,
              case .object(var extensions) = draft.candidate["extensions"] else { throw Self.invalid() }
        var pairs = try Self.pairs(in: draft.candidate)
        let index = pairs.firstIndex {
            Self.matches($0["openingId"], target.openingId) && Self.matches($0["peerOpeningId"], target.peerOpeningId)
        }
        if let extensionMeters {
            let row: DocumentValue = .object([
                "openingId": .string(target.openingId), "peerOpeningId": .string(target.peerOpeningId),
                "side": .string(target.side.rawValue), "extensionMeters": .number(extensionMeters)
            ])
            if let index { pairs[index] = row } else { pairs.append(row) }
        } else {
            guard let index else { throw Self.invalid() }
            pairs.remove(at: index)
        }
        extensions[Self.namespace] = pairs.isEmpty ? nil : .object(["version": .integer(1), "pairs": .array(pairs)])
        var expected = draft.candidate
        expected["extensions"] = .object(extensions)
        guard expected == reply["candidate"] else { throw Self.invalid() }
        guard expected != draft.candidate else { return nil }
        var next = draft
        try next.replaceCandidate(expected)
        return next
    }
}
