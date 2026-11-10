// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct FurnishingWalkingCollisionTarget: Hashable, Sendable {
    public enum Kind: String, Hashable, Sendable {
        case workspace, bench, plinth, light, frame, opening, baseboard, sign
    }

    public let kind: Kind
    public let id: UUID

    init(_ row: DocumentValue) throws {
        guard let kind = row["kind"].string.flatMap(Kind.init(rawValue:)),
              let id = row["id"].string.flatMap(UUID.init(uuidString:)) else {
            throw FurnishingWalkingCollisionEvaluation.invalid()
        }
        self.kind = kind
        self.id = id
    }

    var value: DocumentValue { .object(["kind": .string(kind.rawValue), "id": .string(id.uuidString.lowercased())]) }
}

public enum FurnishingWalkingCollisionSetting: Hashable, Sendable {
    case inherit, on, off

    init(_ enabled: Bool?) {
        switch enabled {
        case true?: self = .on
        case false?: self = .off
        case nil: self = .inherit
        }
    }

    var enabled: Bool? {
        switch self {
        case .inherit: nil
        case .on: true
        case .off: false
        }
    }

    var value: DocumentValue { enabled.map(DocumentValue.bool) ?? .null }
}

public struct FurnishingWalkingCollisionState: Identifiable, Sendable {
    public let target: FurnishingWalkingCollisionTarget
    public let defaultEnabled: Bool
    public let setting: FurnishingWalkingCollisionSetting
    public var id: FurnishingWalkingCollisionTarget { target }
}

/// Authored settings. Actual collision geometry depends on the Viewer scene.
struct FurnishingWalkingCollisionEvaluation {
    static let profile = "furnishing-walking-collision-v1"
    fileprivate static let namespace = "org.exhibitos.studio/furnishing-walking-collision"
    fileprivate static let maximumTargets = 1024

    let value: DocumentValue
    let targets: [FurnishingWalkingCollisionState]

    init(_ response: DocumentValue, in draft: NativeExhibitionDraft) throws {
        guard case .object(let responseFields) = response, Set(responseFields.keys) == ["targets"],
              let rows = response["targets"].items, rows.count <= Self.maximumTargets else { throw Self.invalid() }
        let overrides = try Self.authored(in: draft.candidate)
        var authored = Dictionary(uniqueKeysWithValues: overrides.map { ($0.target, $0.enabled) })
        var seen = Set<FurnishingWalkingCollisionTarget>()
        targets = try rows.map { row in
            guard case .object(let fields) = row,
                  Set(fields.keys).subtracting(["override"]) == ["kind", "id", "defaultEnabled"],
                  case .bool(let defaultEnabled) = row["defaultEnabled"] else { throw Self.invalid() }
            let target = try FurnishingWalkingCollisionTarget(row)
            guard seen.insert(target).inserted else { throw Self.invalid() }
            let override = authored.removeValue(forKey: target)
            guard fields["override"] == override.map(DocumentValue.bool) else { throw Self.invalid() }
            return .init(target: target, defaultEnabled: defaultEnabled, setting: .init(override))
        }
        guard authored.isEmpty else { throw Self.invalid() }
        value = response
    }

    fileprivate struct Override {
        let target: FurnishingWalkingCollisionTarget
        let enabled: Bool
        let value: DocumentValue

        init(_ row: DocumentValue) throws {
            guard case .object(let fields) = row, Set(fields.keys) == ["kind", "id", "enabled"],
                  case .bool(let enabled) = row["enabled"] else { throw FurnishingWalkingCollisionEvaluation.invalid() }
            target = try .init(row)
            self.enabled = enabled
            value = row
        }
    }

    fileprivate static func extensions(in candidate: DocumentValue) throws -> [String: DocumentValue] {
        let value = candidate["extensions"]
        if value == .null { return [:] }
        guard case .object(let extensions) = value else { throw invalid() }
        return extensions
    }

    fileprivate static func authored(in candidate: DocumentValue) throws -> [Override] {
        guard let stored = try extensions(in: candidate)[namespace] else { return [] }
        guard case .object(let fields) = stored, Set(fields.keys) == ["version", "overrides"],
              stored["version"].integer == 1, let rows = stored["overrides"].items,
              rows.count <= maximumTargets else { throw invalid() }
        var seen = Set<FurnishingWalkingCollisionTarget>()
        return try rows.map { row in
            let override = try Override(row)
            guard seen.insert(override.target).inserted else { throw invalid() }
            return override
        }
    }

    static func invalid() -> AuthoringFailure { AuthoringFailure("FURNISHING_WALKING_COLLISION_INVALID") }
}

struct FurnishingWalkingCollisionProposal: Encodable {
    let draft: NativeExhibitionDraft
    let target: FurnishingWalkingCollisionTarget
    let setting: FurnishingWalkingCollisionSetting

    private enum CodingKeys: CodingKey { case draft, target, enabled }

    func encode(to encoder: Encoder) throws {
        var values = encoder.container(keyedBy: CodingKeys.self)
        try values.encode(draft, forKey: .draft)
        try values.encode(target.value, forKey: .target)
        try values.encode(setting.value, forKey: .enabled)
    }

    static func command(_ input: DocumentValue) throws -> (FurnishingWalkingCollisionTarget, FurnishingWalkingCollisionSetting) {
        guard case .object(let fields) = input, Set(fields.keys) == ["target", "enabled"],
              case .object(let targetFields) = input["target"], Set(targetFields.keys) == ["kind", "id"],
              let target = try? FurnishingWalkingCollisionTarget(input["target"]) else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        switch input["enabled"] {
        case .null: return (target, .inherit)
        case .bool(let enabled): return (target, .init(enabled))
        default: throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
    }

    func applying(_ response: DocumentValue) throws -> NativeExhibitionDraft? {
        typealias Collision = FurnishingWalkingCollisionEvaluation
        guard case .object(let fields) = response, Set(fields.keys) == ["changed", "candidate"],
              case .bool(let changed) = response["changed"] else { throw Collision.invalid() }
        let overrides = try Collision.authored(in: draft.candidate)
        let current = overrides.first(where: { $0.target == target })?.enabled
        guard changed == (current != setting.enabled) else { throw Collision.invalid() }
        let proposed = response["candidate"]
        if !changed {
            guard proposed == draft.candidate else { throw Collision.invalid() }
            return nil
        }

        var rows = overrides.filter { $0.target != target }.map(\.value)
        if let choice = setting.enabled {
            guard let added = try Collision.authored(in: proposed).last,
                  added.target == target, added.enabled == choice else { throw Collision.invalid() }
            rows.append(added.value)
        }
        guard rows.count <= Collision.maximumTargets else { throw Collision.invalid() }
        var extensions = try Collision.extensions(in: draft.candidate)
        if rows.isEmpty { extensions.removeValue(forKey: Collision.namespace) }
        else { extensions[Collision.namespace] = .object(["version": .integer(1), "overrides": .array(rows)]) }
        var expected = draft.candidate
        expected["extensions"] = .object(extensions)
        guard proposed == expected else { throw Collision.invalid() }
        var next = draft
        try next.replaceCandidate(expected)
        return next
    }
}
