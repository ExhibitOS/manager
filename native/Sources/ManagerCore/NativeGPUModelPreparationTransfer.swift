// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Durable identity only: never stores credentials, model bytes or a local path.
public struct NativeGPUModelPreparationTransfer: Codable, Equatable, Sendable {
    public let version: Int
    public let workspaceId: String, tenantId: String, principalId: String, artworkId: String
    public let request: NativeGPUModelPreparationRequest
    public let jobId: String?
    enum CodingKeys: String, CodingKey { case version, workspaceId, tenantId, principalId, artworkId, request, jobId }
    public init(from decoder: Decoder) throws {
        let raw = try decoder.container(keyedBy: NativeGPUModelCodingKey.self)
        let keys = Set(raw.allKeys.map(\.stringValue))
        guard keys.subtracting(["jobId"]) == ["version", "workspaceId", "tenantId", "principalId", "artworkId", "request"] else { throw NativeGPUModelPreparation.invalid() }
        let v = try decoder.container(keyedBy: CodingKeys.self)
        guard try v.decode(Int.self, forKey: .version) == 1 else { throw NativeGPUModelPreparation.invalid() }
        if keys.contains("jobId"), try v.decodeNil(forKey: .jobId) { throw NativeGPUModelPreparation.invalid() }
        try self.init(workspaceId: v.decode(String.self, forKey: .workspaceId), tenantId: v.decode(String.self, forKey: .tenantId), principalId: v.decode(String.self, forKey: .principalId), artworkId: v.decode(String.self, forKey: .artworkId), request: v.decode(NativeGPUModelPreparationRequest.self, forKey: .request), jobId: v.decodeIfPresent(String.self, forKey: .jobId))
    }
    public init(workspaceId: String, tenantId: String, principalId: String, artworkId: String,
                request: NativeGPUModelPreparationRequest, jobId: String? = nil) throws {
        guard [workspaceId, tenantId, principalId, artworkId].allSatisfy({ NativeGPUModelPreparation.id($0) }),
              request.valid, jobId == nil || NativeGPUModelPreparation.id(jobId) else { throw NativeGPUModelPreparation.invalid() }
        version = 1; self.workspaceId = workspaceId; self.tenantId = tenantId; self.principalId = principalId
        self.artworkId = artworkId; self.request = request; self.jobId = jobId
    }
    public func matches(workspaceId: String, tenantId: String, principalId: String, artworkId: String,
                        request: NativeGPUModelPreparationRequest, jobId: String?) -> Bool {
        version == 1 && self.request.valid && self.request == request
            && NativeGPUModelPreparation.sameID(self.workspaceId, workspaceId) && NativeGPUModelPreparation.sameID(self.tenantId, tenantId)
            && NativeGPUModelPreparation.sameID(self.principalId, principalId) && NativeGPUModelPreparation.sameID(self.artworkId, artworkId)
            && ((self.jobId == nil && jobId == nil) || NativeGPUModelPreparation.sameID(self.jobId, jobId))
    }
}

/// An existing selection is allowed as prior state; only exact bindings qualify
/// this candidate as selected. Approval is an explicit separate request.
public struct NativeGPUModelSelection: Equatable, Sendable {
    public let value: DocumentValue
    public init(_ value: DocumentValue) throws {
        guard NativeGPUModelPreparation.exact(value, ["version", "profile", "jobId", "sourceAssetId", "candidateAssetId", "candidateSHA256", "receiptSHA256", "approvedRevision"]),
              value["version"] == .integer(1), value["profile"] == .string(NativeGPUModelPreparation.profile),
              NativeGPUModelPreparation.id(value["jobId"].string), NativeGPUModelPreparation.id(value["sourceAssetId"].string),
              NativeGPUModelPreparation.id(value["candidateAssetId"].string),
              !NativeGPUModelPreparation.sameID(value["sourceAssetId"].string, value["candidateAssetId"].string),
              NativeGPUModelPreparation.digest(value["candidateSHA256"].string), NativeGPUModelPreparation.digest(value["receiptSHA256"].string),
              NativeGPUModelPreparation.integer(value["approvedRevision"], 1...2_147_483_647) else { throw NativeGPUModelPreparation.invalid() }
        self.value = value
    }
    public func matchesCandidate(job: NativeGPUModelPreparationJob, request: NativeGPUModelPreparationRequest) -> Bool {
        let nested = job.value["gpuPreparation"], candidate = nested["candidate"], receipt = nested["serverReceipt"]
        return request == job.request && request.revision < 2_147_483_647 && nested["status"] == .string("ready")
            && NativeGPUModelPreparation.sameID(value["jobId"].string, job.value["id"].string)
            && NativeGPUModelPreparation.sameID(value["sourceAssetId"].string, request.assetId)
            && NativeGPUModelPreparation.sameID(value["candidateAssetId"].string, candidate["assetId"].string)
            && value["candidateSHA256"] == candidate["sha256"] && value["receiptSHA256"] == receipt["sha256"]
            && value["approvedRevision"] == .integer(Int64(request.revision + 1))
            && nested["generation"] == .string(request.generation)
    }
}

