// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeOEXImportIntent: Codable, Equatable, Sendable {
    public let requestId: String
    public let sha256: String
    public let bytes: Int
    public let profile: String?
    public let oexFormat: String?
    public init(requestId: String, sha256: String, bytes: Int, profile: String? = nil, oexFormat: String? = nil) {
        self.requestId = requestId; self.sha256 = sha256; self.bytes = bytes
        self.profile = profile; self.oexFormat = oexFormat
    }
    public var hasSupportedProfile: Bool {
        switch (profile, oexFormat) {
        case (nil, nil): return (1...64 * 1024 * 1024).contains(bytes)
        case ("bounded-file-v1"?, "1.0.0-draft.9"?): return (1...512 * 1024 * 1024).contains(bytes)
        default: return false
        }
    }
    /// Large-profile records must never enter legacy mutation or retry paths.
    public var permitsLegacyJobMutation: Bool {
        profile == nil && oexFormat == nil && hasSupportedProfile
            && UUID(uuidString: requestId)?.uuidString.lowercased() == requestId
            && sha256.utf8.count == 64
            && sha256.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    public func matches(_ archive: NativeOEXArchive) -> Bool { hasSupportedProfile && bytes == archive.count && sha256 == archive.sha256 }
}
public struct NativeOEXImportState: Codable, Sendable {
    public let format: Int
    public var revision: UInt64
    public let workspaceId: String
    public let tenantId: String
    public let principalId: String
    public var intent: NativeOEXImportIntent?
    public var jobId: String?
    public var fileName: String?
}

/// A creation request may have committed, or its returned job could not be
/// durably recorded. Preserve the intent; this is not permission to retry.
public struct NativeBoundedOEXUncertain: Error, Sendable {
    public let workspaceId: String
    public let tenantId: String
    public let principalId: String
    public let requestId: String
    public let jobId: String?
}

public protocol NativeOEXImportTransport: Sendable {
    func createOEXImport(requestId: String, bytes: Int, sha256: String) async throws -> NativeOEXImportJob
    func oexImportJob(_ id: String) async throws -> NativeOEXImportJob
    func uploadOEXImport(_ id: String, archive: NativeOEXArchive) async throws
    func oexImportAction(_ id: String, action: String) async throws -> NativeOEXImportJob
}
extension AuthoringAPI: NativeOEXImportTransport {}

/// Selected bounded transport remains unusable without a separately admitted
/// live owner; the protocol permits focused failure and cancellation checks.
public protocol NativeBoundedOEXTransport: Sendable {
    func createBoundedOEXImport(requestId: String, archive: NativeOEXArchive) async throws -> NativeBoundedOEXJob
    func uploadBoundedOEXImport(_ id: String, archive: NativeOEXArchive) async throws
    func queueBoundedOEXImport(_ id: String) async throws -> NativeBoundedOEXJob
}
extension AuthoringAPI: NativeBoundedOEXTransport {}

