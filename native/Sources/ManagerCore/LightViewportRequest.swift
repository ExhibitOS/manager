// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Optional light-inspector-v1 only. The old frame/selection/action remains v1.
public struct LightViewportRequest: Sendable {
    public let frame: ViewportFrame
    public let command: LightCommand
    public let value: DocumentValue
    public init(frame: ViewportFrame, command: LightCommand, selectedLightId: String?) throws {
        guard !frame.disabled, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else {
            throw AuthoringFailure("VIEWPORT_FRAME_STALE")
        }
        value = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                         "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId),
                         "requestId": .string(UUID().uuidString.lowercased()), "type": .string("set-light"),
                         "lightId": .string(command.lightId), "light": try command.encodedLight(in: frame.draft, selectedLightId: selectedLightId)])
        guard try JSONEncoder().encode(value).count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        self.frame = frame; self.command = command
    }
    public func checkedReply(_ reply: DocumentValue, selectedLightId: String?) throws -> LightPreview {
        guard case .object(let fields) = reply,
              Set(fields.keys) == Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "lightId", "candidate"]),
              try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "lightId"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        }
        return try LightPreview(candidate: reply["candidate"], base: frame.draft, command: command, selectedLightId: selectedLightId)
    }
}
