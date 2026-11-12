// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Selection can change without changing the artwork revision. Invalidate
/// already-running render/read results before allowing the next cache fill.
@MainActor
final class CatalogImageRequestFence {
    private var generations: [String: UInt64] = [:]

    func generation(for revision: String) -> UInt64 { generations[revision, default: 0] }
    func invalidate(_ revision: String) { generations[revision, default: 0] &+= 1 }
    func isCurrent(_ generation: UInt64, for revision: String) -> Bool {
        generation == self.generation(for: revision)
    }
}
