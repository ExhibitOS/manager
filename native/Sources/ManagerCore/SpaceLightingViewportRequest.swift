// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Optional space-settings-v1; ordinary geometry and light-inspector-v1 stay closed.
public struct SpaceLightingViewportRequest: Sendable {
    public let frame: ViewportFrame
    public let command: SpaceLightingCommand
    public let value: DocumentValue
    public init(frame: ViewportFrame, command: SpaceLightingCommand) throws {
        guard !frame.disabled, frame.sequence > 0, [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        self.frame = frame; self.command = command
        value = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased()), "type": .string("set-space-lighting"), "settings": command.settings])
        guard try JSONEncoder().encode(value).count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
    }
    public func checkedReply(_ reply: DocumentValue) throws -> SpaceLightingPreview {
        guard case .object(let fields) = reply, Set(fields.keys) == Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "candidate"]), try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") } }
        return try SpaceLightingPreview(candidate: reply["candidate"], base: frame.draft, command: command)
    }
}
