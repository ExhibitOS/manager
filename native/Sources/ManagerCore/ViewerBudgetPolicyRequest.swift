// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A closed policy-only command. The shared Server producer remains the owner
/// of validation and policy decisions; the client verifies an exact delta.
public struct ViewerBudgetPolicyRequest: Sendable {
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
