// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Parse only the existing native inspector commands; shared viewport producers
/// remain responsible for geometry and source validation before checked apply.
enum NativeAutomationPlaqueWall {
    static func plaque(_ value: DocumentValue) throws -> PlaqueCommand {
        guard case .object(let fields) = value, let type = value["type"].string,
              ["add-plaque", "set-plaque"].contains(type),
              Set(fields.keys).subtracting(["roomIntroductionText"]) == ["type", "plaque"] else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        let plaque = try PlaqueSettings(value: value["plaque"])
        let introduction: String?
        if let raw = fields["roomIntroductionText"] {
            guard let text = raw.string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            introduction = text
        } else { introduction = nil }
        return type == "add-plaque" ? .add(plaque, roomIntroductionText: introduction) : .set(plaque, roomIntroductionText: introduction)
    }

    static func wall(_ value: DocumentValue) throws -> WallFinishCommand {
        guard case .object(let fields) = value, Set(fields.keys) == ["type", "wall"],
              value["type"] == .string("set-wall-finish"), case .object(let wall) = value["wall"],
              Set(wall.keys) == ["surfaceId", "depth", "preset", "trimWidth", "baseboardHeight", "ceilingBand"],
              let id = value["wall"]["surfaceId"].string, let depth = value["wall"]["depth"].double,
              let preset = value["wall"]["preset"].string, let trim = value["wall"]["trimWidth"].double,
              let base = value["wall"]["baseboardHeight"].double, case .bool(let band) = value["wall"]["ceilingBand"] else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        return try WallFinishCommand(surfaceId: id, settings: WallFinishSettings(surfaceId: id, depth: depth,
            preset: preset, trimWidth: trim, baseboardHeight: base, ceilingBand: band))
    }
}
