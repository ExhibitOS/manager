// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeStaticExportReceipt: Codable, Sendable {
    public let publicationId: String, revisionSha256: String, indexSha256: String, manifestSha256: String
    public let files: UInt64, bytes: UInt64, output: String
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
    let outputParentIdentity: OutputParentIdentity?
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
