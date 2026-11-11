// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Server-resolved custody for the current CMS selection. It contains no blob
/// key, local transfer state or adoption authority.
public struct NativeRequiredModelCustody: Codable, Equatable, Sendable {
    public static let profile = "required-model-custody-v1"
    public struct Asset: Codable, Equatable, Sendable {
        public let assetId: String, mime: String, sha256: String
        public let bytes: Int
    }
    public struct Display: Codable, Equatable, Sendable {
        public let assetId: String, mime: String, sha256: String
        public let bytes: Int
        public let reusedOriginal: Bool
    }
    public let version: Int
    public let kind: String
    public let workspaceId: String, tenantId: String, principalId: String
    public let artworkId: String, artworkRevisionId: String, importJobId: String
    public let artworkRevision: Int
    public let serverReceiptSha256: String, rightsSha256: String
    public let original: Asset
    public let display: Display

    public func validate(in state: NativeDocumentState, artwork: DocumentValue) throws {
        guard version == 1, kind == Self.profile,
              workspaceId == state.workspaceId, tenantId == state.tenantId, principalId == state.principalId,
              [workspaceId, tenantId, principalId, artworkId, artworkRevisionId, importJobId,
               original.assetId, display.assetId].allSatisfy({ UUID(uuidString: $0)?.uuidString.lowercased() == $0 }),
              (1...Int(Int32.max)).contains(artworkRevision),
              [serverReceiptSha256, rightsSha256, original.sha256, display.sha256].allSatisfy(nativeImportDigest),
              original.mime == "model/gltf-binary", display.mime == "model/gltf-binary",
              original.bytes > 0, display.bytes > 0, !display.reusedOriginal,
              original.assetId != display.assetId,
              artwork["id"] == .string(artworkId), artwork["revision"].integer == Int64(artworkRevision),
              artwork["revisionId"] == .string(artworkRevisionId),
              artwork["primaryAssetId"] == .string(display.assetId) else {
            throw AuthoringFailure("REQUIRED_MODEL_ORIGINAL_UNAVAILABLE")
        }
    }
}

public struct NativeRequiredModelOriginalApproval: Codable, Equatable, Sendable {
    public let version: Int
    public let approved: Bool
    public let exhibitionId: String
    public let revision: Int
    public let etag: String
    public let custody: NativeRequiredModelCustody
    public let approvalSha256: String?

    func validate(custody expected: NativeRequiredModelCustody, exhibitionId: String,
                  revision: Int, etag: String) throws {
        guard version == 1, self.exhibitionId == exhibitionId, self.revision == revision,
              self.etag == etag, custody == expected,
              approved == (approvalSha256 != nil),
              approvalSha256.map(nativeImportDigest) ?? !approved else {
            throw AuthoringFailure("REQUIRED_MODEL_ORIGINAL_APPROVAL_INVALID")
        }
    }
}
