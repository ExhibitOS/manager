// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativePublicationIssue: Decodable, Sendable {
    public let code: String, path: String, message: String, remediation: String
}
public struct NativePublicationReady: Decodable, Sendable {
    public let status: String, etag: String
    public let issues: [NativePublicationIssue]
    public func validated() throws -> Self {
        guard ["READY","BLOCKED"].contains(status), validPublicationETag(etag), issues.count <= 1024,
              (status == "READY") == issues.isEmpty,
              issues.allSatisfy({ $0.code.range(of: "^[A-Z0-9_]{1,80}$", options: .regularExpression) != nil && $0.path.count <= 2048 && $0.message.count <= 4096 && $0.remediation.count <= 4096 }) else { throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID") }
        return self
    }
}
public struct NativePublication: Codable, Equatable, Sendable {
    public let publicationId: String, publicUrl: String, revisionSha256: String, publishedAt: String, status: String
    public func validated() throws -> Self {
        let formatter = ISO8601DateFormatter(); formatter.formatOptions = [.withInternetDateTime,.withFractionalSeconds]
        guard UUID(uuidString: publicationId) != nil, publicUrl == "/p/" + publicationId,
              revisionSha256.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil,
              ["published","unpublished"].contains(status), publishedAt.count <= 64,
              formatter.date(from: publishedAt) != nil || ISO8601DateFormatter().date(from: publishedAt) != nil else { throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID") }
        return self
    }
}
public struct NativePublicationPending: Codable, Equatable, Sendable {
    public let sourceExhibitionId: String, sourceEtag: String
    public let snapshot: NativeExhibitionDraft
    public let saveRequestId: String, publishRequestId: String
    public var snapshotEtag: String?
    public init(source: NativeExhibitionDraft, etag: String) throws {
        guard validPublicationETag(etag) else { throw AuthoringFailure("ETAG_INVALID") }
        sourceExhibitionId = source.id; sourceEtag = etag
        snapshot = try source.copyForPublication()
        saveRequestId = UUID().uuidString.lowercased(); publishRequestId = UUID().uuidString.lowercased()
    }
}
public struct NativePublicationCompleted: Codable, Sendable {
    public let sourceExhibitionId: String, sourceEtag: String, snapshotExhibitionId: String, publishRequestId: String
    public var publication: NativePublication
    public init(pending: NativePublicationPending, publication: NativePublication) {
        sourceExhibitionId = pending.sourceExhibitionId; sourceEtag = pending.sourceEtag
        snapshotExhibitionId = pending.snapshot.id; publishRequestId = pending.publishRequestId; self.publication = publication
    }
}
public struct NativePublicationState: Codable, Sendable {
    public let format: Int
    public var revision: UInt64
    public let workspaceId: String, tenantId: String, principalId: String
    public var pending: NativePublicationPending?
    public var completed: NativePublicationCompleted?
}
func validPublicationETag(_ value: String) -> Bool { value.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil }

extension NativeExhibitionDraft {
    /// The existing publication API freezes its source exhibition. A separate
    /// private copy retains that contract while the user's authoring draft stays editable.
    public func copyForPublication(now: Date = Date()) throws -> Self { try copyWithNewIdentity(suffix: " · 공개용 저장본", now: now) }
    public func copyForEditing(now: Date = Date()) throws -> Self { try copyWithNewIdentity(suffix: " · 편집 사본", now: now) }
    private func copyWithNewIdentity(suffix: String, now: Date) throws -> Self {
        var value = document, candidate = self.candidate
        let id = UUID().uuidString.lowercased(), time = ISO8601DateFormatter().string(from: now)
        value["id"] = .string(UUID().uuidString.lowercased()); value["exhibitionId"] = .string(id)
        value["editVersion"] = .integer(1); value["createdAt"] = .string(time); value["updatedAt"] = .string(time)
        candidate["id"] = .string(id); candidate["revisionId"] = .string(UUID().uuidString.lowercased())
        candidate["revision"] = .integer(1); candidate["createdAt"] = .string(time)
        candidate["title"] = .string(String(title.prefix(480)) + suffix)
        candidate = try InstallationSettings.remappingPlacementReferences(in: candidate)
        value["candidate"] = candidate
        return try Self(document: value)
    }
}
