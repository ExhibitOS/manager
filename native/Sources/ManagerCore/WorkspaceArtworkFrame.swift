// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct WorkspaceArtworkFrameAttachment: Codable, Equatable, Sendable {
    public static let thinBrassSHA256 = "4b14e45ccf14ed38eea90c50553733a0b97b1671b9ec377d900e5a947545f8b4"
    static let thinBrassBytes = 7_892

    static func extensionVersion(for model: WorkspaceFurnishingModel) throws -> Int64 {
        guard model.sha256 == thinBrassSHA256 else { return 5 }
        guard model.bytes == thinBrassBytes else { throw AuthoringFailure("FRAME_ASSET_UNSUPPORTED") }
        return 4
    }

    static func extensionVersion(for furnishing: WorkspaceFurnishing) throws -> Int64 {
        if let fit = furnishing.fit, let profile = fit.profile {
            guard profile.matches(.artworkFrame) else { throw AuthoringFailure("FRAME_ASSET_UNSUPPORTED") }
            return profile.minimumExtensionVersion
        }
        return try extensionVersion(for: furnishing.details.model)
    }

    public let placementId: String
    public let artworkRevisionId: String
    public let assetId: String
    public let furnishingId: String
    public let furnishingVersion: Int
    public let modelSha256: String

    public init(placementId: String, artworkRevisionId: String, assetId: String,
                furnishingId: String, furnishingVersion: Int, modelSha256: String) {
        self.placementId = placementId
        self.artworkRevisionId = artworkRevisionId
        self.assetId = assetId
        self.furnishingId = furnishingId
        self.furnishingVersion = furnishingVersion
        self.modelSha256 = modelSha256
    }

    func checkTarget(in draft: NativeExhibitionDraft) throws {
        guard [placementId, artworkRevisionId, assetId, furnishingId].allSatisfy(workspaceFurnishingID),
              furnishingVersion == 1, nativeImportDigest(modelSha256) else {
            throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
        }
        let placements = draft.placements.filter { $0["id"].string == placementId }
        guard placements.count == 1, placements[0]["artworkRevisionId"].string == artworkRevisionId,
              placements[0]["assetId"].string == assetId else {
            throw AuthoringFailure("FRAME_TARGET_STALE")
        }
    }
}

public enum WorkspaceArtworkFrameCommand: Sendable {
    case attach(WorkspaceArtworkFrameAttachment)
    case detach(placementId: String)

    public static func hasAttachment(for placementId: String, in draft: NativeExhibitionDraft) -> Bool {
        let rows = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["frameAttachments"].items
        return rows?.contains { $0["placementId"].string == placementId } == true
    }

