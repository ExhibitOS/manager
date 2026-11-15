// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A closed policy-only command. The shared Server producer remains the owner
/// of validation and policy decisions; the client verifies an exact delta.
public struct ViewerBudgetPolicyRequest: Sendable {
    public static let pendingOrigin = "viewer-budget-policy-v1"

    /// This only identifies a retained intent; authenticated server CAS remains
    /// authoritative and the saved baseline is checked again before replay.
    public static func isPending(in state: NativeDocumentState) -> Bool {
        guard let pending = state.pending, pending.origin == pendingOrigin,
              pending.modelCandidate == nil, pending.productImage == nil,
              pending.etag != nil, pending.etag == state.etag,
              pending.draft == state.draft, let saved = state.savedEditVersion,
              saved >= 1, saved < 2_147_483_647 else { return false }
        return pending.draft.document["editVersion"].integer == saved + 1
    }

    public static func requirePolicyOnlyChange(from saved: NativeExhibitionDraft,
                                             to proposed: NativeExhibitionDraft) throws {
        guard case .object(var before) = saved.document, case .object(var after) = proposed.document,
              let version = before["editVersion"]?.integer, version < 2_147_483_647,
              after["editVersion"]?.integer == version + 1 else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_STALE") }
        _ = try ViewerBudgetSettings(candidate: proposed.candidate)
        func withoutPolicy(_ candidate: DocumentValue) throws -> DocumentValue {
            guard case .object(var fields) = candidate else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_STALE") }
            if let value = fields["extensions"] {
                guard case .object(var extensions) = value else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_STALE") }
                extensions.removeValue(forKey: ViewerBudgetSettings.namespace)
                if extensions.isEmpty { fields.removeValue(forKey: "extensions") }
                else { fields["extensions"] = .object(extensions) }
            }
            return .object(fields)
        }
        guard try withoutPolicy(saved.candidate) == withoutPolicy(proposed.candidate) else {
            throw AuthoringFailure("VIEWER_BUDGET_POLICY_STALE")
        }
        for key in ["candidate", "editVersion", "updatedAt"] { before.removeValue(forKey: key); after.removeValue(forKey: key) }
        guard before == after else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_STALE") }
    }

    public let frame: ViewportFrame
    public let value: DocumentValue
    public let candidate: DocumentValue
    public init(frame: ViewportFrame, command: DocumentValue,
                requestId: String = UUID().uuidString.lowercased()) throws {
        guard !frame.disabled, frame.sequence > 0, frame.sequence <= 9_007_199_254_740_991,
              [frame.epoch, frame.tenantId, frame.principalId, requestId].allSatisfy({ UUID(uuidString: $0) != nil }),
              case .object(let fields) = command, Set(fields.keys) == ["type", "policy"],
              command["type"] == .string("set-viewer-budget-policy"),
              case .object(var next) = frame.draft.candidate else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_INVALID") }
        let namespace = ViewerBudgetSettings.namespace
        var policy = command["policy"]
        if policy != .null {
            _ = try ViewerBudgetSettings(candidate: .object(["extensions": .object([namespace: policy])]))
            guard case .object(var values) = policy, case .object(let profiles) = values["profiles"] else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_INVALID") }
            if values["targetProfile"] == .string("auto") { values.removeValue(forKey: "targetProfile") }
            policy = profiles.isEmpty && values["targetProfile"] == nil ? .null : .object(values)
        }
        var extensions: [String: DocumentValue]
        if case .object(let existing) = next["extensions"] { extensions = existing }
        else if next["extensions"] == nil { extensions = [:] }
        else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_INVALID") }
        if policy == .null { extensions.removeValue(forKey: namespace) } else { extensions[namespace] = policy }
        if extensions.isEmpty { next.removeValue(forKey: "extensions") } else { next["extensions"] = .object(extensions) }
        self.frame = frame; candidate = .object(next)
        value = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
            "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(requestId),
            "type": command["type"], "policy": command["policy"]])
    }
    public func checkedReply(_ response: DocumentValue) throws -> ViewerBudgetPolicyRequest {
        let keys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
        guard case .object(let fields) = response, Set(fields.keys) == keys.union(["candidate"]),
              keys.allSatisfy({ response[$0] == value[$0] }), response["candidate"] == candidate,
              try JSONEncoder().encode(response).count <= 1_100_000 else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_STALE") }
        return self
    }
    public func apply(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
        guard draft == frame.draft else { throw AuthoringFailure("VIEWER_BUDGET_POLICY_STALE") }
        var next = draft
        if candidate != draft.candidate { try next.replaceCandidate(candidate) }
        return next
    }
}
