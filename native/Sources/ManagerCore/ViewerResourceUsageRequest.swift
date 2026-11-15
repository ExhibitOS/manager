// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Optional live Native-editor observation, separate from the Server's target
/// Viewer analysis. The shared renderer evaluates every status and limit.
public struct ViewerResourceUsageRequest: Sendable {
    public static let capability = "viewer-resource-usage-v1"
    public let frame: ViewportFrame
    public let value: DocumentValue

    public init(frame: ViewportFrame, requestId: String = UUID().uuidString.lowercased()) throws {
        guard frame.sequence > 0, frame.sequence <= 9_007_199_254_740_991,
              [frame.epoch, frame.tenantId, frame.principalId, requestId].allSatisfy({ UUID(uuidString: $0) != nil }) else {
            throw AuthoringFailure("RESOURCE_USAGE_STALE")
        }
        self.frame = frame
        value = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
            "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(requestId)])
    }

    public func checkedReply(_ response: DocumentValue) throws -> DocumentValue {
        let bindings = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
        guard case .object(let fields) = response, Set(fields.keys).isSuperset(of: Set(bindings + ["candidateSha256", "policy", "observations", "evaluation"])),
              bindings.allSatisfy({ response[$0] == value[$0] }),
              let digest = response["candidateSha256"].string, nativeImportDigest(digest) else { throw AuthoringFailure("RESOURCE_USAGE_STALE") }
        if response["observations"] != .null {
            guard response["observations"]["scope"] == .string("mounted-scene"),
                  case .bool = response["observations"]["complete"] else { throw AuthoringFailure("RESOURCE_USAGE_INVALID") }
        }
        if response["policy"] != .null {
            guard response["policy"]["policyVersion"] == .integer(1), response["policy"]["accountingVersion"] == .integer(1),
                  response["policy"]["profile"] == .string("native") else { throw AuthoringFailure("RESOURCE_USAGE_INVALID") }
        }
        if response["evaluation"] != .null {
            guard response["policy"] != .null, response["observations"] != .null,
                  response["evaluation"]["policyVersion"] == .integer(1), response["evaluation"]["accountingVersion"] == .integer(1),
                  response["evaluation"]["profile"] == .string("native"),
                  response["evaluation"]["scope"] == .string("analyzedSection"),
                  ["within", "warning", "admission-exceeded", "unknown"].contains(response["evaluation"]["status"].string ?? "") else { throw AuthoringFailure("RESOURCE_USAGE_INVALID") }
        }
        return response
    }
}
