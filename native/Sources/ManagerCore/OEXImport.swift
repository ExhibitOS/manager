// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeOEXImportIntent: Codable, Equatable, Sendable {
    public let requestId: String
    public let sha256: String
    public let bytes: Int
    public func matches(_ archive: NativeOEXArchive) -> Bool { bytes == archive.data.count && sha256 == archive.sha256 }
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

public protocol NativeOEXImportTransport: Sendable {
    func createOEXImport(requestId: String, bytes: Int, sha256: String) async throws -> NativeOEXImportJob
    func oexImportJob(_ id: String) async throws -> NativeOEXImportJob
    func uploadOEXImport(_ id: String, archive: NativeOEXArchive) async throws
    func oexImportAction(_ id: String, action: String) async throws -> NativeOEXImportJob
}
extension AuthoringAPI: NativeOEXImportTransport {}

/// One durable request ID is saved before the first network mutation; retries
/// never manufacture a new job or use a different selected archive.
@MainActor
public enum NativeOEXImportTransfer {
    public static func run(state: NativeOEXImportState, file: NativeOEXImportFile?, transport: any NativeOEXImportTransport,
                           persist: @escaping @MainActor (NativeOEXImportState) async throws -> NativeOEXImportState,
                           isCurrent: @escaping @MainActor () -> Bool) async throws -> (NativeOEXImportState, NativeOEXImportJob) {
        func check() throws { try Task.checkCancellation(); guard isCurrent() else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") } }
        try check(); var saved = state
        if saved.intent == nil {
            guard let file else { throw AuthoringFailure("OEX_FILE_REQUIRED") }
            saved.intent = NativeOEXImportIntent(requestId: UUID().uuidString.lowercased(), sha256: file.archive.sha256, bytes: file.archive.data.count)
            saved.fileName = file.name; saved = try await persist(saved); try check()
        }
        guard let intent = saved.intent else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
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
