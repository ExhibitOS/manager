// SPDX-License-Identifier: Apache-2.0
import Foundation

/// The authenticated evaluator describes authored settings. It does not report
/// whether a Viewer has loaded a model or installed its collision geometry.
struct FurnishingWalkingCollisionEvaluation {
    static let profile = "furnishing-walking-collision-v1"
    private static let namespace = "org.exhibitos.studio/furnishing-walking-collision"
    private static let maximumTargets = 1024

    private enum Kind: String, Hashable {
        case workspace, bench, plinth, light, frame, opening, baseboard, sign
    }

    private struct Target: Hashable {
        let kind: Kind
        let id: UUID

        init(_ row: DocumentValue) throws {
            guard let kind = row["kind"].string.flatMap(Kind.init(rawValue:)),
                  let id = row["id"].string.flatMap(UUID.init(uuidString:)) else { throw FurnishingWalkingCollisionEvaluation.invalid() }
            self.kind = kind
            self.id = id
        }
    }

    let value: DocumentValue

    init(_ response: DocumentValue, in draft: NativeExhibitionDraft) throws {
        guard case .object(let responseFields) = response, Set(responseFields.keys) == ["targets"],
              let rows = response["targets"].items, rows.count <= Self.maximumTargets else { throw Self.invalid() }
        var authored = try Self.overrides(in: draft)
        var seen = Set<Target>()
        for row in rows {
            guard case .object(let fields) = row,
                  Set(fields.keys).subtracting(["override"]) == ["kind", "id", "defaultEnabled"],
                  case .bool = row["defaultEnabled"] else { throw Self.invalid() }
            let target = try Target(row)
            guard seen.insert(target).inserted else { throw Self.invalid() }
            let expected = authored.removeValue(forKey: target).map { DocumentValue.bool($0) }
            guard fields["override"] == expected else { throw Self.invalid() }
        }
        guard authored.isEmpty else { throw Self.invalid() }
        value = response
    }

    private static func overrides(in draft: NativeExhibitionDraft) throws -> [Target: Bool] {
        let value = draft.candidate["extensions"]
        if value == .null { return [:] }
        guard case .object(let extensions) = value else { throw invalid() }
        guard let stored = extensions[namespace] else { return [:] }
        guard case .object(let fields) = stored, Set(fields.keys) == ["version", "overrides"],
              stored["version"].integer == 1, let rows = stored["overrides"].items,
              rows.count <= maximumTargets else { throw invalid() }
        var overrides: [Target: Bool] = [:]
        for row in rows {
            guard case .object(let fields) = row, Set(fields.keys) == ["kind", "id", "enabled"],
                  case .bool(let enabled) = row["enabled"] else { throw invalid() }
            let target = try Target(row)
            guard overrides.updateValue(enabled, forKey: target) == nil else { throw invalid() }
        }
        return overrides
    }

    private static func invalid() -> AuthoringFailure { AuthoringFailure("FURNISHING_WALKING_COLLISION_INVALID") }
}