/// One durable request ID is saved before the first network mutation; retries
/// never manufacture a new job or use a different selected archive.
@MainActor
public enum NativeOEXImportTransfer {
    /// Selected fresh runtime only. Saves the exact bounded intent before
    /// private selection or HTTP; the caller retains the same file and Session.
    /// Errors preserve the journal and are never retried/rearmed here.
    public static func runBoundedOnce(state: NativeOEXImportState, file: NativeOEXImportFile,
                                      api: any NativeBoundedOEXTransport,
                                      persist: @escaping @MainActor (NativeOEXImportState) async throws -> NativeOEXImportState,
                                      select: @escaping @MainActor () async throws -> Void,
                                      isCurrent: @escaping @MainActor () -> Bool) async throws -> (NativeOEXImportState, NativeBoundedOEXJob) {
        func check() throws { try Task.checkCancellation(); guard isCurrent() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") } }
        try check()
        guard state.format == 1, state.revision < UInt64.max - 1,
              state.intent == nil, state.jobId == nil else { throw AuthoringFailure("OEX_IMPORT_PROFILE_UNAVAILABLE") }
        var saved = state
        let intent = NativeOEXImportIntent(requestId: UUID().uuidString.lowercased(), sha256: file.archive.sha256, bytes: file.archive.count,
                                           profile: "bounded-file-v1", oexFormat: "1.0.0-draft.9")
        guard intent.matches(file.archive) else { throw AuthoringFailure("OEX_IMPORT_FILE_CHANGED") }
        saved.intent = intent; saved.fileName = file.name
        saved = try await persist(saved); try check()
        guard saved.format == state.format, saved.revision == state.revision + 1,
              saved.workspaceId == state.workspaceId, saved.tenantId == state.tenantId, saved.principalId == state.principalId,
              saved.intent == intent, saved.jobId == nil, saved.fileName == file.name else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
        try await select(); try check()
        let created: NativeBoundedOEXJob
        do {
            created = try await api.createBoundedOEXImport(requestId: intent.requestId, archive: file.archive)
        } catch {
            throw NativeBoundedOEXUncertain(workspaceId: state.workspaceId, tenantId: state.tenantId,
                                            principalId: state.principalId, requestId: intent.requestId, jobId: nil)
        }
        let uncertain = NativeBoundedOEXUncertain(workspaceId: state.workspaceId, tenantId: state.tenantId,
                                                   principalId: state.principalId, requestId: intent.requestId, jobId: created.id)
        // The persist closure is bound to the original selection token and
        // uses the journal revision check. Try it even if UI scope changed;
        // a changed token must reject rather than redirecting the write.
        let priorRevision = saved.revision
        saved.jobId = created.id
        // Persist the known server job even when the initiating task was
        // cancelled. The captured writer still uses the original selection
        // token and revision CAS; it cannot redirect into another workspace.
        let pending = saved
        let persistence = Task { @MainActor in try await persist(pending) }
        do { saved = try await persistence.value } catch { throw uncertain }
        guard saved.format == state.format, saved.revision == priorRevision + 1,
              saved.workspaceId == state.workspaceId, saved.tenantId == state.tenantId, saved.principalId == state.principalId,
              saved.intent == intent, saved.jobId == created.id, saved.fileName == file.name else { throw uncertain }
        try check()
        try await api.uploadBoundedOEXImport(created.id, archive: file.archive); try check()
        let queued = try await api.queueBoundedOEXImport(created.id); try check()
        return (saved, queued)
    }
    public static func run(state: NativeOEXImportState, file: NativeOEXImportFile?, transport: any NativeOEXImportTransport,
                           persist: @escaping @MainActor (NativeOEXImportState) async throws -> NativeOEXImportState,
                           isCurrent: @escaping @MainActor () -> Bool) async throws -> (NativeOEXImportState, NativeOEXImportJob) {
        func check() throws { try Task.checkCancellation(); guard isCurrent() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") } }
        try check(); var saved = state
        if saved.intent == nil {
            guard let file else { throw AuthoringFailure("OEX_FILE_REQUIRED") }
            saved.intent = NativeOEXImportIntent(requestId: UUID().uuidString.lowercased(), sha256: file.archive.sha256, bytes: file.archive.count)
            saved.fileName = file.name; saved = try await persist(saved); try check()
        }
        guard let intent = saved.intent, intent.hasSupportedProfile else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
        // Large-profile transport is not enabled until capability and job binding are verified.
        guard intent.profile == nil, intent.oexFormat == nil else { throw AuthoringFailure("OEX_IMPORT_PROFILE_UNAVAILABLE") }
        if let file, !intent.matches(file.archive) { throw AuthoringFailure("OEX_IMPORT_FILE_CHANGED") }
        // Once a job ID is durable, never POST again. A restored older server
        // may no longer contain that job; creating a replacement would mutate
        // a different history before the mismatch could be detected.
        let job: NativeOEXImportJob
        if let id = saved.jobId { job = try await transport.oexImportJob(id) }
        else { job = try await transport.createOEXImport(requestId: intent.requestId, bytes: intent.bytes, sha256: intent.sha256) }
        try check(); _ = try job.checked()
        guard saved.jobId.map({ $0.lowercased() == job.id.lowercased() }) ?? true else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
        if saved.jobId == nil { saved.jobId = job.id; saved = try await persist(saved); try check() }
        guard job.state == "uploading" else { return (saved, job) }
        if let file { try await transport.uploadOEXImport(job.id, archive: file.archive); try check() }
        do {
            let completed = try await transport.oexImportAction(job.id, action: "complete"); try check()
            guard completed.id.lowercased() == job.id.lowercased(), completed.state != "uploading" else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
            return (saved, try completed.checked())
        } catch let failure as AuthoringFailure where file == nil && failure.code == "UPLOAD_INCOMPLETE" {
            throw AuthoringFailure("OEX_FILE_REQUIRED")
        }
    }
}

public struct NativeOEXImportResult: Decodable, Sendable {
    public let exhibitionId: String
}
public struct NativeOEXImportJob: Decodable, Sendable {
    public let id: String
    public let state: String
    public let errorCode: String?
    public let attempts: Int
    public let result: NativeOEXImportResult?
    public let cleanupPending: Bool

    public func checked() throws -> Self {
        guard UUID(uuidString: id) != nil,
              ["uploading", "queued", "processing", "complete", "failed", "cancelled"].contains(state),
              (0...5).contains(attempts),
              result.map({ UUID(uuidString: $0.exhibitionId) != nil }) ?? (state != "complete"),
              state == "complete" || result == nil else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
        return self
    }
}
