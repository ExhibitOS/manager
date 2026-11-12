// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Transient correspondence for one explicitly selected stand/plaque pair.
/// The caller obtains the prospective draft and current registered furnishing
/// through AuthoringAPI's fresh authorization/validation path. This type does
/// not issue authority, prove physical fit, stage a document or save one.
public struct WorkspaceInfoStandPreview: Sendable {
    public let base: NativeDocumentState
    public let replacement: WorkspaceInfoStandReplacement
    public let furnishing: WorkspaceFurnishing
    /// One checked value for both display and apply; never regenerate in a view.
    public let displayDraft: NativeExhibitionDraft
    public let changed: Bool
    private let baseValue: DocumentValue

    public init(base: NativeDocumentState, replacement: WorkspaceInfoStandReplacement,
                furnishing: WorkspaceFurnishing, supportedExtensionVersion: Int64,
                prospectiveDraft: NativeExhibitionDraft?) throws {
        guard base.pending == nil, !base.dirty, let draft = base.draft,
              let etag = base.etag, !etag.isEmpty,
              base.savedEditVersion == draft.document["editVersion"].integer,
              [base.workspaceId, base.tenantId, base.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else {
            throw AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_UNAVAILABLE")
        }
        let expected = try replacement.applying(furnishing, to: draft, supportedExtensionVersion: supportedExtensionVersion)
        let display: NativeExhibitionDraft
        if let expected {
            guard let prospectiveDraft,
                  Self.nativeTimestamp(prospectiveDraft.document["updatedAt"]) else {
                throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED")
            }
            // The API and this second internal derivation can cross a clock
            // tick. All other draft fields, including identity, edit counter,
            // createdAt and the complete candidate, must remain exact.
            var comparison = prospectiveDraft.document
            comparison["updatedAt"] = expected.document["updatedAt"]
            guard comparison == expected.document else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            display = prospectiveDraft
        } else {
            guard prospectiveDraft == nil || prospectiveDraft == draft else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            display = draft
        }
        self.base = base; self.replacement = replacement; self.furnishing = furnishing
        displayDraft = display; changed = expected != nil
        baseValue = try Self.value(base)
    }

    public func apply(to current: NativeDocumentState) throws -> NativeExhibitionDraft {
        guard try Self.value(current) == baseValue else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return displayDraft
    }

    private static func value(_ state: NativeDocumentState) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(state))
    }
    private static func nativeTimestamp(_ value: DocumentValue) -> Bool {
        guard let text = value.string else { return false }
        let formatter = ISO8601DateFormatter()
        guard let date = formatter.date(from: text) else { return false }
        return formatter.string(from: date) == text
    }
}
