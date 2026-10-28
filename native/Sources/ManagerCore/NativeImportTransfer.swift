// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing authenticated server import operations. An acknowledged complete
/// rechecks the durable original's length/hash before the server queues work.
public protocol NativeImportTransport: Sendable {
    func createImport(_ input: NativeImportIntent) async throws -> NativeImportJob
    func uploadImport(_ id: String, bytes: Data) async throws
    func uploadDerivedImport(_ id: String, bytes: Data) async throws
    func importAction(_ id: String, action: NativeImportAction) async throws -> NativeImportJob
}
extension AuthoringAPI: NativeImportTransport {}

/// The same import API, with observation of the server-owned worker. A cancel
/// acknowledgement is not its close/drain receipt.
public protocol NativeModelDerivationTransport: NativeImportTransport {
    func importJob(_ id: String) async throws -> NativeImportJob
}
extension AuthoringAPI: NativeModelDerivationTransport {}

/// Owns only the local transfer task; a server cancellation response is never a
/// receipt that its worker has drained. No conversion or asset registration is
/// performed here. The unchanged server lease/validation transaction owns both.
@MainActor
public final class NativeImportTransfer {
    private var task: Task<NativeImportJob, Error>?
    private struct ModelOperation {
        let intent: NativeImportIntent
        let transport: any NativeModelDerivationTransport
        var jobID: String?
    }
    private var modelOperation: ModelOperation?
    private var modelDrainTask: Task<Void, Error>?
    public private(set) var modelDrainFailure: String?
    public var hasPendingModelWorker: Bool { modelOperation != nil || modelDrainTask != nil }
    public private(set) var operationID: UUID?
    public init() {}

    /// Restore the exact durable request before observing a server worker. No
    /// new generation, source or payload is synthesized during recovery.
    public func restoreModel(intent: NativeImportIntent, jobID: String?, transport: any NativeModelDerivationTransport) throws {
        guard intent.modelDerivation != nil else { return }
        guard intent.mime == "model/gltf-binary", intent.derived == nil,
              intent.modelDerivation?.valid == true, intent.modelDerivation?.operationID == intent.idempotencyKey,
              task == nil, modelDrainTask == nil, modelOperation.map({ $0.intent == intent }) ?? true else { throw AuthoringFailure("IMPORT_ACTIVE") }
        modelOperation = ModelOperation(intent: intent, transport: transport, jobID: jobID)
    }

