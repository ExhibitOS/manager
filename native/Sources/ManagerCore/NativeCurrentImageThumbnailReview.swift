// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Constructed only after typed current-owner receipt, source and PNG checks.
/// Inspection is separate from the subsequent explicit selection command.
public struct NativeCurrentImageThumbnailReview: Sendable {
    public let job: NativeCurrentImageThumbnailJob
    public let png: Data
    let generation: UInt64
    let issuer: UUID
    let workspaceId: String
    let principalId: String
}

/// Existing-asset selection response; image identity is verified by an authoritative read.
public struct NativeCurrentImageThumbnailSelectionReceipt: Codable, Sendable {
    public let id: UUID
    public let selectionVersion: Int
    public let selected: Bool
}
