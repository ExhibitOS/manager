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
