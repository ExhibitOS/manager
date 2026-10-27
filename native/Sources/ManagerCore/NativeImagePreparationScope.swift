// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Thread-safe currentness for one owned preparation. This is a local lifetime
/// ticket, not an authorization/custody receipt; the controller still rechecks
/// workspace, account, file and saved metadata immediately before any upload.
public final class NativeImagePreparationScope: @unchecked Sendable {
    public let operationID: String
    public let generation: String
    public let originalSHA256: String
    private let lock = NSLock()
    private var active = true
    public init(operationID: String, generation: String, originalSHA256: String) {
        self.operationID = operationID; self.generation = generation; self.originalSHA256 = originalSHA256
    }
    public func isCurrent(operationID: String, generation: String, originalSHA256: String) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return active && self.operationID == operationID && self.generation == generation && self.originalSHA256 == originalSHA256
    }
    public func invalidate() { lock.lock(); active = false; lock.unlock() }
}