public struct NativeGPUModelReview: Sendable {
    public let value: DocumentValue
    public let priorSelection: NativeGPUModelSelection?
    public init(_ value: DocumentValue, artworkId: String, request: NativeGPUModelPreparationRequest,
                job: NativeGPUModelPreparationJob) throws {
        let n = job.value["gpuPreparation"]
        guard NativeGPUModelPreparation.sameID(job.artworkId, artworkId), job.request == request,
              NativeGPUModelPreparation.exact(value, ["artworkId", "revision", "jobId", "receiptSHA256", "source", "candidate", "serverReceipt", "selection"]),
              NativeGPUModelPreparation.sameID(value["artworkId"].string, artworkId), value["revision"] == .integer(Int64(request.revision)),
              NativeGPUModelPreparation.sameID(value["jobId"].string, job.value["id"].string), n["status"] == .string("ready"),
              value["receiptSHA256"] == n["serverReceipt"]["sha256"], value["source"] == n["source"],
              value["candidate"] == n["candidate"], value["serverReceipt"] == n["serverReceipt"] else { throw NativeGPUModelPreparation.invalid() }
        priorSelection = value["selection"] == .null ? nil : try NativeGPUModelSelection(value["selection"])
        self.value = value
    }
}

public struct NativeGPUModelApprovalRequest: Encodable, Equatable, Sendable {
    public let revision: Int, jobId: String, receiptSHA256: String, candidateSHA256: String, generation: String
    public init(review: NativeGPUModelReview, request: NativeGPUModelPreparationRequest) throws {
        guard request.valid, review.value["revision"] == .integer(Int64(request.revision)),
              review.value["serverReceipt"]["operationID"] == .string(request.idempotencyKey),
              review.value["serverReceipt"]["generation"] == .string(request.generation),
              NativeGPUModelPreparation.sameID(review.value["source"]["assetId"].string, request.assetId),
              review.value["source"]["sha256"] == .string(request.sha256), review.value["source"]["bytes"] == .integer(Int64(request.bytes)),
              let job = review.value["jobId"].string, let receipt = review.value["receiptSHA256"].string,
              let candidate = review.value["candidate"]["sha256"].string else { throw NativeGPUModelPreparation.invalid() }
        revision = request.revision; jobId = job; receiptSHA256 = receipt; candidateSHA256 = candidate; generation = request.generation
    }
}

/// Authoritative readback can contain a previous selection; consumers must call
/// matchesCandidate explicitly and never infer selection from ready status.
public struct NativeGPUModelSelectionReadback: Sendable {
    public let artworkId: String, revision: Int
    public let selection: NativeGPUModelSelection?
    public init(_ value: DocumentValue, artworkId: String) throws {
        guard NativeGPUModelPreparation.exact(value, ["artworkId", "revision", "selection"]),
              let id = value["artworkId"].string, NativeGPUModelPreparation.sameID(id, artworkId),
              let revision = value["revision"].integer, (1...2_147_483_647).contains(revision) else { throw NativeGPUModelPreparation.invalid() }
        self.artworkId = id; self.revision = Int(revision)
        selection = value["selection"] == .null ? nil : try NativeGPUModelSelection(value["selection"])
        if let selection {
            guard let approvedRevision = selection.value["approvedRevision"].integer,
                  approvedRevision <= revision else { throw NativeGPUModelPreparation.invalid() }
        }
    }
}

public struct NativeGPUModelApprovalResult: Sendable {
    public let selection: NativeGPUModelSelection
    public init(_ value: DocumentValue, artworkId: String, job: NativeGPUModelPreparationJob,
                request: NativeGPUModelPreparationRequest) throws {
        guard NativeGPUModelPreparation.sameID(job.artworkId, artworkId), job.request == request,
              request.revision < 2_147_483_647,
              NativeGPUModelPreparation.exact(value, ["artworkId", "revision", "jobId", "receiptSHA256", "selection"]),
              NativeGPUModelPreparation.sameID(value["artworkId"].string, artworkId),
              value["revision"] == .integer(Int64(request.revision + 1)),
              NativeGPUModelPreparation.sameID(value["jobId"].string, job.value["id"].string),
              value["receiptSHA256"] == job.value["gpuPreparation"]["serverReceipt"]["sha256"] else { throw NativeGPUModelPreparation.invalid() }
        let candidate = try NativeGPUModelSelection(value["selection"])
        guard candidate.matchesCandidate(job: job, request: request) else { throw NativeGPUModelPreparation.invalid() }
        selection = candidate
    }
}
