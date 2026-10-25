// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum ViewportPlacementPhase: String, Equatable, Sendable { case begin, commit, cancel }

public struct ViewportPlacementTarget: Equatable, Sendable {
    public let placementRequestId: String
    public let valid: Bool
    public let reason: String
    public let token: String?
    public let surfaceId: String?
    public let offset: [Double]?
    public init(value: DocumentValue, intent: ArtworkPlacementIntent) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == Set(["placementRequestId", "targetToken", "surfaceId", "offset", "valid", "reason"]),
              value["placementRequestId"].string == intent.requestId,
              case .bool(let valid) = value["valid"], let reason = value["reason"].string, reason.utf8.count <= 160 else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
        placementRequestId = intent.requestId; self.valid = valid; self.reason = reason
        if valid {
            guard reason.isEmpty, let token = value["targetToken"].string, UUID(uuidString: token) != nil,
                  let id = value["surfaceId"].string, UUID(uuidString: id) != nil,
                  let surface = intent.base.candidate["surfaces"].items?.first(where: { $0["id"].string == id }),
                  surface["roomId"].string == intent.roomId, surface["type"].string == intent.targetKind.rawValue,
                  let raw = value["offset"].items, raw.count == 2, raw.allSatisfy({ $0.double.map { $0.isFinite && abs($0) <= 1_000_000 } == true }) else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
            self.token = token; surfaceId = id; offset = raw.map { $0.double! }
        } else {
            guard !reason.isEmpty, value["targetToken"] == .null, value["surfaceId"] == .null, value["offset"] == .null else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
            token = nil; surfaceId = nil; offset = nil
        }
    }
}

/// Optional capability: closed auxiliary requests do not extend the v1 Frame or
/// grant the general geometry receiver permission to replace artwork inventory.
public struct ViewportPlacementRequest: Sendable {
    public let value: DocumentValue
    public let frame: ViewportFrame
    public let intent: ArtworkPlacementIntent
    public let phase: ViewportPlacementPhase
    public let target: ViewportPlacementTarget?
    public init(frame: ViewportFrame, intent: ArtworkPlacementIntent, phase: ViewportPlacementPhase, target: ViewportPlacementTarget? = nil) throws {
        guard !frame.disabled, frame.sequence > 0, frame.presentation == .threeD, frame.draft == intent.base,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
              intent.artwork["extensions"]["org.exhibitos.studio/cms"]["tenantId"].string == frame.tenantId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        var row: DocumentValue = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(phase == .begin ? intent.requestId : UUID().uuidString.lowercased()), "phase": .string(phase.rawValue)])
        if phase == .begin {
            row["artworkId"] = .string(intent.artworkId); row["expectedRevision"] = .integer(Int64(intent.expectedRevision))
            row["placementId"] = .string(intent.placementId); row["roomId"] = .string(intent.roomId); row["targetKind"] = .string(intent.targetKind.rawValue)
        } else {
            row["placementRequestId"] = .string(intent.requestId)
            if phase == .commit {
                guard let target, target.placementRequestId == intent.requestId, target.valid, let token = target.token else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
                row["targetToken"] = .string(token)
            }
        }
        value = row; self.frame = frame; self.intent = intent; self.phase = phase; self.target = target
    }
    public func checkedReply(_ reply: DocumentValue) throws -> DocumentValue? {
        var keys = Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "phase", "placementRequestId", "accepted"])
        if phase == .commit { keys.formUnion(["targetToken", "validatedPlacement"]) }
        guard case .object(let fields) = reply, Set(fields.keys) == keys,
              reply["accepted"] == .bool(true), reply["placementRequestId"].string == intent.requestId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "phase"] {
            guard reply[key] == value[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        }
        guard phase == .commit else { return nil }
        guard let target, reply["targetToken"].string == target.token, let surface = target.surfaceId, let offset = target.offset else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
        _ = try intent.applying(reply["validatedPlacement"], surfaceId: surface, offset: offset, to: frame.draft)
        return reply["validatedPlacement"]
    }
}
