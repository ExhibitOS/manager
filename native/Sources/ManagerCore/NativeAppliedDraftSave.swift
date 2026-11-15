// SPDX-License-Identifier: Apache-2.0
import Foundation

/// An opt-in scene apply retains its exact Save body in the existing Pending.
/// The caller's original intent is an input, not a second recovery journal.
enum NativeAppliedDraftSave {
    static let origin = "automation-applied-draft-v1"
    static let applyOperations: Set<String> = ["pedestal-apply", "light-apply", "fixture-apply", "light-attachment-apply", "light-object-apply"]

    struct Anchor: Equatable, Sendable {
        let requestId: String, workspaceId: String, tenantId: String, principalId: String
        let exhibitionId: String, runtimeManifestSHA256: String, etag: String
        let sourceNativeRevision: UInt64, sourceEditVersion: Int64
        let candidate: DocumentValue
        let originalDraft: NativeExhibitionDraft?

        init(_ input: DocumentValue) throws {
            let keys: Set<String> = ["version", "requestId", "workspaceId", "tenantId", "principalId", "exhibitionId", "runtimeManifestSHA256", "etag", "sourceNativeRevision", "sourceEditVersion", "candidate"]
            guard case .object(let fields) = input, Set(fields.keys).subtracting(["originalDraft"]) == keys,
                  input["version"] == .integer(1),
                  let request = input["requestId"].string, let workspace = input["workspaceId"].string,
                  let tenant = input["tenantId"].string, let principal = input["principalId"].string,
                  let exhibition = input["exhibitionId"].string,
                  [request, workspace, tenant, principal, exhibition].allSatisfy({ UUID(uuidString: $0)?.uuidString.lowercased() == $0 }),
                  let manifest = input["runtimeManifestSHA256"].string, nativeImportDigest(manifest),
                  let etag = input["etag"].string, let server = Self.serverRevision(etag), server < 2_147_483_647,
                  let native = input["sourceNativeRevision"].integer, native > 0, native <= Int64.max - 2,
                  let edit = input["sourceEditVersion"].integer, (1..<2_147_483_647).contains(edit),
                  case .object = input["candidate"], input["candidate"]["id"] == .string(exhibition) else {
                throw BridgeFailure("APPLIED_DRAFT_INTENT_INVALID")
            }
            requestId = request; workspaceId = workspace; tenantId = tenant; principalId = principal
            exhibitionId = exhibition; runtimeManifestSHA256 = manifest; self.etag = etag
            sourceNativeRevision = UInt64(native); sourceEditVersion = edit; candidate = input["candidate"]
            if let body = fields["originalDraft"] {
                originalDraft = try NativeExhibitionDraft(document: body)
                guard originalDraft?.id == exhibition, originalDraft?.candidate == candidate,
                      originalDraft?.document["editVersion"] == .integer(edit + 1) else {
                    throw BridgeFailure("APPLIED_DRAFT_INTENT_INVALID")
                }
            } else { originalDraft = nil }
        }

        static func serverRevision(_ etag: String) -> Int? {
            guard etag.hasPrefix("\"studio-r"), etag.hasSuffix("\"") else { return nil }
            let parts = etag.dropFirst(9).dropLast().split(separator: "-", maxSplits: 1)
            guard parts.count == 2, !parts[0].hasPrefix("0"), let revision = Int(parts[0]),
                  (1...2_147_483_647).contains(revision), nativeImportDigest(String(parts[1])) else { return nil }
            return revision
        }

