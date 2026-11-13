// SPDX-License-Identifier: Apache-2.0
import Foundation

// Opaque, session-owned proposals use the ordinary portal API, never caller-supplied candidates.
enum NativeAutomationPortal {
    struct Command: Sendable {
        let openingId: String
        let furnishingId: String
        let modelSHA256: String

        init(_ value: DocumentValue) throws {
            guard case .object(let fields) = value,
                  Set(fields.keys) == ["openingId", "furnishingId", "furnishingVersion", "modelSha256"],
                  let opening = fields["openingId"]?.string, workspaceFurnishingID(opening),
                  let furnishing = fields["furnishingId"]?.string, workspaceFurnishingID(furnishing),
                  fields["furnishingVersion"] == .integer(1),
                  let hash = fields["modelSha256"]?.string, hash.utf8.count == 64,
                  hash.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            openingId = opening; furnishingId = furnishing; modelSHA256 = hash
        }
    }

    struct Proposal: Sendable {
        let command: Command
        let base: DocumentValue
        let draft: NativeExhibitionDraft?

        func check(current: DocumentValue, refreshed: NativeExhibitionDraft?) throws {
            guard current == base else { throw BridgeFailure("AUTOMATION_PROPOSAL_EXPIRED") }
            var expected = draft?.document, observed = refreshed?.document
            // The ordinary typed delta regenerates only this envelope timestamp on each preparation.
            // Keep candidate, draft/exhibition identity, edit version and every other field exact.
            expected?["updatedAt"] = .null; observed?["updatedAt"] = .null
            guard observed == expected else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        }
    }
}
