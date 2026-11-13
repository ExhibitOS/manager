// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Constructs ordinary private draft edits; the caller must validate each
/// candidate with the authoritative server before adopting or saving it.
enum NativePrivateHostArchitecture {
    private static func invalid() -> BridgeFailure { BridgeFailure("AUTOMATION_COMMAND_INVALID") }

    private static func id(_ value: DocumentValue) throws -> String {
        guard let raw = value.string, let uuid = UUID(uuidString: raw) else { throw invalid() }
        return uuid.uuidString.lowercased()
    }
    private static func number(_ value: DocumentValue, positive: Bool = false) throws -> Double {
        guard let result = value.double, result.isFinite, abs(result) <= 1_000_000,
              !positive || result > 0 else { throw invalid() }
        return result
    }
    private static func vector(_ value: DocumentValue, count: Int) throws -> [Double] {
        guard let values = value.items, values.count == count else { throw invalid() }
        return try values.map { try number($0) }
    }
    private static func fields(_ value: DocumentValue, _ expected: Set<String>) throws {
        guard case .object(let fields) = value, Set(fields.keys) == expected else { throw invalid() }
    }
    private static func unique(_ id: String, candidate: DocumentValue) -> Bool {
        guard case .object(let fields) = candidate else { return false }
        guard candidate["id"].string?.lowercased() != id, candidate["revisionId"].string?.lowercased() != id else { return false }
        return !fields.values.contains { value in
            value.items?.contains { $0["id"].string?.lowercased() == id } == true
        }
    }
    private static func index(_ id: String, in rows: [DocumentValue]) throws -> Int {
        let matches = rows.indices.filter { rows[$0]["id"].string?.lowercased() == id }
        guard matches.count == 1 else { throw invalid() }
        return matches[0]
    }
    private static func markTemplate(_ candidate: inout DocumentValue) throws {
        let key = "org.exhibitos.studio/template"
        if candidate["extensions"][key] != .null {
            guard case .object = candidate["extensions"][key],
                  case .bool = candidate["extensions"][key]["modified"] else { throw invalid() }
            candidate["extensions"][key]["modified"] = .bool(true)
        }
    }
    static func applying(_ command: DocumentValue, to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
        guard let type = command["type"].string else { throw invalid() }
        var candidate = draft.candidate
        switch type {
        case "room-add", "room-update":
            try fields(command, ["type", "id", "name", "width", "height", "depth", "position"])
            let roomID = try id(command["id"]), position = try vector(command["position"], count: 3)
            guard let name = command["name"].string, !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                  name.count <= 512, var rooms = candidate["rooms"].items else { throw invalid() }
            let width = try number(command["width"], positive: true)
            let height = try number(command["height"], positive: true)
            let depth = try number(command["depth"], positive: true)
            if type == "room-add" {
                guard unique(roomID, candidate: candidate) else { throw invalid() }
                var transform = NativeExhibitionDraft.identity; transform["position"] = .vector(position)
                rooms.append(.object(["id": .string(roomID), "name": .string(name),
                    "dimensions": .object(["width": .number(width), "height": .number(height), "depth": .number(depth)]),
                    "transform": transform]))
            } else {
                let i = try index(roomID, in: rooms)
                guard case .object = rooms[i]["dimensions"], case .object = rooms[i]["transform"] else { throw invalid() }
                rooms[i]["name"] = .string(name)
                rooms[i]["dimensions"]["width"] = .number(width)
                rooms[i]["dimensions"]["height"] = .number(height)
                rooms[i]["dimensions"]["depth"] = .number(depth)
                rooms[i]["transform"]["position"] = .vector(position)
            }
            candidate["rooms"] = .array(rooms)
            if type == "room-update" { candidate = try RoomBaseLightIntent.reflow(candidate, roomId: rooms[try index(roomID, in: rooms)]["id"].string!) }
        case "white-cube":
            try fields(command, ["type", "roomId", "surfaceIds"])
            let roomID = try id(command["roomId"])
            guard let rooms = candidate["rooms"].items, var surfaces = candidate["surfaces"].items,
                  let supplied = command["surfaceIds"].items, supplied.count == 6 else { throw invalid() }
            let room = rooms[try index(roomID, in: rooms)]
            guard !surfaces.contains(where: { $0["roomId"].string?.lowercased() == roomID }) else { throw invalid() }
            let ids = try supplied.map { try id($0) }
            guard Set(ids).count == 6, ids.allSatisfy({ unique($0, candidate: candidate) }) else { throw invalid() }
            let w = try number(room["dimensions"]["width"], positive: true)
            let h = try number(room["dimensions"]["height"], positive: true)
            let d = try number(room["dimensions"]["depth"], positive: true)
            let s = sqrt(0.5)
            let shapes: [(String, Double, Double, [Double], [Double])] = [
                ("wall", w, h, [0,h/2,-d/2], [0,0,0,1]),
                ("wall", w, h, [0,h/2,d/2], [0,1,0,0]),
                ("wall", d, h, [-w/2,h/2,0], [0,s,0,s]),
                ("wall", d, h, [w/2,h/2,0], [0,-s,0,s]),
                ("floor", w, d, [0,0,0], [-s,0,0,s]),
                ("ceiling", w, d, [0,h,0], [s,0,0,s])
            ]
            for (i, shape) in shapes.enumerated() {
                surfaces.append(.object(["id": .string(ids[i]), "roomId": room["id"], "type": .string(shape.0),
                    "dimensions": .object(["width": .number(shape.1), "height": .number(shape.2)]),
                    "transform": .object(["position": .vector(shape.3), "rotation": .vector(shape.4), "scale": .vector([1,1,1])])]))
            }
            candidate["surfaces"] = .array(surfaces)
        case "surface-update":
            try fields(command, ["type", "id", "width", "height", "position"])
            let surfaceID = try id(command["id"]), position = try vector(command["position"], count: 3)
            let width = try number(command["width"], positive: true)
            let height = try number(command["height"], positive: true)
            guard var surfaces = candidate["surfaces"].items else { throw invalid() }
            let i = try index(surfaceID, in: surfaces)
            guard let originalID = surfaces[i]["id"].string else { throw invalid() }
            let edit = ArchitectureCommand.surface(id: originalID, width: width, height: height, position: position)
            var replacement = try edit.encodedValue(in: draft, selected: edit.selection)["surface"]
            replacement["id"] = .string(originalID)
            surfaces[i] = replacement
            candidate["surfaces"] = .array(surfaces)
        case "opening-add":
            try fields(command, ["type", "id", "surfaceId", "width", "height", "offset"])
            let openingID = try id(command["id"]), surfaceID = try id(command["surfaceId"])
            guard unique(openingID, candidate: candidate), let surfaces = candidate["surfaces"].items,
                  var openings = candidate["openings"].items else { throw invalid() }
            let surface = surfaces[try index(surfaceID, in: surfaces)]
            guard surface["type"].string == "wall", let rooms = candidate["rooms"].items,
                  let hostRoom = surface["roomId"].string, UUID(uuidString: hostRoom) != nil else { throw invalid() }
            _ = try index(hostRoom.lowercased(), in: rooms)
            let width = try number(command["width"], positive: true), height = try number(command["height"], positive: true)
            let offset = try vector(command["offset"], count: 2)
            let wallHeight = try number(surface["dimensions"]["height"], positive: true)
            guard abs(offset[1] - (height - wallHeight) / 2) <= 0.000001 else { throw invalid() }
            openings.append(.object(["id": .string(openingID), "surfaceId": surface["id"], "type": .string("door"),
                "dimensions": .object(["width": .number(width), "height": .number(height)]),
                "offset": .vector(offset), "exterior": .bool(true)]))
            candidate["openings"] = .array(openings)
        case "connect-openings":
            try fields(command, ["type", "firstId", "secondId"])
            let first = try id(command["firstId"]), second = try id(command["secondId"])
            guard first != second, var openings = candidate["openings"].items,
                  let surfaces = candidate["surfaces"].items, let rooms = candidate["rooms"].items else { throw invalid() }
            let a = try index(first, in: openings), b = try index(second, in: openings)
            func host(_ opening: DocumentValue) throws -> String {
                guard opening["type"].string == "door", opening["connectsToOpeningId"] == .null,
                      opening["exterior"] == .bool(true) || opening["exterior"] == .null else { throw invalid() }
                let wall = surfaces[try index(id(opening["surfaceId"]), in: surfaces)]
                guard wall["type"].string == "wall" else { throw invalid() }
                let room = try id(wall["roomId"]); _ = try index(room, in: rooms); return room
            }
            guard try host(openings[a]) != host(openings[b]) else { throw invalid() }
            for key in ["width", "height"] {
                let left = try number(openings[a]["dimensions"][key], positive: true)
                let right = try number(openings[b]["dimensions"][key], positive: true)
                guard abs(left - right) <= 0.000001 else { throw invalid() }
            }
            for (i, other) in [(a, openings[b]["id"]), (b, openings[a]["id"])] {
                guard case .object(var fields) = openings[i] else { throw invalid() }
                fields.removeValue(forKey: "exterior"); fields["connectsToOpeningId"] = other
                openings[i] = .object(fields)
            }
            candidate["openings"] = .array(openings)
        default: throw invalid()
        }
        try markTemplate(&candidate)
        var result = draft
        try result.replaceCandidate(candidate)
        return result
    }
}
