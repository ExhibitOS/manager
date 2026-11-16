// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Binds an advisory Server read to this exact local draft and saved CAS base.
/// This is not a write, a permission grant, or a Native budget calculation.
public struct ViewerBudgetAnalysisRequest: Sendable {
    public let draft: NativeExhibitionDraft
    public let etag: String
    public let savedRevision: Int
    public let requestedProfile: String?
    public let body: DocumentValue

    public init(document: NativeDocumentState, requestedProfile: String? = nil,
                context: String = "viewer", automaticProfile: String = "standard",
                requestId: String = UUID().uuidString.lowercased()) throws {
        guard document.pending == nil, let draft = document.draft, let etag = document.etag,
              etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil,
              let revision = Int(etag.dropFirst(9).prefix(while: { $0 != "-" })), (1...Int(Int32.max)).contains(revision),
              UUID(uuidString: requestId) != nil, ["viewer", "native"].contains(context),
              ["standard", "compact"].contains(automaticProfile),
              requestedProfile.map({ ViewerBudgetSettings.profiles.contains($0) }) ?? true else { throw AuthoringFailure("VIEWER_BUDGET_ANALYSIS_INPUT_INVALID") }
        self.draft = draft; self.etag = etag; savedRevision = revision; self.requestedProfile = requestedProfile
        var options: [String: DocumentValue] = ["context": .string(context), "automaticProfile": .string(automaticProfile)]
        if let requestedProfile { options["profile"] = .string(requestedProfile) }
        body = .object(["requestId": .string(requestId), "expectedRevision": .integer(Int64(revision)),
            "draft": draft.document, "options": .object(options)])
    }

    public func checkedReply(_ value: DocumentValue) throws -> DocumentValue {
        guard value["version"] == .integer(1), value["policyVersion"] == .integer(1), value["accountingVersion"] == .integer(1),
              value["binding"]["exhibitionId"] == .string(draft.id), value["binding"]["savedRevision"] == .integer(Int64(savedRevision)),
              value["binding"]["etag"] == .string(etag), value["binding"]["draftId"] == draft.document["id"],
              value["binding"]["draftEditVersion"] == draft.document["editVersion"], value["binding"]["source"] == .string("draft"),
              let input = value["binding"]["inputSHA256"].string, nativeImportDigest(input),
              let policy = value["policySHA256"].string, nativeImportDigest(policy),
              let effective = value["selection"]["effectiveProfile"].string, ViewerBudgetSettings.profiles.contains(effective),
              requestedProfile == nil || effective == requestedProfile,
              value["resolvedPolicy"]["profile"] == .string(effective),
              value["resolvedPolicy"]["policyVersion"] == .integer(1), value["resolvedPolicy"]["accountingVersion"] == .integer(1),
              value["estimates"]["wholeExhibition"]["evaluation"]["profile"] == .string(effective),
              value["estimates"]["wholeExhibition"]["evaluation"]["scope"] == .string("wholeExhibition"),
              case .bool = value["analysisComplete"] else { throw AuthoringFailure("VIEWER_BUDGET_ANALYSIS_STALE") }
        return value
    }
}

/// Read-only analysis of the exact prepared public projection used by static export.
public struct ViewerBudgetPublicationRequest: Equatable, Sendable {
    public let publicationId: String
    public let revisionSha256: String

    public init(publicationId: String, revisionSha256: String) throws {
        guard UUID(uuidString: publicationId)?.uuidString.lowercased() == publicationId,
              nativeImportDigest(revisionSha256) else {
            throw AuthoringFailure("VIEWER_BUDGET_ANALYSIS_INPUT_INVALID")
        }
        self.publicationId = publicationId
        self.revisionSha256 = revisionSha256
    }

    public init(publication: NativePublication) throws {
        guard try publication.validated().status == "published" else {
            throw AuthoringFailure("VIEWER_BUDGET_ANALYSIS_STALE")
        }
        try self.init(publicationId: publication.publicationId, revisionSha256: publication.revisionSha256)
    }

    public func checkedReply(_ value: DocumentValue) throws -> DocumentValue {
        let binding = value["binding"]
        let target = value["estimateTarget"]
        let evaluation = value["estimates"]["wholeExhibition"]["evaluation"]
        guard value["version"] == .integer(1), value["policyVersion"] == .integer(1),
              value["accountingVersion"] == .integer(1),
              binding["source"] == .string("publication"), binding["publicationId"] == .string(publicationId),
              binding["revisionSha256"] == .string(revisionSha256),
              binding["inputSHA256"] == .string(revisionSha256),
              let assets = binding["projectionAssetsSHA256"].string, nativeImportDigest(assets),
              let policy = value["policySHA256"].string, nativeImportDigest(policy),
              let effective = value["selection"]["effectiveProfile"].string, ["standard", "compact"].contains(effective),
              value["resolvedPolicy"]["profile"] == .string(effective),
              value["resolvedPolicy"]["policyVersion"] == .integer(1),
              value["resolvedPolicy"]["accountingVersion"] == .integer(1),
              value["exportConfiguration"]["performanceAdvisoryOnly"] == .bool(true),
              evaluation["profile"] == .string(effective), evaluation["scope"] == .string("wholeExhibition"),
              case .bool = value["analysisComplete"] else {
            throw AuthoringFailure("VIEWER_BUDGET_ANALYSIS_STALE")
        }
        guard target["scenario"] == .string("fine-pointer-no-memory-hint-auto-v1"),
              target["requestedMode"] == .string("auto"),
              target["hints"]["finePointer"] == .bool(true), target["hints"]["coarsePointer"] == .bool(false),
              target["selectionBudget"]["maxTriangles"].integer.map({ $0 > 0 }) == true,
              target["selectionBudget"]["maxTextureSize"].integer.map({ $0 > 0 }) == true else {
            throw AuthoringFailure("VIEWER_BUDGET_PRODUCER_UNAVAILABLE")
        }
        return value
    }
}