    public func run(intent: NativeImportIntent, jobID: String?, file: ImportFileSnapshot?, derived: NativePreparedJPEG? = nil,
                    transport: any NativeImportTransport,
                    persistIntent: @escaping @MainActor () async throws -> Void = {},
                    persistJob: @escaping @MainActor (NativeImportJob) async throws -> Void,
                    isCurrent: @escaping @MainActor () -> Bool) async throws -> NativeImportJob {
        try Task.checkCancellation()
        guard task == nil, modelDrainTask == nil,
              modelOperation.map({ $0.intent == intent }) ?? true else { throw AuthoringFailure("IMPORT_ACTIVE") }
        if let model = intent.modelDerivation {
            guard intent.mime == "model/gltf-binary", intent.derived == nil, model.valid,
                  model.operationID == intent.idempotencyKey, transport is any NativeModelDerivationTransport else { throw AuthoringFailure("IMPORT_MODEL_BINDING_INVALID") }
        }
        let id = UUID()
        operationID = id
        let owned = Task { @MainActor [weak self] in
            @MainActor func check() throws {
                try Task.checkCancellation()
                guard self?.operationID == id, isCurrent() else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
            }
            try check()
            if let file {
                guard file.matches(intent) else { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
                if let descriptor = intent.derived {
                    guard intent.mime == "image/jpeg", descriptor.valid, let derived,
                          derived.originalSHA256 == intent.sha256, derived.derived == descriptor,
                          derived.png.count == descriptor.bytes, nativeImportSHA256(derived.png) == descriptor.sha256 else { throw AuthoringFailure("IMPORT_DERIVED_REQUIRED") }
                } else if derived != nil || intent.mime == "image/jpeg" { throw AuthoringFailure("IMPORT_INTENT_CHANGED") }
            }
            try await persistIntent()
            try check()
            if intent.modelDerivation != nil, let transport = transport as? any NativeModelDerivationTransport {
                // Capture before the first possibly uncertain POST. Recovery
                // observes this exact durable key instead of inventing a job.
                self?.modelOperation = ModelOperation(intent: intent, transport: transport, jobID: jobID)
            }
            let created = try await transport.createImport(intent)
            try check()
            guard Self.valid(created, intent: intent),
                  jobID.map({ $0.lowercased() == created.id.lowercased() }) ?? true else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            self?.modelOperation?.jobID = created.id
            try await persistJob(created) // durable same-intent binding before sending any bytes
            try check()
            guard created.state == "uploading" else {
                try self?.observe(created, intent: intent)
                return created
            }
            if let file {
                try await transport.uploadImport(created.id, bytes: file.data)
                try check()
                if let derived {
                    try await transport.uploadDerivedImport(created.id, bytes: derived.png)
                    try check()
                }
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
            guard Self.valid(completed, intent: intent), completed.state != "uploading",
                  completed.id.lowercased() == created.id.lowercased() else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            try self?.observe(completed, intent: intent)
            return completed
        }
        task = owned
        defer { if operationID == id { task = nil; operationID = nil } }
        @MainActor func checkCaller() throws {
            try Task.checkCancellation()
            guard !owned.isCancelled else { throw CancellationError() }
            guard operationID == id, isCurrent() else { throw AuthoringFailure("IMPORT_CONTEXT_CHANGED") }
        }
        return try await withTaskCancellationHandler {
            do {
                let result = try await owned.value
                try checkCaller()
                return result
            } catch {
                try checkCaller()
                throw error
            }
        } onCancel: { owned.cancel() }
    }

    private static func valid(_ job: NativeImportJob, intent: NativeImportIntent) -> Bool {
        UUID(uuidString: job.id) != nil
            && ["uploading", "queued", "processing", "approved", "failed", "cancelled"].contains(job.state)
            && (job.originalAssetId == nil || job.originalAssetId.flatMap(UUID.init(uuidString:)) != nil)
            && (job.state != "approved" || (job.assetId.flatMap(UUID.init(uuidString:)) != nil
                && (intent.derived == nil || (job.originalAssetId.flatMap(UUID.init(uuidString:)) != nil && job.originalAssetId?.lowercased() != job.assetId?.lowercased()))))
            && (intent.modelDerivation == nil ? job.modelDerivation == nil : job.modelDerivation?.matches(intent, job: job) == true)
    }

    /// A response updates only the matching owned operation. Optional failure
    /// is still a successful original import when the server says approved.
    public func observe(_ job: NativeImportJob, intent: NativeImportIntent) throws {
        guard Self.valid(job, intent: intent) else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
        guard let owned = modelOperation, owned.intent == intent,
              owned.jobID == nil || owned.jobID == job.id else { return }
        modelOperation?.jobID = job.id
        if job.modelDerivation?.workerDrained == true,
           ["approved", "failed", "cancelled"].contains(job.state),
           job.state != "approved" || job.modelDerivation?.status != "pending" {
            modelOperation = nil; modelDrainFailure = nil
        }
    }

    private func drainModel() async throws {
        if let existing = modelDrainTask { try await existing.value; return }
        guard let owned = modelOperation else { return }
        let drain = Task { @MainActor [weak self] in
            var id = owned.jobID
            if id == nil {
                let observed = try await owned.transport.createImport(owned.intent)
                guard Self.valid(observed, intent: owned.intent) else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
                id = observed.id
                self?.modelOperation?.jobID = observed.id
            }
            guard let id else { throw AuthoringFailure("IMPORT_DRAIN_PENDING") }
            let cancelled: NativeImportJob
            do { cancelled = try await owned.transport.importAction(id, action: .cancel) }
            catch {
                // The cancel response can be lost or race an already completed
                // worker. Only a fresh matching status can establish drain.
                cancelled = try await owned.transport.importJob(id)
            }
            guard Self.valid(cancelled, intent: owned.intent), cancelled.id == id else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            let deadline = ContinuousClock.now.advanced(by: .seconds(30))
            var observed = cancelled
            while true {
                try Task.checkCancellation()
                guard self?.modelOperation?.intent == owned.intent, self?.modelOperation?.jobID == id else { throw CancellationError() }
                try self?.observe(observed, intent: owned.intent)
                if self?.modelOperation == nil { return }
                guard ContinuousClock.now < deadline else { throw AuthoringFailure("IMPORT_DRAIN_PENDING") }
                try await Task.sleep(for: .milliseconds(100))
                observed = try await owned.transport.importJob(id)
                guard observed.id == id else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
            }
        }
        modelDrainTask = drain
        defer { modelDrainTask = nil }
        try await drain.value
    }

    /// Cancellation requests are cooperative. Retain and await the actual owned
    /// task before allowing service/profile shutdown or a replacement transfer.
    public func cancelAndWait() async {
        if let owned = task {
            let id = operationID
            owned.cancel()
            _ = await owned.result
            if operationID == id { task = nil; operationID = nil }
        }
        do { try await drainModel() }
        catch {
            if modelOperation != nil { modelDrainFailure = (error as? AuthoringFailure)?.code ?? "IMPORT_DRAIN_PENDING" }
        }
    }

    /// Called by the owning native service controller only after its actual
    /// stopped/no-children receipt, never from a request/HTTP cancel response.
    public func ownedRuntimeDidStop() async {
        if let owned = task {
            let id = operationID
            owned.cancel(); _ = await owned.result
            if operationID == id { task = nil; operationID = nil }
        }
        if let drain = modelDrainTask { drain.cancel(); _ = await drain.result }
        modelDrainTask = nil; modelOperation = nil; modelDrainFailure = nil
    }
}
