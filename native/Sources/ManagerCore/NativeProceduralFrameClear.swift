// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Constructs a local, closed frame-only edit; it does not persist or authorize it.
enum NativeProceduralFrameClear {
    struct Command: Sendable {
        let placementId: String

        init(_ value: DocumentValue) throws {
            guard case .object(let fields) = value, Set(fields.keys) == ["placementId"],
                  let id = fields["placementId"]?.string,
                  let uuid = UUID(uuidString: id), uuid.uuidString.lowercased() == id else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            placementId = id
        }
    }

    struct Proposal: Sendable {
        let command: Command
        let base: NativeExhibitionDraft
        let draft: NativeExhibitionDraft
        /// Complete original typed frame representation, retained for recovery.
        let originalFrame: DocumentValue

        init(command: Command, draft base: NativeExhibitionDraft) throws {
            let id = command.placementId
            let matches = base.placements.filter { $0["id"].string?.lowercased() == id }
            guard !matches.isEmpty else { throw AuthoringFailure("PROCEDURAL_FRAME_TARGET_MISSING") }
            guard matches.count == 1 else { throw AuthoringFailure("PROCEDURAL_FRAME_TARGET_DUPLICATE") }
            guard matches[0]["id"].string == id else { throw AuthoringFailure("PROCEDURAL_FRAME_TARGET_INVALID") }
            try Self.checkAttachments(base, placementId: id)
            guard var settings = try base.installationSettings(for: id), let frame = settings.frame else {
                throw AuthoringFailure("PROCEDURAL_FRAME_ABSENT")
            }
            guard frame.kind != .none else { throw AuthoringFailure("PROCEDURAL_FRAME_ALREADY_NONE") }
            let namespace = InstallationSettings.namespace
            guard var rows = base.candidate["extensions"][namespace]["placements"].items,
                  let index = rows.firstIndex(where: { $0["placementId"].string?.lowercased() == id }) else {
                throw AuthoringFailure("PROCEDURAL_FRAME_ABSENT")
            }
            let originalFrame = rows[index]["frame"]

            // Exercise the ordinary typed setter and validate its result. The
            // actual delta retains the untouched raw pedestal and sibling rows.
            settings.frame = .init(kind: .none)
            var typed = base
            try typed.setInstallation(settings, for: id)
            guard try typed.installationSettings(for: id) == settings else {
                throw AuthoringFailure("INSTALLATION_SETTINGS_INVALID")
            }
            rows[index]["frame"] = .object(["kind": .string("none")])
            var candidate = base.candidate
            candidate["extensions"][namespace]["placements"] = .array(rows)
            var result = base
            try result.replaceCandidate(candidate)
            guard try result.installationSettings(for: id) == settings else {
                throw AuthoringFailure("INSTALLATION_SETTINGS_INVALID")
            }
            self.command = command; self.base = base; self.draft = result; self.originalFrame = originalFrame
        }

        func apply(to current: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
            guard Self.exact(current.document, base.document) else { throw AuthoringFailure("PROCEDURAL_FRAME_PROPOSAL_STALE") }
            return draft
        }

        private static func checkAttachments(_ draft: NativeExhibitionDraft, placementId: String) throws {
            guard case .object(let root) = draft.candidate else { throw AuthoringFailure("PROCEDURAL_FRAME_ATTACHMENTS_INVALID") }
            guard let extensionValue = root["extensions"] else { return }
            guard case .object(let extensions) = extensionValue else { throw AuthoringFailure("PROCEDURAL_FRAME_ATTACHMENTS_INVALID") }
            guard let value = extensions[WorkspaceFurnishingPlacement.namespace] else { return }
            guard case .object(let fields) = value, let version = fields["version"]?.integer,
                  (1...7).contains(version) || version == 9 || version == 10 || version == 11 else { throw AuthoringFailure("PROCEDURAL_FRAME_ATTACHMENTS_INVALID") }
            if version == 11 { try WorkspaceCeilingRail.validate(candidate: draft.candidate) }
            guard let attachments = fields["frameAttachments"] else {
                guard version < 4 else { throw AuthoringFailure("PROCEDURAL_FRAME_ATTACHMENTS_INVALID") }
                return
            }
            guard let rows = attachments.items else { throw AuthoringFailure("PROCEDURAL_FRAME_ATTACHMENTS_INVALID") }
            var targets = Set<String>()
            for row in rows {
                guard case .object = row, let target = row["placementId"].string,
                      let uuid = UUID(uuidString: target), uuid.uuidString.lowercased() == target.lowercased(),
                      targets.insert(target.lowercased()).inserted else {
                    throw AuthoringFailure("PROCEDURAL_FRAME_ATTACHMENTS_INVALID")
                }
                if target.lowercased() == placementId { throw AuthoringFailure("PROCEDURAL_FRAME_MODEL_CONFLICT") }
            }
        }

        // DocumentValue's general equality equates integer and floating values;
        // a session proposal must instead bind the exact original representation.
        private static func exact(_ lhs: DocumentValue, _ rhs: DocumentValue) -> Bool {
            switch (lhs, rhs) {
            case (.object(let a), .object(let b)):
                return a.count == b.count && a.allSatisfy { key, value in b[key].map { exact(value, $0) } ?? false }
            case (.array(let a), .array(let b)):
                return a.count == b.count && zip(a, b).allSatisfy { pair in exact(pair.0, pair.1) }
            case (.string(let a), .string(let b)): return a == b
            case (.integer(let a), .integer(let b)): return a == b
            case (.number(let a), .number(let b)): return a.bitPattern == b.bitPattern
            case (.bool(let a), .bool(let b)): return a == b
            case (.null, .null): return true
            default: return false
            }
        }
    }
}
