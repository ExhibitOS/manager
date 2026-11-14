// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Closed local capture intent. The caller owns runtime admission, rights,
/// output-directory custody, and the existing saved-viewpoint renderer.
public struct NativeSceneCaptureRequest: Decodable, Sendable {
    public let version: Int
    public let requestId: String, workspaceId: String, tenantId: String, principalId: String
    public let runtimeManifestSHA256: String
    public let expectedNativeRevision: UInt64
    public let expectedEditVersion: Int64
    public let exhibitionId: String
    public let serverRevision: Int
    public let expectedETag: String
    public let placementId: String, viewpointId: String, outputDirectory: String

    private enum Keys: String, CodingKey, CaseIterable {
        case version, requestId, workspaceId, tenantId, principalId, runtimeManifestSHA256
        case expectedNativeRevision, expectedEditVersion, exhibitionId, serverRevision, expectedETag
        case placementId, viewpointId, outputDirectory
    }
    private struct AnyKey: CodingKey {
        let stringValue: String
        let intValue: Int? = nil
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { return nil }
    }
    private static func uuid(_ value: String) -> Bool {
        guard let id = UUID(uuidString: value) else { return false }
        return id.uuidString.lowercased() == value.lowercased()
    }
    private static func path(_ value: String) -> Bool {
        guard value.first == "/", !value.unicodeScalars.contains(where: { $0.value == 0 || $0.value < 32 || $0.value == 127 }) else { return false }
        if value == "/" { return true }
        let components = value.dropFirst().split(separator: "/", omittingEmptySubsequences: false)
        return components.allSatisfy { !$0.isEmpty && $0 != "." && $0 != ".." }
    }
    public init(from decoder: Decoder) throws {
        do {
            let raw = try decoder.container(keyedBy: AnyKey.self)
            guard Set(raw.allKeys.map(\.stringValue)) == Set(Keys.allCases.map(\.rawValue)) else {
                throw AuthoringFailure("NATIVE_SCENE_CAPTURE_REQUEST_INVALID")
            }
            let c = try decoder.container(keyedBy: Keys.self)
            version = try c.decode(Int.self, forKey: .version)
            requestId = try c.decode(String.self, forKey: .requestId)
            workspaceId = try c.decode(String.self, forKey: .workspaceId)
            tenantId = try c.decode(String.self, forKey: .tenantId)
            principalId = try c.decode(String.self, forKey: .principalId)
            runtimeManifestSHA256 = try c.decode(String.self, forKey: .runtimeManifestSHA256)
            expectedNativeRevision = try c.decode(UInt64.self, forKey: .expectedNativeRevision)
            expectedEditVersion = try c.decode(Int64.self, forKey: .expectedEditVersion)
            exhibitionId = try c.decode(String.self, forKey: .exhibitionId)
            serverRevision = try c.decode(Int.self, forKey: .serverRevision)
            expectedETag = try c.decode(String.self, forKey: .expectedETag)
            placementId = try c.decode(String.self, forKey: .placementId)
            viewpointId = try c.decode(String.self, forKey: .viewpointId)
            outputDirectory = try c.decode(String.self, forKey: .outputDirectory)
            let prefix = "\"studio-r\(serverRevision)-"
            guard version == 1,
                  [requestId, workspaceId, tenantId, principalId, exhibitionId, placementId, viewpointId].allSatisfy(Self.uuid),
                  nativeImportDigest(runtimeManifestSHA256), expectedNativeRevision > 0,
                  expectedNativeRevision <= UInt64(Int64.max), expectedEditVersion >= 1,
                  (1...Int(Int32.max)).contains(serverRevision), expectedETag.hasPrefix(prefix), expectedETag.hasSuffix("\""),
                  nativeImportDigest(String(expectedETag.dropFirst(prefix.count).dropLast())), Self.path(outputDirectory) else {
                throw AuthoringFailure("NATIVE_SCENE_CAPTURE_REQUEST_INVALID")
            }
        } catch {
            throw AuthoringFailure("NATIVE_SCENE_CAPTURE_REQUEST_INVALID")
        }
    }

    public func validate(document: NativeDocumentState, saved: NativeStudioResponse? = nil) throws {
        let failure = AuthoringFailure("NATIVE_SCENE_CAPTURE_STATE_CHANGED")
        guard document.format == 1, !document.dirty, document.pending == nil,
              document.workspaceId == workspaceId, document.tenantId == tenantId, document.principalId == principalId,
              document.revision == expectedNativeRevision, document.savedEditVersion == expectedEditVersion,
              let draft = document.draft, draft.id == exhibitionId, draft.document["editVersion"].integer == expectedEditVersion,
              document.etag == expectedETag else { throw failure }
        if let saved {
            guard saved.revision == serverRevision, saved.etag == expectedETag, saved.draft == draft else { throw failure }
        }
        let placements = draft.placements.filter { $0["id"].string == placementId }
        guard placements.count == 1, let room = placements[0]["roomId"].string,
              let revision = placements[0]["artworkRevisionId"].string, Self.uuid(revision),
              let asset = placements[0]["assetId"].string, Self.uuid(asset),
              draft.artworks.filter({ $0["revisionId"].string == revision }).count == 1 else { throw failure }
        do {
            let viewpoints = try BenchViewpoint.list(in: draft).filter { $0.id == viewpointId }
            guard viewpoints.count == 1, viewpoints[0].roomId == room else { throw failure }
        } catch { throw failure }
    }
}
