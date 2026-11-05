// SPDX-License-Identifier: Apache-2.0
import Foundation

/// The selected owner offers the same save handshake used by ManagerApp. This
/// adapter never edits importer JSON or adopts an archive's scoped asset rows.
public enum NativeSelectedOEXCompletion {
    public static func finish(bridge: NativeBridge, context: Context, api: AuthoringAPI,
                              original: NativeOEXImportState, jobId: String) async throws -> NativeDocumentState {
        var offer: NativeOEXSaveOffer?
        var reopened: NativeStudioResponse?
        for _ in 0..<2400 {
            try Task.checkCancellation()
            let status = try await bridge.nativeStatus(context)
            if let offered = status.oexSaveOffer, offer == nil {
                offer = offered
                let saved = try await api.saveAndReopenSelectedOEX(exhibitionId: offered.exhibitionId, expectedETag: offered.etag, requestId: offered.requestId) { source, requestId in
                    var document = try await bridge.documentState(context)
                    guard document.workspaceId == original.workspaceId, document.tenantId == original.tenantId,
                          document.principalId == original.principalId, document.draft == nil, document.pending == nil,
                          document.etag == nil, document.savedEditVersion == nil,
                          let version = source.draft.document["editVersion"].integer else { throw BridgeFailure("EXHIBITION_PENDING") }
                    document.draft = source.draft; document.etag = source.etag; document.savedEditVersion = version
                    document.pending = .init(requestId: requestId, etag: source.etag, draft: source.draft, origin: "bounded-import-v1")
                    let stored = try await bridge.saveDocumentState(context, state: document)
                    let checked = try await bridge.documentState(context)
                    guard checked.revision == stored.revision, checked.workspaceId == original.workspaceId,
                          checked.tenantId == original.tenantId, checked.principalId == original.principalId,
                          checked.pending?.requestId == requestId, checked.pending?.origin == "bounded-import-v1",
                          checked.pending?.etag == source.etag, checked.pending?.draft == source.draft,
                          checked.draft == source.draft else { throw BridgeFailure("EXHIBITION_SCOPE_CHANGED") }
                    try await bridge.authorizeBoundedOEXSave(context, offer: offered)
                }
                reopened = saved
                try await bridge.reportBoundedOEXSave(context, offer: offered, etag: saved.etag)
            }
            if !status.busy {
                guard status.errorCode == nil, let offer, let reopened else { throw BridgeFailure(status.errorCode ?? "OEX_IMPORT_INVALID") }
                let completed = try await api.oexImportJob(jobId)
                guard completed.id == jobId, completed.state == "complete",
                      completed.result?.exhibitionId.lowercased() == offer.exhibitionId.lowercased() else { throw BridgeFailure("OEX_IMPORT_RESPONSE_INVALID") }
                var document = try await bridge.documentState(context)
                guard document.workspaceId == original.workspaceId, document.tenantId == original.tenantId,
                      document.principalId == original.principalId, document.pending?.requestId == offer.requestId,
                      document.pending?.etag == offer.etag, document.pending?.draft == reopened.draft,
                      document.draft == reopened.draft, let version = reopened.draft.document["editVersion"].integer else { throw BridgeFailure("EXHIBITION_SCOPE_CHANGED") }
                document.etag = reopened.etag; document.savedEditVersion = version; document.pending = nil
                let stored = try await bridge.saveDocumentState(context, state: document)
                let cleared = try await bridge.oexImportState(context)
                guard stored.pending == nil, stored.draft == reopened.draft, stored.etag == reopened.etag,
                      cleared.intent == nil, cleared.jobId == nil else { throw BridgeFailure("OEX_IMPORT_SCOPE_CHANGED") }
                return stored
            }
            try await Task.sleep(for: .milliseconds(250))
        }
        throw BridgeFailure("NATIVE_TIMEOUT")
    }
}
