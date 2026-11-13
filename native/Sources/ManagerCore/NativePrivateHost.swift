// SPDX-License-Identifier: Apache-2.0
import Foundation

/// An ordinary remote exhibition, held only in the current operator session.
/// This value never writes the selected native document or its transfer journals.
struct NativePrivateHost: Sendable {
    let creationRequestId: String
    private let protectedBytes: Data
    private let protectedExhibitionId: String?
    private(set) var document: NativeDocumentState
    private(set) var serverRevision: Int?
    private(set) var savedDraft: NativeExhibitionDraft?

    static func bytes(_ state: NativeDocumentState) throws -> Data {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(state)
    }
    init(requestId: String, title: String, protecting current: NativeDocumentState,
         roomBaseLight: Bool) throws {
        guard UUID(uuidString: requestId)?.uuidString.lowercased() == requestId,
              !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, title.utf16.count <= 512,
              current.pending == nil, !current.dirty,
              [current.workspaceId,current.tenantId,current.principalId].allSatisfy({ UUID(uuidString: $0)?.uuidString.lowercased() == $0 }) else {
            throw BridgeFailure("PRIVATE_EXHIBITION_SCOPE_CHANGED")
        }
        creationRequestId = requestId; protectedBytes = try Self.bytes(current)
        protectedExhibitionId = current.draft?.id
        let draft = try NativeExhibitionDraft.new(title: title, roomBaseLight: roomBaseLight)
        guard draft.id != protectedExhibitionId else { throw BridgeFailure("PRIVATE_EXHIBITION_SCOPE_CHANGED") }
        document = NativeDocumentState(format: 1, revision: 1, workspaceId: current.workspaceId,
            tenantId: current.tenantId, principalId: current.principalId, draft: draft,
            etag: nil, savedEditVersion: nil, pending: nil)
    }
    func checkProtected(_ current: NativeDocumentState) throws {
        guard try Self.bytes(current) == protectedBytes,
              document.workspaceId == current.workspaceId, document.tenantId == current.tenantId,
              document.principalId == current.principalId, document.draft?.id != protectedExhibitionId else {
            throw BridgeFailure("PRIVATE_EXHIBITION_SCOPE_CHANGED")
        }
    }
    func checkTarget(id: String, revision: UInt64) throws {
        guard id == document.draft?.id, revision == document.revision else {
            throw BridgeFailure("PRIVATE_EXHIBITION_COMPARISON_STALE")
        }
    }
    mutating func prepareSave(requestId: String) throws -> NativeDocumentState.Pending {
        guard UUID(uuidString: requestId)?.uuidString.lowercased() == requestId,
              let draft = document.draft, draft.id != protectedExhibitionId else {
            throw BridgeFailure("PRIVATE_EXHIBITION_SCOPE_CHANGED")
        }
        if let pending = document.pending {
            guard pending.requestId == requestId, pending.draft == draft, pending.etag == document.etag else {
                throw BridgeFailure("PRIVATE_EXHIBITION_PENDING")
            }
            return pending
        }
        let pending = NativeDocumentState.Pending(requestId: requestId, etag: document.etag, draft: draft)
        document.pending = pending
        return pending
    }
    mutating func acceptSaved(_ response: NativeStudioResponse) throws {
        guard let pending = document.pending, response.draft == pending.draft,
              response.draft.id == document.draft?.id, response.draft.id != protectedExhibitionId,
              response.revision == (serverRevision.map { $0 + 1 } ?? 1),
              response.etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil,
              response.etag.hasPrefix("\"studio-r\(response.revision)-"), document.revision < UInt64(Int64.max) else {
            throw BridgeFailure("PRIVATE_EXHIBITION_RESPONSE_INVALID")
        }
        document.draft = response.draft; document.etag = response.etag
        document.savedEditVersion = response.draft.document["editVersion"].integer
        document.pending = nil; document.revision += 1; serverRevision = response.revision; savedDraft = response.draft
    }
    func checkRemote(_ response: NativeStudioResponse) throws {
        guard response.revision == serverRevision, response.draft == savedDraft,
              response.etag == document.etag else { throw BridgeFailure("PRIVATE_EXHIBITION_COMPARISON_STALE") }
    }
    func requireSaved(_ response: NativeStudioResponse) throws {
        guard document.pending == nil, !document.dirty, response.revision == serverRevision,
              response.draft == document.draft, response.etag == document.etag else {
            throw BridgeFailure("PRIVATE_EXHIBITION_COMPARISON_STALE")
        }
    }
    mutating func stage(_ draft: NativeExhibitionDraft) throws {
        guard document.pending == nil, let existing = document.draft, draft.id == existing.id,
              draft.document["id"] == existing.document["id"],
              draft.candidate["revisionId"] == existing.candidate["revisionId"],
              document.revision < UInt64(Int64.max) else { throw BridgeFailure("PRIVATE_EXHIBITION_SCOPE_CHANGED") }
        document.draft = draft; document.revision += 1
    }
}
