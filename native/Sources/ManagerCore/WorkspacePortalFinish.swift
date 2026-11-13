// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum WorkspacePortalFinish {
    public static let modelSHA256 = "385a3a6d5266a5dda5b3fc6e51fd14e0b990e29de66ee27a80b634a93ef2591b"

    public static func hasAttachment(openingId: String, in draft: NativeExhibitionDraft) -> Bool {
        draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["openingAttachments"].items?
            .contains { $0["openingId"].string?.lowercased() == openingId.lowercased() } == true
    }

    // Platform validates dimensions, host transforms, fit, rights and the saved revision.
    // The client carries the server's registered fit descriptor into the visual delta.
    static func applying(to draft: NativeExhibitionDraft, openingId: String,
                         furnishing: WorkspaceFurnishing?, admitsModularFit: Bool = false,
                         admitsRectangularFit: Bool = false, admitsMiterFrames: Bool = false, admitsFrameImageFit: Bool = false) throws -> NativeExhibitionDraft? {
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
                  version == 3 || version == 5 || version == 6 || (version == 7 && (admitsModularFit || admitsRectangularFit))
                    || (version == 9 && admitsMiterFrames) || (version == 10 && admitsFrameImageFit),
                  let listed = existing["furnishings"].items,
                  let placed = existing["placements"].items else {
                throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
            }
            let common: Set<String> = ["version", "furnishings", "placements", "frameAttachments"]
            let expected: Set<String> = version == 3 ? ["version", "furnishings", "placements"]
                : version >= 6 ? common.union(["openingAttachments"]) : common
            guard Set(fields.keys) == expected,
                  version != 3 || (!listed.isEmpty && listed.count <= 64 && !placed.isEmpty && placed.count <= 128
                    && listed.allSatisfy({ item in
                        guard let kind = item["kind"].string.flatMap(WorkspaceFurnishingKind.init(rawValue:)) else { return false }
                        return kind.minimumExtensionVersion <= 3
                    })),
                  version == 3 || existing["frameAttachments"].items != nil,
                  version < 6 || existing["openingAttachments"].items != nil,
                  version != 6 || existing["openingAttachments"].items?.isEmpty == false else {
                throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
            }
            furnishings = listed
            placements = placed
            frames = existing["frameAttachments"].items ?? []
            openings = existing["openingAttachments"].items ?? []
        }
        let targets = openings.compactMap { $0["openingId"].string?.lowercased() }
        guard targets.count == openings.count, Set(targets).count == targets.count else {
            throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
        }
        let existing = targets.firstIndex(of: openingId)
        if let furnishing {
            let model = furnishing.details.model
            let qualifiedModel = furnishing.fit.map {
                ($0.profile == .modularPortal && admitsModularFit)
                    || ($0.profile == .rectangularPortal && admitsRectangularFit)
            }
                ?? (model.sha256 == modelSHA256 && model.bytes == 164_044)
            guard furnishing.details.kind == .portalFinish, furnishing.version == 1, workspaceFurnishingID(furnishing.id),
                  qualifiedModel, model.mime == "model/gltf-binary" else {
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
            let hasMiter = furnishings.contains { $0["fit"]["geometry"]["profile"].string == WorkspaceFurnishingFitProfile.straightMiterRing.rawValue }
            let legacyVersion: Int64 = hasMiter ? 9 : furnishings.contains { $0["fit"] != .null } ? 7 : openings.isEmpty ? 5 : 6
            let version = max(candidate["extensions"][namespace]["version"].integer == 10 ? 10 : 0, legacyVersion)
            var fields: [String: DocumentValue] = [
                "version": .integer(version), "furnishings": .array(furnishings),
                "placements": .array(placements), "frameAttachments": .array(frames)
            ]
            if version >= 6 { fields["openingAttachments"] = .array(openings) }
            extensions[namespace] = .object(fields)
        }
        candidate["extensions"] = .object(extensions)
        var next = draft
        try next.replaceCandidate(candidate)
        return next
    }
}
