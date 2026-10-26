// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct PlaqueViewportRequest: Sendable {
    public let frame: ViewportFrame
    public let command: PlaqueCommand
    public let value: DocumentValue
    public init(frame: ViewportFrame, command: PlaqueCommand, selectedPlaqueId: String?) throws {
        guard !frame.disabled, frame.sequence > 0, [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ BenchSettings.uuid($0) }),
              case .object(var fields) = command.value else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        _ = try command.proposedCandidate(in: frame.draft, selectedPlaqueId: selectedPlaqueId)
        fields["version"] = .integer(1); fields["epoch"] = .string(frame.epoch); fields["sequence"] = .integer(Int64(frame.sequence))
        fields["tenantId"] = .string(frame.tenantId); fields["principalId"] = .string(frame.principalId); fields["requestId"] = .string(UUID().uuidString.lowercased())
        value = .object(fields); self.frame = frame; self.command = command
        guard try JSONEncoder().encode(value).count <= 65_536 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
    }
    public func checkedReply(_ reply: DocumentValue, selectedPlaqueId: String?) throws -> PlaquePreview {
        guard case .object(let fields) = reply, Set(fields.keys) == ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "plaqueId", "candidate"],
              reply["plaqueId"].string == command.plaqueId, try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        }
        return try PlaquePreview(candidate: reply["candidate"], base: frame.draft, command: command, selectedPlaqueId: selectedPlaqueId)
    }
}
