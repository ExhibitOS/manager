// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum NativeAutomationFailure {
    /// Fixed safe diagnostic fields only; never serialize arbitrary Error text,
    /// cookies, tokens, database strings or a transport response body.
    public static func value(_ error: Error) -> DocumentValue {
        if let uncertain = error as? NativeBoundedOEXUncertain,
           [uncertain.workspaceId, uncertain.tenantId, uncertain.principalId, uncertain.requestId].allSatisfy({ UUID(uuidString: $0) != nil }),
           uncertain.jobId.map({ UUID(uuidString: $0) != nil }) ?? true {
            return .object(["code": .string("AUTOMATION_OEX_UNCERTAIN"), "retryAllowed": .bool(false),
                "retained": .object(["workspaceId": .string(uncertain.workspaceId), "tenantId": .string(uncertain.tenantId),
                    "principalId": .string(uncertain.principalId), "requestId": .string(uncertain.requestId),
                    "jobId": uncertain.jobId.map(DocumentValue.string) ?? .null])])
        }
        let code = (error as? BridgeFailure)?.code ?? (error as? AuthoringFailure)?.code ?? (error is CancellationError ? "AUTOMATION_CANCELLED" : "AUTOMATION_FAILED")
        var result: [String: DocumentValue] = ["code": .string(code)]
        if let failure = error as? AuthoringFailure {
            if let role = failure.role { result["role"] = .string(role) }
            if let field = failure.field { result["field"] = .string(field) }
        }
        return .object(result)
    }
}
