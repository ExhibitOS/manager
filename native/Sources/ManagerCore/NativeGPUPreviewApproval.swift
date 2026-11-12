// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Package-internal, one-use authority held only by the live preview controller.
/// It is never Codable, persisted, or accepted from automation input.
package final class NativeGPUPreviewApprovalAuthority: @unchecked Sendable {
    private enum State { case ready, claimed, dispatched, revoked, settled }
    private let lock = NSLock()
    private var state: State = .ready
    private var dispatched = false
    package let preview: NativeGPUCandidatePreviewRequest
    private let transfer: NativeGPUModelPreparationTransfer
    private let manifest: String
    private let session: AuthoringSession

    package init(lifecycle: NativeGPUCandidatePreviewLifecycle, transfer: NativeGPUModelPreparationTransfer,
                 manifest: String, session: AuthoringSession) throws {
        guard lifecycle.rendered, !lifecycle.invalidated, !lifecycle.drained,
              NativeGPUModelPreparation.digest(manifest),
              lifecycle.request.value["workspaceId"] == .string(transfer.workspaceId),
              lifecycle.request.value["tenantId"] == .string(transfer.tenantId),
              lifecycle.request.value["principalId"] == .string(transfer.principalId),
              lifecycle.request.value["artworkId"] == .string(transfer.artworkId),
              lifecycle.request.value["jobId"].string == transfer.jobId,
              session.tenantId == transfer.tenantId, session.userId == transfer.principalId,
              session.nativeGpuModelPreparationProfile == NativeGPUModelPreparation.profile else { throw AuthoringFailure("APPROVAL_REVIEW_REQUIRED") }
        self.preview = lifecycle.request; self.transfer = transfer; self.manifest = manifest; self.session = session
    }
    package func claim() -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard state == .ready, preview.reviewRevision == Int64(transfer.request.revision) else { return false }
        state = .claimed; return true
    }
    package func matches(transfer: NativeGPUModelPreparationTransfer, session: AuthoringSession,
                         manifest: String, review: NativeGPUModelReview) -> Bool {
        lock.lock(); defer { lock.unlock() }
        let v = preview.value
        return state == .claimed && self.transfer == transfer && self.manifest == manifest
            && self.session.userId == session.userId && self.session.tenantId == session.tenantId
            && self.session.role == session.role && self.session.csrfToken == session.csrfToken && self.session.expiresAt == session.expiresAt
            && self.session.nativeGpuModelPreparationProfile == session.nativeGpuModelPreparationProfile
            && v["workspaceId"] == .string(transfer.workspaceId) && v["tenantId"] == .string(transfer.tenantId)
            && v["principalId"] == .string(transfer.principalId) && v["artworkId"] == .string(transfer.artworkId)
            && v["jobId"].string == transfer.jobId && v["generation"] == .string(transfer.request.generation)
            && v["artworkRevision"] == .integer(Int64(transfer.request.revision))
            && review.value["revision"] == .integer(preview.reviewRevision)
            && review.value["jobId"] == v["jobId"] && review.value["receiptSHA256"] == v["receiptSHA256"]
            && review.value["source"] == v["source"] && review.value["candidate"] == v["candidate"]
    }
    /// Called immediately before the transport is entered, after fresh checks.
    package func beginDispatch() -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard state == .claimed else { return false }
        state = .dispatched; dispatched = true; return true
    }
    package var wasDispatched: Bool { lock.lock(); defer { lock.unlock() }; return dispatched }
    package func revoke() {
        lock.lock(); defer { lock.unlock() }
        if state != .settled { state = .revoked }
    }
    package func finish() { lock.lock(); defer { lock.unlock() }; state = .settled }
}