        func state(_ current: NativeDocumentState, manifest: String) throws -> Position {
            guard manifest == runtimeManifestSHA256, current.format == 1,
                  current.workspaceId == workspaceId, current.tenantId == tenantId, current.principalId == principalId,
                  let draft = current.draft, draft.id == exhibitionId, draft.candidate == candidate,
                  draft.document["editVersion"] == .integer(sourceEditVersion + 1) else {
                throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT")
            }
            if let pending = current.pending {
                guard current.revision == sourceNativeRevision + 1, current.savedEditVersion == sourceEditVersion,
                      current.etag == etag, pending.origin == origin, pending.requestId == requestId,
                      pending.etag == etag, pending.draft == draft,
                      pending.modelCandidate == nil, pending.productImage == nil,
                      originalDraft == nil || originalDraft == draft else {
                    throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT")
                }
                return .pending
            }
            // A candidate alone cannot reconstruct the original updatedAt or
            // body. A lost final reply needs the caller's exact retained body.
            guard current.revision == sourceNativeRevision + 2, !current.dirty,
                  current.savedEditVersion == sourceEditVersion + 1,
                  let currentETag = current.etag, currentETag != etag,
                  Self.serverRevision(currentETag) == Self.serverRevision(etag).map({ $0 + 1 }),
                  let originalDraft, originalDraft == draft else {
                throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT")
            }
            return .saved
        }

        func checkHead(_ head: NativeStudioResponse, current: NativeDocumentState, manifest: String) throws {
            let position = try state(current, manifest: manifest)
            guard head.draft.id == exhibitionId else { throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT") }
            if position == .pending && head.etag == etag {
                guard head.revision == Self.serverRevision(etag),
                      head.draft.document["editVersion"] == .integer(sourceEditVersion) else {
                    throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT")
                }
            } else {
                guard head.revision == Self.serverRevision(etag).map({ $0 + 1 }),
                      head.etag != etag, head.draft == current.draft,
                      position != .saved || head.etag == current.etag else {
                    throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT")
                }
            }
        }
    }

    enum Position: Equatable { case pending, saved }

    static func retainedRequestId(in state: NativeDocumentState) -> String? {
        guard let pending = state.pending, pending.origin == origin,
              pending.modelCandidate == nil, pending.productImage == nil,
              pending.draft == state.draft, pending.etag == state.etag, pending.etag != nil,
              let saved = state.savedEditVersion, (1..<2_147_483_647).contains(saved),
              pending.draft.document["editVersion"] == .integer(saved + 1),
              UUID(uuidString: pending.requestId)?.uuidString.lowercased() == pending.requestId else { return nil }
        return pending.requestId
    }

    static func stage(_ current: NativeDocumentState, draft: NativeExhibitionDraft, requestId: String) throws -> NativeDocumentState {
        guard !current.dirty, current.pending == nil, let before = current.draft, let etag = current.etag,
              let server = Anchor.serverRevision(etag), server < 2_147_483_647,
              current.revision > 0, current.revision <= UInt64(Int64.max - 2),
              let saved = current.savedEditVersion, (1..<2_147_483_647).contains(saved),
              before.document["editVersion"] == .integer(saved), draft.id == before.id,
              draft.document["editVersion"] == .integer(saved + 1), draft.candidate != before.candidate,
              UUID(uuidString: requestId)?.uuidString.lowercased() == requestId else {
            throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT")
        }
        var next = current; next.draft = draft
        next.pending = .init(requestId: requestId, etag: etag, draft: draft, origin: origin)
        return next
    }

    /// Reuse the ordinary server receipt using its ORIGINAL body/ID/If-Match.
    /// All errors leave the durable Pending to the owning caller unchanged.
    static func replayOriginal(_ anchor: Anchor, current: NativeDocumentState, manifest: String,
                               api: AuthoringAPI) async throws -> NativeStudioResponse {
        _ = try anchor.state(current, manifest: manifest)
        try anchor.checkHead(await api.exhibition(anchor.exhibitionId), current: current, manifest: manifest)
        guard let body = current.pending?.draft ?? anchor.originalDraft else {
            throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT")
        }
        let saved = try await api.saveExhibition(body, requestId: anchor.requestId, etag: anchor.etag)
        guard saved.revision == Anchor.serverRevision(anchor.etag).map({ $0 + 1 }), saved.etag != anchor.etag,
              saved.draft == body else { throw BridgeFailure("APPLIED_DRAFT_RECOVERY_CONFLICT") }
        try saved.requireSameComparison(await api.exhibition(anchor.exhibitionId))
        return saved
    }
}
