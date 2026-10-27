// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing authenticated server import operations. An acknowledged complete
/// rechecks the durable original's length/hash before the server queues work.
public protocol NativeImportTransport: Sendable {
    func createImport(_ input: NativeImportIntent) async throws -> NativeImportJob
    func uploadImport(_ id: String, bytes: Data) async throws
    func importAction(_ id: String, action: NativeImportAction) async throws -> NativeImportJob
}
extension AuthoringAPI: NativeImportTransport {}

/// Owns only the local transfer task; a server cancellation response is never a
/// receipt that its worker has drained. No conversion or asset registration is
/// performed here. The unchanged server lease/validation transaction owns both.
@MainActor
public final class NativeImportTransfer {
    private var task: Task<NativeImportJob, Error>?
    public private(set) var operationID: UUID?
    public init() {}

    public func run(intent: NativeImportIntent, jobID: String?, file: ImportFileSnapshot?,
                    transport: any NativeImportTransport,
                    persistIntent: @escaping @MainActor () async throws -> Void = {},
                    persistJob: @escaping @MainActor (NativeImportJob) async throws -> Void,
                    isCurrent: @escaping @MainActor () -> Bool) async throws -> NativeImportJob {
        try Task.checkCancellation()
        guard task == nil else { throw AuthoringFailure("IMPORT_ACTIVE") }
        let id = UUID()
        operationID = id
        let owned = Task { @MainActor [weak self] in
            @MainActor func check() throws {
                try Task.checkCancellation()
                guard self?.operationID == id, isCurrent() else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
            }
            try check()
            if let file, !file.matches(intent) { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            try await persistIntent()
            try check()
            let created = try await transport.createImport(intent)
            try check()
            guard Self.valid(created),
                  jobID.map({ $0.lowercased() == created.id.lowercased() }) ?? true else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            try await persistJob(created) // durable same-intent binding before sending any bytes
            try check()
            guard created.state == "uploading" else { return created }
            if let file {
                try await transport.uploadImport(created.id, bytes: file.data)
                try check()
            }
            // When recovering without a local selection, reuse already received
            // original bytes. Missing/corrupt bytes must not start a new job or
            // substitute a derivative. Preserve the intent and request the same file.
            let completed: NativeImportJob
            do { completed = try await transport.importAction(created.id, action: .complete) }
            catch let error as AuthoringFailure where file == nil && error.code == "UPLOAD_INCOMPLETE" {
                try check()
                throw AuthoringFailure("IMPORT_FILE_REQUIRED")
            }
            try check()
            guard Self.valid(completed), completed.state != "uploading",
                  completed.id.lowercased() == created.id.lowercased() else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            return completed
        }
        task = owned
        defer { if operationID == id { task = nil; operationID = nil } }
        return try await withTaskCancellationHandler { try await owned.value } onCancel: { owned.cancel() }
    }

    private static func valid(_ job: NativeImportJob) -> Bool {
        UUID(uuidString: job.id) != nil
            && ["uploading", "queued", "processing", "approved", "failed", "cancelled"].contains(job.state)
            && (job.state != "approved" || job.assetId.flatMap(UUID.init(uuidString:)) != nil)
    }

    /// Cancellation requests are cooperative. Retain and await the actual owned
    /// task before allowing service/profile shutdown or a replacement transfer.
    public func cancelAndWait() async {
        guard let owned = task else { return }
        let id = operationID
        owned.cancel()
        _ = await owned.result
        if operationID == id { task = nil; operationID = nil }
    }
}
