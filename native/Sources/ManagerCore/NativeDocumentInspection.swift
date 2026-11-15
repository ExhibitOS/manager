// SPDX-License-Identifier: Apache-2.0
import Foundation

/// One existing-profile journal read. This does not open an authoring session
/// or refresh Native status, whose normal path may settle owned operations.
public enum NativeDocumentInspection {
    struct Paths {
        let helper: URL, profile: URL
    }

    static func paths(_ arguments: [String]) throws -> Paths {
        guard arguments.count == 6, arguments[1] == "--inspect-existing-document",
              arguments[2] == "--helper", arguments[4] == "--profile" else {
            throw BridgeFailure("NATIVE_DOCUMENT_INSPECTION_ARGUMENTS_INVALID")
        }
        func existingPath(_ path: String) throws -> URL {
            guard path.hasPrefix("/"), path.utf8.count <= 4096,
                  !path.unicodeScalars.contains(where: { $0.value < 32 || $0.value == 127 }) else {
                throw BridgeFailure("NATIVE_DOCUMENT_INSPECTION_PATH_INVALID")
            }
            let url = URL(fileURLWithPath: path)
            guard url.standardizedFileURL.path == path, url.resolvingSymlinksInPath().path == path,
                  FileManager.default.fileExists(atPath: path) else {
                throw BridgeFailure("NATIVE_DOCUMENT_INSPECTION_PATH_INVALID")
            }
            return url
        }
        return try Paths(helper: existingPath(arguments[3]), profile: existingPath(arguments[5]))
    }

    static func summary(_ document: NativeDocumentState) throws -> DocumentValue {
        // A missing journal is an in-memory revision-zero default in the
        // existing helper. Do not represent it as an inspected saved document.
        guard document.format == 1, document.revision > 0, document.revision <= UInt64(Int64.max),
              [document.workspaceId, document.tenantId, document.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else {
            throw BridgeFailure("NATIVE_EXISTING_DOCUMENT_REQUIRED")
        }
        let pending: DocumentValue
        if let save = document.pending {
            pending = .object(["requestId": .string(save.requestId),
                "origin": save.origin.map(DocumentValue.string) ?? .null,
                "etag": save.etag.map(DocumentValue.string) ?? .null])
        } else { pending = .null }
        return .object(["version": .integer(1),
            "workspaceId": .string(document.workspaceId), "tenantId": .string(document.tenantId),
            "principalId": .string(document.principalId), "nativeRevision": .integer(Int64(document.revision)),
            "savedEditVersion": document.savedEditVersion.map(DocumentValue.integer) ?? .null,
            "draftEditVersion": document.draft?.document["editVersion"].integer.map(DocumentValue.integer) ?? .null,
            "exhibitionId": document.draft.map { .string($0.id) } ?? .null,
            "etag": document.etag.map(DocumentValue.string) ?? .null,
            "dirty": .bool(document.dirty), "pendingSave": pending,
            "liveRuntimeOwnership": .string("unknown"),
            "applyOperationAttribution": NativeAppliedDraftSave.retainedRequestId(in: document).map {
                .object(["requestId": .string($0), "origin": .string(NativeAppliedDraftSave.origin)])
            } ?? .string("unknown")])
    }

    public static func inspect(arguments: [String]) async throws -> DocumentValue {
        let input = try paths(arguments)
        let bridge = try NativeBridge(helper: input.helper, profile: input.profile, requireExistingManagedProfile: true)
        let result: DocumentValue
        do {
            let context = try await bridge.context()
            result = try summary(await bridge.documentState(context))
        } catch {
            try? await bridge.close()
            throw error
        }
        // Close only the never-started helper through EOF. Session.close would
        // consult Native status and may start normal shutdown/reconciliation.
        try await bridge.close()
        return result
    }
}
