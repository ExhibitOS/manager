// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing StudioExperienceEditor per-floor metadata. Concrete can declare
/// its approved recordings. Stone uses the Viewer's procedural path.
public enum ConcreteFootsteps {
    public static let experienceNamespace = "org.exhibitos.viewer/experience"
    public static let resourceNamespace = "org.exhibitos.viewer/footstep-resources"
    // Commit to the reviewed owner-local material version and color map.
    private static let entranceStoneMaterial = "7e2f478b5dc0e5ddf310ac44a3e010a30985f4907b214ab208c8d91c0cf37657"
    private enum Material: String { case concrete, stone }
    public static func proposedCandidate(in draft: NativeExhibitionDraft, target: FloorSettings.Target, resourcesAvailable: Bool) throws -> DocumentValue {
        guard resourcesAvailable else { throw AuthoringFailure("FLOOR_AUDIO_RUNTIME_UNAVAILABLE") }
        return try proposedCandidate(in: draft, target: target, material: .concrete)
    }

    static func entranceMaterialCommitment(id: String, version: Int, colorSHA256: String) throws -> String {
        let fields = ["exhibitos-entrance-stone-material-v1", id, String(version), colorSHA256]
        return nativeImportSHA256(try JSONEncoder().encode(fields))
    }

    public static func proposedEntranceStoneCandidate(in draft: NativeExhibitionDraft, target: FloorSettings.Target) throws -> DocumentValue {
        try proposedEntranceStoneCandidate(in: draft, target: target, expectedMaterial: entranceStoneMaterial)
    }

    static func proposedEntranceStoneCandidate(in draft: NativeExhibitionDraft, target: FloorSettings.Target,
                                               expectedMaterial: String) throws -> DocumentValue {
        guard target.kind == .surface,
              let canonical = try FloorSettings.targets(in: draft.candidate).first(where: { $0.key == target.key }) else {
            throw AuthoringFailure("FLOOR_TARGET_INVALID")
        }
        let workspace = draft.candidate["extensions"][WorkspaceFloorMaterialBinding.namespace]
        let bindings = workspace["bindings"].items?.filter { $0["surfaceId"].string == canonical.id } ?? []
        guard workspace["version"] == .integer(1), bindings.count == 1, let binding = bindings.first,
              binding["materialVersion"] == .integer(1), let materialId = binding["materialId"].string,
              UUID(uuidString: materialId)?.uuidString.lowercased() == materialId else {
            throw AuthoringFailure("FLOOR_TARGET_INVALID")
        }
        let materials = workspace["materials"].items?.filter { $0["id"].string == materialId && $0["version"] == .integer(1) } ?? []
        guard materials.count == 1, let material = materials.first,
              material["kind"] == .string("floor"),
              let color = material["maps"]["color"]["sha256"].string, nativeImportDigest(color),
              try entranceMaterialCommitment(id: materialId, version: 1, colorSHA256: color) == expectedMaterial else {
            throw AuthoringFailure("FLOOR_TARGET_INVALID")
        }
        return try proposedCandidate(in: draft, target: canonical, material: .stone)
    }

    private static func proposedCandidate(in draft: NativeExhibitionDraft, target: FloorSettings.Target, material: Material) throws -> DocumentValue {
        guard target.kind == .surface, UUID(uuidString: target.id) != nil,
              draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == target.id && $0["type"].string == "floor" }) == true else { throw AuthoringFailure("FLOOR_TARGET_INVALID") }
        // An authored floor sound takes precedence in the shared resolver.
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
              Set(fields.keys).subtracting(["audioDefaults", "exitDoors"]) == ["version", "footsteps", "voices", "annotations", "rooms", "translations"],
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
        if material == .stone && seen.contains(target.id.lowercased()) {
            throw AuthoringFailure("FLOOR_SOUND_CONFLICT")
        }
        footsteps.removeAll { $0["surfaceId"].string?.lowercased() == target.id.lowercased() }
        guard footsteps.count < 256 else { throw invalid() }
        footsteps.append(.object(["surfaceId": .string(target.id), "material": .string(material.rawValue)]))
        experience["footsteps"] = .array(footsteps)
        candidate["extensions"][experienceNamespace] = experience
        if material == .concrete {
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
            candidate["extensions"][resourceNamespace] = .object(["version": .integer(1), "families": .array(families.map(DocumentValue.string))])
        }
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
    private static func invalid() -> AuthoringFailure { AuthoringFailure("CONCRETE_FOOTSTEPS_INVALID") }
}
