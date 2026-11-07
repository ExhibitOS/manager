// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum WorkspacePortalFinish {
    public static let modelSHA256 = "385a3a6d5266a5dda5b3fc6e51fd14e0b990e29de66ee27a80b634a93ef2591b"

    public static func hasAttachment(openingId: String, in draft: NativeExhibitionDraft) -> Bool {
        draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["openingAttachments"].items?
            .contains { $0["openingId"].string?.lowercased() == openingId.lowercased() } == true
    }

    // Platform validates dimensions, host transforms, fit, rights and the saved revision.
    // The client constructs only the current v5/v6 visual delta.
    static func applying(to draft: NativeExhibitionDraft, openingId: String,
                         furnishing: WorkspaceFurnishing?) throws -> NativeExhibitionDraft? {
        let openingId = openingId.lowercased()
        let hosts = draft.candidate["openings"].items?.filter { $0["id"].string?.lowercased() == openingId }
        guard workspaceFurnishingID(openingId), hosts?.count == 1, hosts?.first?["type"].string == "door" else {
            throw AuthoringFailure("PORTAL_TARGET_STALE")
        }
        let namespace = WorkspaceFurnishingPlacement.namespace
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        guard case .object(var extensions) = candidate["extensions"] else {
            throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
        }
        var furnishings: [DocumentValue] = []
        var placements: [DocumentValue] = []
        var frames: [DocumentValue] = []
        var openings: [DocumentValue] = []
        if let existing = extensions[namespace] {
            guard case .object(let fields) = existing, let version = existing["version"].integer,
                  version == 5 || version == 6,
                  let listed = existing["furnishings"].items,
                  let placed = existing["placements"].items,
                  let attachedFrames = existing["frameAttachments"].items else {
                throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
            }
            let common: Set<String> = ["version", "furnishings", "placements", "frameAttachments"]
            guard Set(fields.keys) == (version == 6 ? common.union(["openingAttachments"]) : common),
                  version == 5 || existing["openingAttachments"].items?.isEmpty == false else {
                throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
            }
            furnishings = listed
            placements = placed
            frames = attachedFrames
            openings = existing["openingAttachments"].items ?? []
        }
        let targets = openings.compactMap { $0["openingId"].string?.lowercased() }
        guard targets.count == openings.count, Set(targets).count == targets.count else {
            throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
        }
        let existing = targets.firstIndex(of: openingId)
        if let furnishing {
            let model = furnishing.details.model
            guard furnishing.details.kind == .portalFinish, furnishing.version == 1, workspaceFurnishingID(furnishing.id),
                  model.sha256 == modelSHA256, model.bytes == 164_044, model.mime == "model/gltf-binary" else {
                throw AuthoringFailure("PORTAL_ASSET_UNSUPPORTED")
            }
            let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(furnishing))
            let matching = furnishings.filter { $0["id"].string == furnishing.id }
            guard matching.isEmpty || matching == [descriptor] else { throw AuthoringFailure("FURNISHING_RESPONSE_INVALID") }
            let attachment: DocumentValue = .object([
                "openingId": .string(openingId), "furnishingId": .string(furnishing.id),
                "furnishingVersion": .integer(1), "modelSha256": .string(model.sha256)
            ])
            if let existing {
                if openings[existing] == attachment && matching == [descriptor] { return nil }
                openings[existing] = attachment
            } else { openings.append(attachment) }
            if matching.isEmpty { furnishings.append(descriptor) }
        } else {
            guard let existing else { return nil }
            openings.remove(at: existing)
        }
        let used = Set(openings.compactMap { $0["furnishingId"].string })
        furnishings.removeAll { $0["kind"].string == WorkspaceFurnishingKind.portalFinish.rawValue
            && !used.contains($0["id"].string ?? "") }
        if furnishings.isEmpty && placements.isEmpty && frames.isEmpty && openings.isEmpty {
            extensions.removeValue(forKey: namespace)
        } else {
            var fields: [String: DocumentValue] = [
                "version": .integer(openings.isEmpty ? 5 : 6), "furnishings": .array(furnishings),
                "placements": .array(placements), "frameAttachments": .array(frames)
            ]
            if !openings.isEmpty { fields["openingAttachments"] = .array(openings) }
            extensions[namespace] = .object(fields)
        }
        candidate["extensions"] = .object(extensions)
        var next = draft
        try next.replaceCandidate(candidate)
        return next
    }
}
