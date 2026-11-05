// SPDX-License-Identifier: Apache-2.0
import Foundation

enum NativeAutomationArtworkRules {
    static func command(_ request: DocumentValue, keys: Set<String>) throws -> DocumentValue {
        guard case .object(let fields) = request["command"], Set(fields.keys) == keys else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        return request["command"]
    }
    static func checkUploadScope(_ state: NativeUploadState, document: NativeDocumentState) throws {
        guard state.workspaceId == document.workspaceId, state.tenantId == document.tenantId,
              state.principalId == document.principalId else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
    }
    static func checkJob(_ job: NativeImportJob, id: String) throws {
        guard job.id == id, UUID(uuidString: id) != nil, job.modelDerivation == nil, job.imagePreparation == nil,
              ["uploading", "queued", "processing", "approved", "failed", "cancelled"].contains(job.state),
              (0...100).contains(job.progress), job.attempts >= 0 else { throw BridgeFailure("IMPORT_RECEIPT_INVALID") }
    }
    static func checkApproval(artwork: NativeArtwork, job: NativeImportJob, jobId: String,
                              intent: NativeImportIntent, revision: Int64, assetId: String) throws {
        try checkJob(job, id: jobId)
        guard revision > 0, revision <= 2_147_483_647, artwork.id == intent.artworkId,
              Int64(artwork.revision) == revision, !artwork.archived, artwork.metadata?.rights == intent.rights,
              job.state == "approved", job.assetId == assetId,
              artwork.assets.contains(where: { $0.id == assetId && $0.state == "approved"
                  && $0.mime == intent.mime && $0.sha256 == intent.sha256 && $0.bytes == intent.bytes }) else {
            throw BridgeFailure("APPROVAL_REVIEW_REQUIRED")
        }
    }
    static func checkRegistration(_ artwork: DocumentValue, artworkId: String, revision: Int64, tenantId: String) throws {
        let cms = artwork["extensions"]["org.exhibitos.studio/cms"]
        guard revision > 0, revision <= 2_147_483_647, artwork["id"].string == artworkId,
              cms["tenantId"].string == tenantId, cms["artworkId"].string == artworkId,
              cms["revision"].integer == revision, artwork["artworkType"].string == "sculpture",
              artwork["rights"]["permissions"]["display"] == .bool(true) else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
    }
    static func jobValue(_ job: NativeImportJob) -> DocumentValue {
        .object(["id": .string(job.id), "state": .string(job.state), "progress": .integer(Int64(job.progress)),
                 "attempts": .integer(Int64(job.attempts)), "assetId": job.assetId.map(DocumentValue.string) ?? .null,
                 "errorCode": job.errorCode.map(DocumentValue.string) ?? .null,
                 "retryAt": job.retryAt.map(DocumentValue.string) ?? .null,
                 "originalAssetId": job.originalAssetId.map(DocumentValue.string) ?? .null,
                 "workerDrainVersion": job.workerDrainVersion.map { .integer(Int64($0)) } ?? .null,
                 "workerDrained": job.workerDrained.map(DocumentValue.bool) ?? .null])
    }
}

/// Reuses the GUI's durable native upload journal and transfer owner. Optional
/// derivation is deliberately absent: the admitted GLB bytes are not rewritten.
@MainActor
enum NativeAutomationGLBTransfer {
    static func run(bridge: NativeBridge, context: Context, state initial: NativeUploadState,
                    intent: NativeImportIntent, file: ImportFileSnapshot,
                    transport: any NativeImportTransport) async throws -> NativeImportJob {
        guard intent.mime == "model/gltf-binary", intent.derived == nil, intent.modelDerivation == nil,
              intent.scaleMeters.isFinite, intent.scaleMeters > 0, intent.scaleMeters <= 1_000_000,
              initial.intent == nil || initial.intent == intent else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
        var state = initial
        let transfer = NativeImportTransfer()
        return try await transfer.run(intent: intent, jobID: state.jobId, file: file, transport: transport,
            persistIntent: {
                if state.intent == nil {
                    state.intent = intent; state.fileName = file.name; state.jobId = nil
                    state = try await bridge.saveUploadState(context, state: state)
                }
                guard state.intent == intent else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
            }, persistJob: { job in
                guard state.intent == intent, state.jobId == nil || state.jobId == job.id else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
                state.jobId = job.id
                state = try await bridge.saveUploadState(context, state: state)
            }, isCurrent: { !Task.isCancelled })
    }
}
