// SPDX-License-Identifier: Apache-2.0
import Foundation

/// One selected rail group is replaced by one registered visual. This is not a
/// general candidate patch. Light and fixture records are never rewritten.
public struct WorkspaceCeilingRailReplacement: Sendable {
    public let roomId: String, oldFurnishingId: String, oldModelSHA256: String
    public let oldPlacementIds: [String], preserveLightIds: [String]
    public let furnishingId: String, modelSHA256: String
    public let attachment: WorkspaceCeilingRailAttachment

    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == ["roomId", "oldFurnishingId", "oldModelSha256", "oldPlacementIds", "preserveLightIds", "furnishingId", "modelSha256", "attachment"],
              let room = value["roomId"].string, workspaceFurnishingID(room),
              let old = value["oldFurnishingId"].string, workspaceFurnishingID(old),
              let furnishing = value["furnishingId"].string, workspaceFurnishingID(furnishing), old != furnishing,
              let oldSHA = value["oldModelSha256"].string, nativeImportDigest(oldSHA),
              let modelSHA = value["modelSha256"].string, nativeImportDigest(modelSHA), oldSHA != modelSHA,
              let oldRows = value["oldPlacementIds"].items, (1...128).contains(oldRows.count),
              let lightRows = value["preserveLightIds"].items, (1...128).contains(lightRows.count),
              case .object(let attachmentFields) = value["attachment"],
              Set(attachmentFields.keys) == ["placementId", "surfaceId", "anchor", "yaw", "gap"] else { throw Self.invalid }
        let oldIDs = oldRows.compactMap(\.string), lightIDs = lightRows.compactMap(\.string)
        guard oldIDs.count == oldRows.count, lightIDs.count == lightRows.count,
              oldIDs.allSatisfy(workspaceFurnishingID), lightIDs.allSatisfy(workspaceFurnishingID),
              Set(oldIDs).count == oldIDs.count, Set(lightIDs).count == lightIDs.count else { throw Self.invalid }
        let attachment = try JSONDecoder().decode(WorkspaceCeilingRailAttachment.self, from: JSONEncoder().encode(value["attachment"]))
        guard !oldIDs.contains(attachment.placementId) else { throw Self.invalid }
        roomId = room; oldFurnishingId = old; oldModelSHA256 = oldSHA
        oldPlacementIds = oldIDs; preserveLightIds = lightIDs
        furnishingId = furnishing; modelSHA256 = modelSHA; self.attachment = attachment
    }

    static var invalid: AuthoringFailure { AuthoringFailure("CEILING_RAIL_REPLACEMENT_INVALID") }
}
