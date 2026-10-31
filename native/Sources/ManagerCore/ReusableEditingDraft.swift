// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Server-owned, read-only correspondence between a publication and the one
/// authoring row for its lineage. This response is never a write authority.
public struct NativeReusableEditingDraft: Decodable, Sendable {
    public let capability: String
    public let sourcePublicationId: String
    public let authoringExhibitionId: String
    public let reused: Bool
    public let revision: Int
    public let etag: String
    public let draft: NativeExhibitionDraft
    public let currentPublication: NativePublication?
    public let previousPublication: NativePublication?

    public func validated(expectedPublicationId: String) throws -> Self {
        guard capability == "reusable-published-draft-v1", reused,
              let expected = UUID(uuidString: expectedPublicationId),
              let source = UUID(uuidString: sourcePublicationId), expected == source,
              let authoring = UUID(uuidString: authoringExhibitionId),
              let actual = UUID(uuidString: draft.id), authoring == actual,
              revision > 0, revision <= 2_147_483_647,
              validPublicationETag(etag), etag.hasPrefix("\"studio-r\(revision)-") else {
            throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED")
        }
        if let currentPublication { _ = try currentPublication.validated() }
        if let previousPublication {
            _ = try previousPublication.validated()
            guard let currentPublication, currentPublication.publicationId.lowercased() != previousPublication.publicationId.lowercased() else {
                throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID")
            }
        }
        return self
    }
}
