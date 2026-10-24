// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativeStaticExportReceipt: Codable, Sendable {
    public let publicationId: String, revisionSha256: String, indexSha256: String, manifestSha256: String
    public let files: UInt64, bytes: UInt64, output: String
}
public struct NativeStaticExportStatus: Codable, Sendable {
    public let format: Int, jobId: String, workspaceId: String, tenantId: String, principalId: String
    public let publicationId: String, revisionSha256: String, output: String, state: String
    public let receipt: NativeStaticExportReceipt?
    public let errorCode: String?
    public var running: Bool { state == "running" }
}
