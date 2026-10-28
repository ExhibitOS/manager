// SPDX-License-Identifier: Apache-2.0
import Foundation

extension ViewportAcceptance {
    public var supportsViewingPlan: Bool { accepted && capabilities?.contains("viewing-plan-v1") == true }
}

/// Draft consumer of the negotiated auxiliary viewing-plan-v1 capability.
/// Ordinary geometry permissions remain unchanged; a missing producer refuses.
public struct ViewingPlanViewportRequest: Sendable {
    public let frame: ViewportFrame
    public let command: ViewingPlanCommand
    public let value: DocumentValue
    public init(frame: ViewportFrame, command: ViewingPlanCommand) throws {
        guard !frame.disabled, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        _ = try command.proposedCandidate(in: frame.draft)
        self.frame = frame; self.command = command
        value = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                         "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId),
                         "requestId": .string(UUID().uuidString.lowercased()), "type": .string("set-viewing-plan"), "plan": command.plan?.value ?? .null])
        guard try JSONEncoder().encode(value).count <= 65_536 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
    }
    public func checkedReply(_ reply: DocumentValue) throws -> ViewingPlanPreview {
        guard case .object(let fields) = reply,
              Set(fields.keys) == Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "candidate"]),
              try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        }
        return try ViewingPlanPreview(candidate: reply["candidate"], base: frame.draft, command: command)
    }
}
