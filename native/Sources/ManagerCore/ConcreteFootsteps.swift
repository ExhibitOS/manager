// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing StudioExperienceEditor operation: one floor's legacy concrete
/// selection plus its resource family, atomically. No PCM, gain or appearance edit.
public enum ConcreteFootsteps {
    public static let experienceNamespace = "org.exhibitos.viewer/experience"
    public static let resourceNamespace = "org.exhibitos.viewer/footstep-resources"
    public static func proposedCandidate(in draft: NativeExhibitionDraft, target: FloorSettings.Target, resourcesAvailable: Bool) throws -> DocumentValue {
        guard resourcesAvailable else { throw AuthoringFailure("FLOOR_AUDIO_RUNTIME_UNAVAILABLE") }
        guard target.kind == .surface, UUID(uuidString: target.id) != nil,
              draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == target.id && $0["type"].string == "floor" }) == true else { throw AuthoringFailure("FLOOR_TARGET_INVALID") }
        // An authored floor sound takes precedence in the shared resolver. Never claim
        // concrete while an authored material sound/none/explicit choice wins.
        guard try !FloorSettings.all(in: draft.candidate).contains(where: {
            $0.target.key == target.key && $0.value["sound"] != .null && FloorSettings.isActive($0.target, in: draft.candidate)
        }) else { throw AuthoringFailure("FLOOR_SOUND_CONFLICT") }
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        guard case .object = candidate["extensions"] else { throw invalid() }
        var experience = candidate["extensions"][experienceNamespace]
        if experience == .null {
            experience = .object(["version": .integer(1), "footsteps": .array([]), "voices": .array([]),
                                  "annotations": .array([]), "rooms": .array([]), "translations": .array([])])
        }
        guard case .object(let fields) = experience,
              Set(fields.keys).subtracting(["audioDefaults"]) == ["version", "footsteps", "voices", "annotations", "rooms", "translations"],
              experience["version"] == .integer(1), var footsteps = experience["footsteps"].items, footsteps.count <= 256,
              let voices = experience["voices"].items, voices.count <= 64,
              let annotations = experience["annotations"].items, annotations.count <= 256,
              let rooms = experience["rooms"].items, rooms.count <= 32,
              let translations = experience["translations"].items, translations.count <= 256 else { throw invalid() }
        var seen = Set<String>()
        for row in footsteps {
            guard case .object(let values) = row, Set(values.keys).isSubset(of: ["surfaceId", "material", "assetId", "bank"]),
                  let id = row["surfaceId"].string, UUID(uuidString: id) != nil, seen.insert(id.lowercased()).inserted,
                  let material = row["material"].string, ["wood", "stone", "concrete", "carpet", "metal"].contains(material) else { throw invalid() }
        }
        footsteps.removeAll { $0["surfaceId"].string?.lowercased() == target.id.lowercased() }
        guard footsteps.count < 256 else { throw invalid() }
        footsteps.append(.object(["surfaceId": .string(target.id), "material": .string("concrete")]))
        experience["footsteps"] = .array(footsteps)
        let resource = candidate["extensions"][resourceNamespace]
        var families: [String] = []
        if resource != .null {
            guard case .object(let fields) = resource, Set(fields.keys) == ["version", "families"], resource["version"] == .integer(1),
                  let values = resource["families"].items else { throw invalid() }
            families = values.compactMap(\.string)
            guard !families.isEmpty, families.count == values.count, families.count <= 5,
                  families == Array(Set(families)).sorted(), families.allSatisfy({ ["wood", "marble", "tile", "carpet", "concrete"].contains($0) }) else { throw invalid() }
        }
        if !families.contains("concrete") { families.append("concrete"); families.sort() }
        candidate["extensions"][experienceNamespace] = experience
        candidate["extensions"][resourceNamespace] = .object(["version": .integer(1), "families": .array(families.map(DocumentValue.string))])
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
    private static func invalid() -> AuthoringFailure { AuthoringFailure("CONCRETE_FOOTSTEPS_INVALID") }
}
