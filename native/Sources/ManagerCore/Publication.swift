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
    public func acceptingStatus(_ response: Self, requiredStatus: String? = nil) throws -> Self {
        _ = try validated(); _ = try response.validated()
        guard response.publicationId == publicationId, response.publicUrl == publicUrl,
              response.revisionSha256 == revisionSha256, response.publishedAt == publishedAt,
              requiredStatus.map({ response.status == $0 }) ?? true else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        return response
    }
}
public struct NativePublicationPending: Codable, Equatable, Sendable {
    public let mode: String?
    public let capabilityPublicationId: String?
    public let sourceExhibitionId: String, sourceEtag: String
    public let snapshot: NativeExhibitionDraft
    public let saveRequestId: String, publishRequestId: String
    public let planBindingSha256: String?
    public var snapshotEtag: String?
    public init(source: NativeExhibitionDraft, etag: String, planBindingSha256: String? = nil) throws {
        guard validPublicationETag(etag), validPublicationPlanBinding(planBindingSha256) else { throw AuthoringFailure("ETAG_INVALID") }
        mode = nil; capabilityPublicationId = nil; sourceExhibitionId = source.id; sourceEtag = etag
        self.planBindingSha256 = planBindingSha256
        snapshot = try source.copyForPublication()
        saveRequestId = UUID().uuidString.lowercased(); publishRequestId = UUID().uuidString.lowercased()
    }
    /// Reuse the already saved authoring row only after a validated server
    /// capability. No snapshot creation or scoped-asset rebinding occurs.
    public init(reusableSource source: NativeExhibitionDraft, etag: String, capabilityPublicationId: String, planBindingSha256: String? = nil) throws {
        guard UUID(uuidString: capabilityPublicationId) != nil, validPublicationETag(etag), validPublicationPlanBinding(planBindingSha256),
              source.document["editVersion"].integer.map({ $0 > 0 && $0 <= 2_147_483_647 }) == true else { throw AuthoringFailure("ETAG_INVALID") }
        mode = "reusable-draft-v1"; self.capabilityPublicationId = capabilityPublicationId; sourceExhibitionId = source.id; sourceEtag = etag
        self.planBindingSha256 = planBindingSha256; snapshot = source; snapshotEtag = etag
        saveRequestId = UUID().uuidString.lowercased(); publishRequestId = UUID().uuidString.lowercased()
    }
    public func requirePlanBinding(_ expected: String?) throws {
        guard validPublicationPlanBinding(planBindingSha256), validPublicationPlanBinding(expected), planBindingSha256 == expected else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
    }
    /// Compare an authenticated GET result; never replay the uncertain POST or
    /// replace its snapshot/request IDs. The caller retains generation and CAS.
    public func confirmingSavedSnapshot(_ saved: NativeStudioResponse) throws -> Self {
        guard mode == nil, snapshotEtag == nil else { throw AuthoringFailure("PUBLICATION_PENDING") }
        guard saved.revision == 1, validPublicationETag(saved.etag),
              saved.etag.hasPrefix("\"studio-r1-"), saved.draft == snapshot else {
            throw AuthoringFailure("REMOTE_CONFLICT", status: 409)
        }
        var next = self; next.snapshotEtag = saved.etag
        return next
    }
}
public struct NativePublicationCompleted: Codable, Equatable, Sendable {
    public let mode: String?
    public let sourceExhibitionId: String, sourceEtag: String, snapshotExhibitionId: String, publishRequestId: String
    public var publication: NativePublication
    public let planBindingSha256: String?
    public init(pending: NativePublicationPending, publication: NativePublication) {
        mode = pending.mode; sourceExhibitionId = pending.sourceExhibitionId; sourceEtag = pending.sourceEtag
        snapshotExhibitionId = pending.snapshot.id; publishRequestId = pending.publishRequestId; self.publication = publication
        planBindingSha256 = pending.planBindingSha256
    }
    public func isReusable(sourceExhibitionId: String, etag: String, planBindingSha256 expected: String?) -> Bool {
        publication.status == "published" && refersToSource(sourceExhibitionId: sourceExhibitionId, etag: etag, planBindingSha256: expected)
    }
    public func refersToSource(sourceExhibitionId: String, etag: String, planBindingSha256 expected: String?) -> Bool {
        self.sourceExhibitionId == sourceExhibitionId && sourceEtag == etag
            && validPublicationPlanBinding(planBindingSha256) && validPublicationPlanBinding(expected) && planBindingSha256 == expected
    }
}
public struct NativePublicationState: Codable, Equatable, Sendable {
    public let format: Int
    public var revision: UInt64
    public let workspaceId: String, tenantId: String, principalId: String
    public var pending: NativePublicationPending?
    public var completed: NativePublicationCompleted?
}

/// Local correspondence only. Authentication belongs to AuthoringAPI and the
/// durable write still uses the helper's original scope/revision CAS.
public struct NativePendingPublicationSnapshotBinding: Sendable {
    private let original: NativePublicationState
    private let generation: UUID
    public init(_ state: NativePublicationState, generation: UUID) throws {
        guard state.format == 1, let pending = state.pending, pending.snapshotEtag == nil else {
            throw AuthoringFailure("PUBLICATION_PENDING")
        }
        original = state; self.generation = generation
    }
    public func accepting(_ saved: NativeStudioResponse, in current: NativePublicationState, generation: UUID) throws -> NativePublicationState {
        guard current == original, generation == self.generation, let pending = current.pending else {
            throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED")
        }
        var next = current; next.pending = try pending.confirmingSavedSnapshot(saved)
        return next
    }
}

/// A status reply may update only the exact retained preparation and journal
/// revision that issued it. No new durable action or publication is invented.
public struct NativePublicationStatusBinding: Sendable {
    private let original: NativePublicationState
    public let completed: NativePublicationCompleted
    public init(_ state: NativePublicationState) throws {
        guard state.format == 1, state.pending == nil, let completed = state.completed else { throw AuthoringFailure("PUBLICATION_PENDING") }
        _ = try completed.publication.validated()
        original = state; self.completed = completed
    }
    public func replacingPublication(_ response: NativePublication, in current: NativePublicationState) throws -> NativePublicationState {
        guard current == original else { throw AuthoringFailure("PUBLICATION_SCOPE_CHANGED") }
        var value = current, changed = completed
        changed.publication = try completed.publication.acceptingStatus(response)
        value.completed = changed
        return value
    }
}
func validPublicationETag(_ value: String) -> Bool { value.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil }
func validPublicationPlanBinding(_ value: String?) -> Bool { value.map(nativeImportDigest) ?? true }

/// Cache identity only, never an authorization or execution capability. The
/// exact runtime artifact pins publication quality/derivation policy; there is
/// no separate native quality selector. Normalize only effective watermark data.
public func nativePublicationPlanBinding(sourceExhibitionId: String, etag: String, runtimeManifestSha256: String, watermark: PublicationWatermark?) throws -> String {
    guard UUID(uuidString: sourceExhibitionId) != nil, validPublicationETag(etag), nativeImportDigest(runtimeManifestSha256) else { throw AuthoringFailure("PUBLICATION_PLAN_CHANGED") }
    let settings = watermark ?? PublicationWatermark(); try settings.validate()
    let record: DocumentValue = .object(["version": .integer(1), "sourceExhibitionId": .string(sourceExhibitionId), "sourceEtag": .string(etag),
        "runtimeManifestSha256": .string(runtimeManifestSha256), "effectiveWatermark": settings.value])
    let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    return nativeImportSHA256(try encoder.encode(record))
}

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
