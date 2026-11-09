// SPDX-License-Identifier: Apache-2.0
import Foundation

struct NativeAutomationReplacementCommand {
    enum Target {
        case image(coarseSHA256: String)
        case productImage(receiptSHA256: String)
        case productImageThumbnailRepair(receiptSHA256: String)
        case approvedModel(placementID: String)
    }

    static let modelProfile = NativeApprovedModelCandidateReplacement.profile
    let artworkId: String
    let artworkRevision: Int
    let importJobId: String
    let studioRevision: Int
    let etag: String
    let target: Target

    init(_ request: DocumentValue) throws {
        let common: Set<String> = ["artworkId", "artworkRevision", "importJobId", "studioRevision", "etag"]
        let command: DocumentValue
        if request["command"]["kind"] == .string(Self.modelProfile) {
            command = try NativeAutomationArtworkRules.command(request, keys: common.union(["kind", "placementId"]))
            guard let placement = command["placementId"].string,
                  UUID(uuidString: placement)?.uuidString.lowercased() == placement else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            target = .approvedModel(placementID: placement)
        } else if request["command"]["kind"] == .string(NativeProductImagePreparationIntent.profileID)
                    || request["command"]["kind"] == .string(AuthoringAPI.productImageThumbnailRepairProfile) {
            command = try NativeAutomationArtworkRules.command(request, keys: common.union(["kind", "receiptSha256"]))
            guard let sha256 = command["receiptSha256"].string, nativeImportDigest(sha256) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            target = command["kind"] == .string(AuthoringAPI.productImageThumbnailRepairProfile)
                ? .productImageThumbnailRepair(receiptSHA256: sha256) : .productImage(receiptSHA256: sha256)
        } else {
            command = try NativeAutomationArtworkRules.command(request, keys: common.union(["coarseSha256"]))
            guard let sha256 = command["coarseSha256"].string, nativeImportDigest(sha256) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            target = .image(coarseSHA256: sha256)
        }
        guard let artworkId = command["artworkId"].string, UUID(uuidString: artworkId)?.uuidString.lowercased() == artworkId,
              let job = command["importJobId"].string, UUID(uuidString: job)?.uuidString.lowercased() == job,
              let revision = command["artworkRevision"].integer, (1...2_147_483_647).contains(revision),
              let studio = command["studioRevision"].integer, (1..<2_147_483_647).contains(studio),
              let etag = command["etag"].string,
              etag.hasPrefix("\"studio-r\(studio)-"),
              etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        self.artworkId = artworkId
        artworkRevision = Int(revision)
        importJobId = job
        studioRevision = Int(studio)
        self.etag = etag
    }
}

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
        if let metadata = artwork.metadata {
            try checkDisplayAsset(metadata, sha256: intent.sha256, scaleMeters: intent.scaleMeters)
        }
    }
    static func checkICCReadback(artwork: NativeArtwork, job: NativeImportJob, jobId: String,
                                 intent: NativeImportIntent, revision: Int64) throws -> NativeImagePreparationStatus {
        guard intent.mime == "image/jpeg", intent.modelDerivation == nil,
              let derived = intent.derived, derived.valid, derived.recipe.colourConversion != nil,
              job.id == jobId, UUID(uuidString: jobId) != nil, job.modelDerivation == nil,
              job.state == "approved", job.progress == 100, job.attempts >= 0,
              let preparation = job.imagePreparation, preparation.matches(intent),
              let original = job.originalAssetId, UUID(uuidString: original) != nil,
              let display = job.assetId, UUID(uuidString: display) != nil, original.lowercased() != display.lowercased(),
              artwork.id == intent.artworkId, Int64(artwork.revision) == revision, !artwork.archived,
              artwork.approvedRevision == artwork.revision, artwork.approvedAssetId == display,
              artwork.metadata?.rights == intent.rights,
              artwork.assets.contains(where: { $0.id == original && $0.state == "quarantine"
                  && $0.mime == intent.mime && $0.sha256 == intent.sha256 && $0.bytes == intent.bytes }),
              artwork.assets.contains(where: { $0.id == display && $0.state == "approved"
                  && $0.mime == derived.mime && $0.sha256 == derived.sha256 && $0.bytes == derived.bytes }) else {
            throw BridgeFailure("IMPORT_RECEIPT_INVALID")
        }
        return preparation
    }
    static func checkDisplayAsset(_ metadata: ArtworkMetadata, sha256: String, scaleMeters: Double) throws {
        guard metadata.provenance.sourceUnits == "unknown" || metadata.displayReference != nil else { return }
        guard metadata.provenance.sourceUnits == "unknown", !metadata.provenance.scaleApplied,
              let reference = metadata.displayReference,
              reference.version == 1, reference.status == "provisional-uncalibrated",
              reference.assetSha256 == sha256, scaleMeters == 1 else {
            throw BridgeFailure("ARTWORK_DISPLAY_REFERENCE_INVALID")
        }
    }
    static func checkRegistration(_ artwork: DocumentValue, artworkId: String, revision: Int64, tenantId: String,
                                  allowsImage: Bool = false) throws {
        let cms = artwork["extensions"]["org.exhibitos.studio/cms"]
        let type = artwork["artworkType"].string
        let approvedPNG = allowsImage && type == "image" && artwork["primaryAssetId"].string.map { primary in
            artwork["assets"].items?.contains {
                $0["id"].string == primary && $0["mime"].string == "image/png" && $0["role"].string == "image"
            } == true
        } == true
        guard revision > 0, revision <= 2_147_483_647, artwork["id"].string == artworkId,
              cms["tenantId"].string == tenantId, cms["artworkId"].string == artworkId,
              cms["revision"].integer == revision, type == "sculpture" || approvedPNG,
              artwork["rights"]["permissions"]["display"] == .bool(true) else { throw BridgeFailure("ARTWORK_SCOPE_CHANGED") }
    }
    static func jobValue(_ job: NativeImportJob) -> DocumentValue {
        var value: DocumentValue = .object(["id": .string(job.id), "state": .string(job.state), "progress": .integer(Int64(job.progress)),
                 "attempts": .integer(Int64(job.attempts)), "assetId": job.assetId.map(DocumentValue.string) ?? .null,
                 "errorCode": job.errorCode.map(DocumentValue.string) ?? .null,
                 "retryAt": job.retryAt.map(DocumentValue.string) ?? .null,
                 "originalAssetId": job.originalAssetId.map(DocumentValue.string) ?? .null,
                 "workerDrainVersion": job.workerDrainVersion.map { .integer(Int64($0)) } ?? .null,
                 "workerDrained": job.workerDrained.map(DocumentValue.bool) ?? .null])
        if let preparation = job.modelPreparation {
            var detail: [String: DocumentValue] = [
                "profileId": .string(preparation.intent.profileId), "status": .string(preparation.status.rawValue),
                "required": .bool(preparation.required), "workerDrained": .bool(preparation.workerDrained)
            ]
            if let display = preparation.display, let receipt = preparation.serverReceipt {
                detail["assetId"] = .string(display.assetId)
                detail["displaySha256"] = .string(display.sha256)
                detail["originalSha256"] = .string(preparation.original.sha256)
                detail["serverReceiptSha256"] = .string(receipt.sha256)
                detail["reusedOriginal"] = .bool(display.reusedOriginal)
            }
            value["modelPreparation"] = .object(detail)
        }
        return value
    }
    static func reviewedJobValue(_ job: NativeImportJob, intent: NativeImportIntent) throws -> DocumentValue {
        guard let coarse = intent.coarsePreparation, let status = job.coarsePreparation,
              status.matches(coarse, job: job), let preparation = job.imagePreparation,
              preparation.matches(intent), UUID(uuidString: job.id) != nil else {
            throw BridgeFailure("IMPORT_RECEIPT_INVALID")
        }
        var wire = jobValue(job)
        wire["coarsePreparation"] = .object([
            "profile": .string(status.profile), "approvalId": .string(status.approvalId),
            "artworkRevision": .integer(Int64(status.artworkRevision)), "status": .string(status.status),
            "workerDrained": .bool(status.workerDrained), "assetId": status.assetId.map(DocumentValue.string) ?? .null,
            "sha256": status.sha256.map(DocumentValue.string) ?? .null,
            "bytes": status.bytes.map { .integer(Int64($0)) } ?? .null
        ])
        wire["imagePreparation"] = .object([
            "provenance": .string(preparation.provenance), "originalSha256": .string(preparation.originalSha256),
            "derivedSha256": .string(preparation.derivedSha256)
        ])
        return wire
    }
}

