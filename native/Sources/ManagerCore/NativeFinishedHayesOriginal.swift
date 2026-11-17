// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A server-resolved finished release. Custody alone grants no publication rights.
public struct NativeFinishedHayesCustody: Codable, Equatable, Sendable {
    public static let profile = "finished-hayes-custody-v1"
    public struct Asset: Codable, Equatable, Sendable {
        public let assetId: String, sha256: String, mime: String
        public let bytes: Int
    }
    public struct Release: Codable, Equatable, Sendable {
        public let attemptId: String, receiptId: String, receiptSha256: String, completionFrameSha256: String
    }
    public let version: Int
    public let kind: String
    public let workspaceId: String, tenantId: String, principalId: String
    public let artworkId: String, artworkRevisionId: String, importJobId: String
    public let artworkRevision: Int
    public let release: Release
    public let original: Asset, display: Asset
    public let rightsSha256: String

    public func validate(in state: NativeDocumentState, artwork: DocumentValue) throws {
        guard version == 1, kind == Self.profile,
              workspaceId == state.workspaceId, tenantId == state.tenantId, principalId == state.principalId,
              [workspaceId, tenantId, principalId, artworkId, artworkRevisionId, importJobId,
               release.attemptId, release.receiptId, original.assetId, display.assetId].allSatisfy({ UUID(uuidString: $0)?.uuidString.lowercased() == $0 }),
              (1...Int(Int32.max)).contains(artworkRevision),
              [release.receiptSha256, release.completionFrameSha256, original.sha256, display.sha256, rightsSha256].allSatisfy(nativeImportDigest),
              original.mime == "model/gltf-binary", display.mime == "model/gltf-binary",
              original.bytes > 0, display.bytes > 0, original.assetId != display.assetId,
              artwork["id"] == .string(artworkId), artwork["revision"].integer == Int64(artworkRevision),
              artwork["revisionId"] == .string(artworkRevisionId),
              artwork["primaryAssetId"] == .string(display.assetId),
              let primary = artwork["assets"].items?.first(where: { $0["id"] == .string(display.assetId) }),
              primary["sha256"] == .string(display.sha256), primary["bytes"].integer == Int64(display.bytes),
              primary["mime"] == .string(display.mime) else {
            throw AuthoringFailure("FINISHED_HAYES_ORIGINAL_UNAVAILABLE")
        }
    }
}

public struct NativeFinishedHayesOriginalApproval: Codable, Equatable, Sendable {
    public let version: Int
    public let approved: Bool
    public let exhibitionId: String
    public let revision: Int
    public let etag: String
    public let custody: NativeFinishedHayesCustody
    public let approvalSha256: String?

    func validate(custody expected: NativeFinishedHayesCustody, exhibitionId: String, revision: Int, etag: String) throws {
        guard version == 1, self.exhibitionId == exhibitionId, self.revision == revision, self.etag == etag,
              custody == expected, approved == (approvalSha256 != nil),
              approvalSha256.map(nativeImportDigest) ?? !approved else {
            throw AuthoringFailure("FINISHED_HAYES_ORIGINAL_APPROVAL_INVALID")
        }
    }
}
