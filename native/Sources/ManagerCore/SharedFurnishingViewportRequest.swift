// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct SharedFurnishingViewportRequest: Sendable {
    public let frame: ViewportFrame, command: SharedFurnishingCommand, value: DocumentValue
    public init(frame: ViewportFrame, command: SharedFurnishingCommand, selectedInspectorId: String?) throws {
        guard !frame.disabled, frame.sequence > 0, [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        switch command.host {
        case .pedestal:
            guard frame.selection == ViewportSelection(kind: "placement", id: command.hostId), frame.selection?.belongs(to: frame.draft) == true else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        case .opening:
            guard frame.selection == ViewportSelection(kind: "opening", id: command.hostId), frame.selection?.belongs(to: frame.draft) == true else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        case .bench, .light:
            guard selectedInspectorId == command.hostId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        }
        guard case .object(let payload) = command.value else { throw AuthoringFailure("FURNISHING_INVALID") }
        var fields: [String: DocumentValue] = ["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased())]
        fields.merge(payload) { _, new in new }; self.frame = frame; self.command = command; value = .object(fields)
    }
    public func checkedReply(_ reply: DocumentValue) throws -> SharedFurnishingPreview {
        let keys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
        guard case .object(let fields) = reply, Set(fields.keys) == keys.union(["candidate"]), try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in keys { guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") } }
        return try SharedFurnishingPreview(candidate: reply["candidate"], base: frame.draft, command: command)
    }
}
