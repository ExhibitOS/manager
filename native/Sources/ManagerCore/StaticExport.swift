// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum NativeRoomIndirectRecipe: String, Codable, Sendable {
    case diffuseOneBounceRGBA8V1 = "room-diffuse-one-bounce-rgba8-v1"
}

public struct NativeStaticRoomIndirectBake: Codable, Sendable {
    public enum Status: String, Codable, Sendable {
        case baked
        case liveFallback = "live-fallback"
    }

    public let status: Status
    public let descriptorSha256: String?, rooms: UInt64?, textureBytes: UInt64?
}

public struct NativeStaticExportReceipt: Codable, Sendable {
    public let publicationId: String, revisionSha256: String, indexSha256: String, manifestSha256: String
    public let files: UInt64, bytes: UInt64, output: String
    public let indirectBake: NativeStaticRoomIndirectBake?
}
public struct NativeStaticViewerEntry: Decodable, Identifiable, Sendable {
    public let viewerId: String, url: URL, authorization: String, revisionSha256: String
    public var id: String { viewerId }
}
public struct NativeStaticExportStatus: Codable, Sendable {
    struct OutputParentIdentity: Codable, Sendable {
        let device: UInt64, inode: UInt64
    }

    public let format: Int, jobId: String, workspaceId: String, tenantId: String, principalId: String
    public let publicationId: String, revisionSha256: String, output: String, state: String
    public let receipt: NativeStaticExportReceipt?
    public let errorCode: String?
    public let privateEditor: Bool?
    public var roomIndirect: NativeRoomIndirectRecipe? = nil
    var outputParentIdentity: OutputParentIdentity? = nil
    public var running: Bool { state == "running" }

    func canReuse(publicationId: String, output: String) throws -> Bool {
        let matches = self.publicationId == publicationId && self.output == output
        guard matches || !running else { throw BridgeFailure("AUTOMATION_EXPORT_CHANGED") }
        return matches
    }

    func confirmsDetailTerminal(jobId: String?, publicationId: String, output: String) -> Bool {
        self.jobId == jobId && self.publicationId == publicationId && self.output == output
            && ["completed", "cancelled", "failed"].contains(state)
    }
}
