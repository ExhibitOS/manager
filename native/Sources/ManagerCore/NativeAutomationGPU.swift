// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Owner-scoped create observation sufficient to persist a job ID. In
/// particular, a ready-looking envelope here does not authorize its receipt.
public struct NativeGPUModelJobIdentity: Sendable {
    public let jobId: String
    init(_ value: DocumentValue, input: NativeGPUModelPreparationRequest) throws {
        let n = value["gpuPreparation"], source = n["source"]
        guard case .object(let top) = value, case .object(let nested) = n,
              Set(top.keys).subtracting(["assetId"]) == ["id", "state", "progress", "attempts", "errorCode", "retryAt", "gpuPreparation"],
              Set(nested.keys).subtracting(["candidate", "serverReceipt"]) == ["version", "purpose", "profile", "recipeSHA256", "operationID", "generation", "artworkRevision", "status", "workerDrained", "source"],
              input.valid, let jobId = value["id"].string, NativeGPUModelPreparation.id(jobId),
              ["queued", "processing", "approved", "failed", "cancelled"].contains(value["state"].string ?? ""),
              n["version"] == .integer(1), n["purpose"] == .string(NativeGPUModelPreparation.purpose),
              n["profile"] == .string(NativeGPUModelPreparation.profile), n["recipeSHA256"] == .string(NativeGPUModelPreparation.recipe),
              n["operationID"] == .string(input.idempotencyKey), n["generation"] == .string(input.generation),
              n["artworkRevision"] == .integer(Int64(input.revision)), NativeGPUModelPreparation.source(source),
              NativeGPUModelPreparation.sameID(source["assetId"].string, input.assetId),
              source["sha256"] == .string(input.sha256), source["bytes"] == .integer(Int64(input.bytes)),
              case .bool = n["workerDrained"] else { throw NativeGPUModelPreparation.invalid() }
        self.jobId = jobId
    }
}

/// Closed automation command for the existing owner-scoped GPU job route.
/// The durable attempt is written before create and never invents a new key.
struct NativeAutomationGPUCommand: Sendable {
    let artworkId: String
    let input: NativeGPUModelPreparationRequest
    let jobId: String?

    init(_ value: DocumentValue, creates: Bool) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == (creates ? ["artworkId", "input"] : ["artworkId", "input", "jobId"]),
              let artworkId = value["artworkId"].string, NativeGPUModelPreparation.id(artworkId) else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        let input = try JSONDecoder().decode(NativeGPUModelPreparationRequest.self, from: JSONEncoder().encode(value["input"]))
        let jobId = creates ? nil : value["jobId"].string
        guard creates || NativeGPUModelPreparation.id(jobId) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        self.artworkId = artworkId; self.input = input; self.jobId = jobId
    }

}
