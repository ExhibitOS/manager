// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum NativeAutomationFailure {
    /// Fixed safe diagnostic fields only; never serialize arbitrary Error text,
    /// cookies, tokens, database strings or a transport response body.
    public static func value(_ error: Error) -> DocumentValue {
        if let failure = error as? NativeThumbnailPageFailure {
            var fields: [String: DocumentValue] = [
                "version": .integer(Int64(failure.version.rawValue)),
                "phase": .string(failure.phase.rawValue),
                "outcome": .string(failure.outcome.rawValue),
            ]
            if let step = failure.previewStep { fields["previewStep"] = .string(step.rawValue) }
            return .object(["code": .string("CATALOG_THUMBNAIL_PHASE_FAILED"), "thumbnailFailure": .object(fields)])
        }
        if let uncertain = error as? NativeBoundedOEXUncertain,
           [uncertain.workspaceId, uncertain.tenantId, uncertain.principalId, uncertain.requestId].allSatisfy({ UUID(uuidString: $0) != nil }),
           uncertain.jobId.map({ UUID(uuidString: $0) != nil }) ?? true {
            return .object(["code": .string("AUTOMATION_OEX_UNCERTAIN"), "retryAllowed": .bool(false),
                "retained": .object(["workspaceId": .string(uncertain.workspaceId), "tenantId": .string(uncertain.tenantId),
                    "principalId": .string(uncertain.principalId), "requestId": .string(uncertain.requestId),
                    "jobId": uncertain.jobId.map(DocumentValue.string) ?? .null])])
        }
        if let failure = error as? NativeOEXAutomationFailure {
            var fields: [String: DocumentValue] = ["phase": .string(failure.phase.rawValue), "errorDomain": .string(failure.errorDomain)]
            if let id = failure.operationId { fields["operationId"] = .string(id) }
            if let bytes = failure.receivedBytes { fields["receivedBytes"] = .integer(Int64(bytes)) }
            if let bytes = failure.totalBytes { fields["totalBytes"] = .integer(Int64(bytes)) }
            if let code = failure.errorCode { fields["errorCode"] = .integer(Int64(code)) }
            return .object(["code": .string("AUTOMATION_OEX_EXPORT_FAILED"), "retryAllowed": .bool(false), "oexFailure": .object(fields)])
        }
        let code = (error as? BridgeFailure)?.code ?? (error as? AuthoringFailure)?.code ?? (error is CancellationError ? "AUTOMATION_CANCELLED" : "AUTOMATION_FAILED")
        var result: [String: DocumentValue] = ["code": .string(code)]
        if let failure = error as? AuthoringFailure {
            if let role = failure.role { result["role"] = .string(role) }
            if let field = failure.field { result["field"] = .string(field) }
            if let aim = failure.trackSpotAimDiagnostic {
                result["trackAim"] = .object(["reason": .string(aim.reason), "remedy": .string(aim.remedy)])
            }
        }
        return .object(result)
    }
}