    // Build a closed delta. Platform owns fit, transforms and rights.
    func applying(to draft: NativeExhibitionDraft, furnishing: WorkspaceFurnishing?,
                  supportedExtensionVersion: Int64 = 4) throws -> NativeExhibitionDraft? {
        let namespace = WorkspaceFurnishingPlacement.namespace
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        guard case .object(var extensions) = candidate["extensions"] else {
            throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
        }
        var furnishings: [DocumentValue] = []
        var placements: [DocumentValue] = []
        var attachments: [DocumentValue] = []
        var openingAttachments: [DocumentValue]?
        var ceilingAttachments: [DocumentValue]?
        var extensionVersion: Int64 = 4
        if let existing = extensions[namespace] {
            guard case .object(let fields) = existing, let version = existing["version"].integer,
                  ((1...7).contains(version) || version == 9 || version == 10 || version == 11),
                  let descriptors = existing["furnishings"].items, let rows = existing["placements"].items,
                  version < 4 || existing["frameAttachments"].items != nil else {
                throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
            }
            var expected: Set<String> = ["version", "furnishings", "placements"]
            if version >= 4 { expected.insert("frameAttachments") }
            if version >= 6 { expected.insert("openingAttachments") }
            if version == 11 { expected.insert("ceilingAttachments"); try WorkspaceCeilingRail.validate(candidate: candidate) }
            openingAttachments = existing["openingAttachments"].items
            ceilingAttachments = existing["ceilingAttachments"].items
            guard Set(fields.keys) == expected, version < 6 || openingAttachments != nil,
                  version != 6 || openingAttachments?.isEmpty == false else {
                throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
            }
            guard version <= supportedExtensionVersion else {
                throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            extensionVersion = max(extensionVersion, version)
            furnishings = descriptors
            placements = rows
            attachments = existing["frameAttachments"].items ?? []
            let targets = attachments.compactMap { $0["placementId"].string }
            guard targets.count == attachments.count, Set(targets).count == targets.count else {
                throw AuthoringFailure("VIEWPORT_COMMAND_DENIED")
            }
        }

        let encoder = JSONEncoder()
        let decoder = JSONDecoder()
        var previousFurnishingId: String?
        switch self {
        case .attach(let attachment):
            try attachment.checkTarget(in: draft)
            guard let furnishing, furnishing.id == attachment.furnishingId, furnishing.version == attachment.furnishingVersion,
                  furnishing.details.kind == .artworkFrame,
                  furnishing.details.model.sha256 == attachment.modelSha256 else {
                throw AuthoringFailure("FRAME_ASSET_UNSUPPORTED")
            }
            let requiredVersion = try WorkspaceArtworkFrameAttachment.extensionVersion(for: furnishing)
            guard requiredVersion <= supportedExtensionVersion else {
                throw AuthoringFailure("FRAME_ASSET_UNSUPPORTED")
            }
            extensionVersion = max(extensionVersion, requiredVersion)
            let descriptor = try decoder.decode(DocumentValue.self, from: encoder.encode(furnishing))
            var row = try decoder.decode(DocumentValue.self, from: encoder.encode(attachment))
            // Repeating the exact six-field binding must retain its issued
            // contact policy. A deliberate new binding remains legacy until
            // the explicit saved-head reconciliation operation is applied.
            if let previous = attachments.first(where: { $0["placementId"] == .string(attachment.placementId) }),
               case .object(var oldFields) = previous, oldFields.removeValue(forKey: "imageFit") != nil,
               DocumentValue.object(oldFields) == row {
                row = previous
            }
            let existing = furnishings.filter { $0["id"].string == attachment.furnishingId }
            guard existing.isEmpty || existing == [descriptor] else {
                throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
            }
            if let index = attachments.firstIndex(where: { $0["placementId"].string == attachment.placementId }) {
                if attachments[index] == row, existing == [descriptor] { return nil }
                previousFurnishingId = attachments[index]["furnishingId"].string
                attachments[index] = row
            } else {
                attachments.append(row)
            }
            if existing.isEmpty { furnishings.append(descriptor) }
        case .detach(let placementId):
            guard workspaceFurnishingID(placementId) else { throw AuthoringFailure("VIEWPORT_COMMAND_DENIED") }
            guard let index = attachments.firstIndex(where: { $0["placementId"].string == placementId }) else {
                throw AuthoringFailure("FRAME_TARGET_STALE")
            }
            previousFurnishingId = attachments.remove(at: index)["furnishingId"].string
        }
        if let previousFurnishingId,
           !(attachments + placements).contains(where: { $0["furnishingId"].string == previousFurnishingId }) {
            furnishings.removeAll { $0["id"].string == previousFurnishingId && $0["kind"].string == "artwork-frame" }
        }
        // Profile 9 requires a measured miter descriptor. Removing its last
        // attachment returns the remaining graph to its existing legacy shape.
        if extensionVersion == 9 && !furnishings.contains(where: {
            $0["fit"]["geometry"]["profile"].string == WorkspaceFurnishingFitProfile.straightMiterRing.rawValue
        }) {
            extensionVersion = furnishings.contains(where: { $0["fit"] != .null }) ? 7
                : (openingAttachments?.isEmpty == false ? 6 : 5)
        }
        if furnishings.isEmpty, placements.isEmpty, attachments.isEmpty {
            extensions.removeValue(forKey: namespace)
        } else {
            var fields: [String: DocumentValue] = [
                "version": .integer(extensionVersion), "furnishings": .array(furnishings),
                "placements": .array(placements), "frameAttachments": .array(attachments)
            ]
            if extensionVersion >= 6 { fields["openingAttachments"] = .array(openingAttachments ?? []) }
            if extensionVersion == 11 { fields["ceilingAttachments"] = .array(ceilingAttachments ?? []) }
            extensions[namespace] = .object(fields)
        }
        candidate["extensions"] = .object(extensions)
        if candidate["extensions"][namespace]["version"] == .integer(11) { try WorkspaceCeilingRail.validate(candidate: candidate) }
        var result = draft
        try result.replaceCandidate(candidate)
        return result
    }
}
