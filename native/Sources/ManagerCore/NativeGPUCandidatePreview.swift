// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A request derived from a freshly server-verified ready job and review.
/// It does not itself prove that a frame was presented.
public struct NativeGPUCandidatePreviewRequest: Sendable, Equatable {
    public static let capability = "gpu-candidate-preview-v1"
    public let value: DocumentValue
    public let reviewRevision: Int64

    public init(transfer: NativeGPUModelPreparationTransfer, job: NativeGPUModelPreparationJob,
                review: NativeGPUModelReview, epoch: String, sequence: Int, requestId: String) throws {
        guard let jobId = transfer.jobId, job.request == transfer.request,
              NativeGPUModelPreparation.sameID(job.artworkId, transfer.artworkId),
              NativeGPUModelPreparation.sameID(job.value["id"].string, jobId),
              job.value["gpuPreparation"]["status"] == .string("ready"),
              NativeGPUModelPreparation.id(epoch), NativeGPUModelPreparation.id(requestId),
              sequence > 0, Int64(sequence) <= 9_007_199_254_740_991,
              let rev = review.value["revision"].integer,
              review.value["jobId"] == job.value["id"], review.value["serverReceipt"] == job.value["gpuPreparation"]["serverReceipt"],
              review.value["source"] == job.value["gpuPreparation"]["source"], review.value["candidate"] == job.value["gpuPreparation"]["candidate"] else {
            throw NativeGPUModelPreparation.invalid()
        }
        reviewRevision = rev
        value = .object(["version": .integer(1), "phase": .string("begin"), "epoch": .string(epoch), "sequence": .integer(Int64(sequence)),
                         "requestId": .string(requestId), "workspaceId": .string(transfer.workspaceId), "tenantId": .string(transfer.tenantId),
                         "principalId": .string(transfer.principalId), "artworkId": .string(transfer.artworkId),
                         "artworkRevision": .integer(Int64(transfer.request.revision)), "jobId": .string(jobId),
                         "generation": .string(transfer.request.generation), "receiptSHA256": review.value["receiptSHA256"],
                         "source": review.value["source"], "candidate": review.value["candidate"]])
    }

    public var cancel: DocumentValue {
        .object(["version": .integer(1), "phase": .string("cancel"), "epoch": value["epoch"], "sequence": value["sequence"],
                 "requestId": value["requestId"], "workspaceId": value["workspaceId"], "tenantId": value["tenantId"], "principalId": value["principalId"]])
    }

    public func acceptsReady(_ reply: DocumentValue) -> Bool {
        guard case .object(var fields) = reply, case .object(var expected) = value else { return false }
        expected.removeValue(forKey: "phase")
        let extra: Set<String> = ["capability", "reviewRevision", "rendered", "drawGeneration", "targetFormat"]
        guard Set(fields.keys) == Set(expected.keys).union(extra),
              fields.removeValue(forKey: "capability") == .string(Self.capability),
              fields.removeValue(forKey: "reviewRevision") == .integer(reviewRevision),
              fields.removeValue(forKey: "rendered") == .bool(true),
              let draw = fields.removeValue(forKey: "drawGeneration"), NativeGPUModelPreparation.integer(draw, 1...9_007_199_254_740_991),
              let format = fields.removeValue(forKey: "targetFormat")?.string, ["astc-4x4", "bc7"].contains(format) else { return false }
        return fields == expected
    }

    public func acceptsCancelled(_ reply: DocumentValue) -> Bool {
        guard case .object(var fields) = reply, case .object(var expected) = cancel else { return false }
        expected.removeValue(forKey: "phase")
        guard fields.removeValue(forKey: "cancelled") == .bool(true), fields.removeValue(forKey: "drained") == .bool(true) else { return false }
        return fields == expected
    }

    /// Nil indicates a malformed/foreign event. True/false is the claimed drain.
    public func endedDrain(_ event: DocumentValue) -> Bool? {
        guard NativeGPUModelPreparation.exact(event, ["version", "epoch", "sequence", "type", "body"]),
              event["version"] == .integer(1), event["epoch"] == value["epoch"], event["sequence"] == value["sequence"],
              event["type"] == .string("gpu-candidate-preview-ended") else { return nil }
        let b = event["body"]
        guard NativeGPUModelPreparation.exact(b, ["requestId", "workspaceId", "tenantId", "principalId", "artworkId", "artworkRevision", "jobId", "generation", "receiptSHA256", "candidateSHA256", "reason", "drained"]),
              ["requestId", "workspaceId", "tenantId", "principalId", "artworkId", "artworkRevision", "jobId", "generation", "receiptSHA256"].allSatisfy({ b[$0] == value[$0] }),
              b["candidateSHA256"] == value["candidate"]["sha256"],
              let reason = b["reason"].string, ["cancelled", "superseded", "hidden", "context-lost", "authority-changed", "source-changed", "render-failed", "editor-frame", "disposed"].contains(reason),
              case .bool(let drained) = b["drained"] else { return nil }
        return drained
    }
}

/// Mutated only on the WK owner's main actor. Invalidating is irreversible;
/// late begin acknowledgements cannot restore the witness.
public struct NativeGPUCandidatePreviewLifecycle: Sendable {
    public let request: NativeGPUCandidatePreviewRequest
    public private(set) var invalidated = false
    public private(set) var rendered = false
    public private(set) var drained = false
    public init(request: NativeGPUCandidatePreviewRequest) { self.request = request }
    public mutating func acceptReady(_ value: DocumentValue) -> Bool {
        guard !invalidated, !rendered, request.acceptsReady(value) else { return false }
        rendered = true; return true
    }
    public mutating func invalidate() { invalidated = true; rendered = false }
    @discardableResult public mutating func acceptEnded(_ value: DocumentValue) -> Bool {
        guard let finished = request.endedDrain(value) else { return false }
        invalidate(); drained = drained || finished; return true
    }
    @discardableResult public mutating func acceptCancelled(_ value: DocumentValue) -> Bool {
        guard request.acceptsCancelled(value) else { return false }
        invalidate(); drained = true; return true
    }
}