/// Reuses the GUI's durable native upload journal and transfer owner.
@MainActor
enum NativeAutomationArtworkTransfer {
    static func run(bridge: NativeBridge, context: Context, state initial: NativeUploadState,
                    intent: NativeImportIntent, file: ImportFileSnapshot, derived: NativePreparedJPEG? = nil,
                    transport: any NativeImportTransport) async throws -> NativeImportJob {
        let glb = intent.mime == "model/gltf-binary" && intent.derived == nil && intent.coarsePreparation == nil && derived == nil
        let reviewed = intent.mime == "image/jpeg" && intent.coarsePreparation?.valid == true
            && intent.derived?.recipe.colourConversion != nil && derived?.derived == intent.derived && intent.scaleMeters == 1
        let product = intent.productImagePreparation?.matches(intent) == true && derived == nil
        guard glb || reviewed || product, intent.modelDerivation == nil,
              intent.modelPreparation.map({ $0.matches(intent) }) ?? true,
              intent.hayesOriginal == nil, intent.productImagePreparation == nil || product,
              intent.scaleMeters.isFinite, intent.scaleMeters > 0, intent.scaleMeters <= 1_000_000,
              initial.intent == nil || initial.intent == intent else { throw BridgeFailure("IMPORT_INTENT_CHANGED") }
        var state = initial
        let transfer = NativeImportTransfer()
        do { return try await transfer.run(intent: intent, jobID: state.jobId, file: file, derived: derived, transport: transport,
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
        } catch let error as AuthoringFailure where product && NativeProductImagePreparationIntent.definitivelyRejectedCreate(error) {
            if state.intent == intent, state.jobId == nil, !Task.isCancelled {
                state.intent = nil; state.fileName = nil
                _ = try await bridge.saveUploadState(context, state: state)
            }
            throw error
        }
    }
}
