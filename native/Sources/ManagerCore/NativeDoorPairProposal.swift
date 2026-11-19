// SPDX-License-Identifier: Apache-2.0
import Foundation

struct NativeDoorPairProposal {
    static let profile = "saved-head-rectangular-door-pair-v1"
    static let command = "add-connected-door-pair"
    private static let binding = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
    let value: DocumentValue
    private let draft: NativeExhibitionDraft
    private let walls: [DocumentValue]
    private let offsets: [DocumentValue]

    init(command: DocumentValue, draft: NativeExhibitionDraft, tenantId: String, principalId: String, requestId: String) throws {
        guard case .object(var fields) = command, Set(fields.keys) == ["dimensions", "first", "second"],
              case .object(let dimensions) = command["dimensions"], Set(dimensions.keys) == ["width", "height"],
              dimensions.values.allSatisfy({ $0.double.map { $0.isFinite && $0 > 0 } == true }),
              [tenantId, principalId, requestId].allSatisfy({ UUID(uuidString: $0) != nil }) else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        let sides = [command["first"], command["second"]]
        let surfaces = draft.candidate["surfaces"].items ?? []
        walls = try sides.map { side in
            guard case .object(let sideFields) = side, Set(sideFields.keys) == ["surfaceId", "offset"],
                  let id = side["surfaceId"].string, UUID(uuidString: id)?.uuidString.lowercased() == id,
                  let offset = side["offset"].items, offset.count == 2,
                  offset.allSatisfy({ $0.double?.isFinite == true }),
                  let wall = surfaces.first(where: { $0["id"] == side["surfaceId"] && $0["type"] == .string("wall") }) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            return wall
        }
        guard walls[0]["roomId"] != walls[1]["roomId"] else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        offsets = sides.map { $0["offset"] }
        fields["type"] = .string(Self.command)
        fields["version"] = .integer(1)
        fields["sequence"] = .integer(1)
        fields["epoch"] = .string(requestId)
        fields["requestId"] = .string(requestId)
        fields["tenantId"] = .string(tenantId)
        fields["principalId"] = .string(principalId)
        value = .object(fields)
        self.draft = draft
    }

    func checkedReply(_ reply: DocumentValue) throws -> (draft: NativeExhibitionDraft, openingIds: DocumentValue) {
        let error = AuthoringFailure("NATIVE_AUTHORING_PROPOSAL_INVALID")
        guard case .object(let fields) = reply,
              Set(fields.keys) == Set(Self.binding + ["openingIds", "candidate"]),
              Self.binding.allSatisfy({ reply[$0] == value[$0] }),
              let ids = reply["openingIds"].items, ids.count == 2, ids[0] != ids[1],
              try JSONEncoder().encode(reply).count <= 1_100_000 else { throw error }
        var expected = draft.candidate
        guard var openings = expected["openings"].items, ids.allSatisfy({ id in
            guard let value = id.string, UUID(uuidString: value)?.uuidString.lowercased() == value else { return false }
            return id != draft.document["id"] && NativePrivateHostArchitecture.unique(value, candidate: expected)
        }) else { throw error }
        for index in ids.indices {
            openings.append(.object(["type": .string("door"), "id": ids[index], "surfaceId": walls[index]["id"],
                "dimensions": value["dimensions"], "offset": offsets[index], "connectsToOpeningId": ids[1-index]]))
        }
        expected["openings"] = .array(openings)
        try NativePrivateHostArchitecture.markTemplate(&expected)
        guard expected == reply["candidate"] else { throw error }
        var staged = draft
        try staged.replaceCandidate(expected)
        return (staged, reply["openingIds"])
    }
}
