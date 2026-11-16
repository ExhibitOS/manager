// SPDX-License-Identifier: Apache-2.0
import Foundation
import simd

/// Absence is historical UNKNOWN, never an inferred automatic setting.
public enum RoomBaseLightPlacement: String, Codable, Sendable {
    case auto, manual
    public static let profile = "room-base-light-placement-v1"
}

public enum RoomCeilingFill {
    public static let profile = "room-ceiling-fill-v1"
    public static let initialBrightness = 0.5

    static func dimensions(in candidate: DocumentValue, room: DocumentValue) throws -> DocumentValue {
        let invalid = AuthoringFailure("ROOM_CEILING_FILL_GEOMETRY")
        let tolerance = 1e-5
        func rigid(_ pose: DocumentValue) throws -> (position: [Double], rotation: [Double], scale: [Double]) {
            let position = pose["position"].items?.compactMap(\.double) ?? []
            let rotation = pose["rotation"].items?.compactMap(\.double) ?? []
            let scale = pose["scale"].items?.compactMap(\.double) ?? []
            guard position.count == 3, position.allSatisfy(\.isFinite), rotation.count == 4,
                  rotation.allSatisfy(\.isFinite), abs(rotation.reduce(0) { $0 + $1 * $1 } - 1) <= 1e-6,
                  scale.count == 3, scale.allSatisfy({ abs($0 - 1) <= tolerance }) else { throw invalid }
            return (position, rotation, scale)
        }
        let roomPose = try rigid(room["transform"])
        guard abs(roomPose.rotation[0]) + abs(roomPose.rotation[2]) <= tolerance,
              let id = room["id"].string?.lowercased(),
              let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double,
              let depth = room["dimensions"]["depth"].double, let surfaces = candidate["surfaces"].items else { throw invalid }
        let ceilings = surfaces.filter { $0["type"] == .string("ceiling") && $0["roomId"].string?.lowercased() == id }
        guard ceilings.count <= 1 else { throw invalid }
        if let ceiling = ceilings.first {
            let pose = try rigid(ceiling["transform"])
            guard let w = ceiling["dimensions"]["width"].double, let h = ceiling["dimensions"]["height"].double,
                  w.isFinite, w > 0, h.isFinite, h > 0 else { throw invalid }
            let q = simd_normalize(simd_quatd(ix: pose.rotation[0], iy: pose.rotation[1], iz: pose.rotation[2], r: pose.rotation[3]))
            let x = q.act(SIMD3<Double>(1, 0, 0)), y = q.act(SIMD3<Double>(0, 1, 0))
            let normal = q.act(SIMD3<Double>(0, 0, 1))
            func aligned(_ axis: SIMD3<Double>) -> Bool { max(abs(axis.x), abs(axis.y), abs(axis.z)) >= 1 - tolerance }
            guard aligned(x), aligned(y), abs(normal.y + 1) <= tolerance else { throw invalid }
            let half = simd_abs(x) * (w * pose.scale[0] / 2) + simd_abs(y) * (h * pose.scale[1] / 2)
            let center = SIMD3<Double>(pose.position[0], pose.position[1], pose.position[2])
            let expected = SIMD3<Double>(width / 2, 0, depth / 2)
            let level = SIMD3<Double>(0, height, 0)
            let low = (center - half) - (level - expected), high = (center + half) - (level + expected)
            guard [low.x, low.y, low.z, high.x, high.y, high.z].allSatisfy({ abs($0) <= tolerance }) else { throw invalid }
        }
        return room["dimensions"]
    }

    static func brightness(in value: DocumentValue) throws -> Double {
        guard case .object(let fields) = value, Set(fields.keys) == ["version", "brightness"],
              value["version"] == .integer(1), let brightness = value["brightness"].double,
              brightness.isFinite, (0...1).contains(brightness) else { throw AuthoringFailure("LIGHT_DETAILS_CEILING_FILL") }
        return brightness
    }

    static func emitter(dimensions: DocumentValue, brightness: Double) throws -> DocumentValue {
        let invalid = AuthoringFailure("ROOM_CEILING_FILL_GEOMETRY")
        guard let roomWidth = dimensions["width"].double, let roomDepth = dimensions["depth"].double,
              let roomHeight = dimensions["height"].double, roomHeight > 0.02,
              [roomWidth, roomDepth, roomHeight].allSatisfy({ $0.isFinite && $0 > 0 && $0 <= 10_000 }) else { throw invalid }
        guard brightness.isFinite, (0...1).contains(brightness) else { throw AuthoringFailure("ROOM_CEILING_FILL_BRIGHTNESS") }
        let width = roomWidth * 0.8, depth = roomDepth * 0.8, height = roomHeight - 0.02
        let intensity = brightness == 0 ? 0 : brightness / meanFloorLux(width: roomWidth, depth: roomDepth,
            panelWidth: width, panelDepth: depth, height: height)
        guard intensity.isFinite, brightness == 0 || intensity > 0 else { throw invalid }
        let diagonal = 0.7071067811865476
        return .object(["dimensions": .object(["width": .number(width), "height": .number(depth)]),
            "intensity": .number(intensity), "transform": .object(["position": .vector([0, height, 0]),
                "rotation": .vector([-diagonal, 0, 0, diagonal]), "scale": .vector([1, 1, 1])])])
    }

    private static func meanFloorLux(width: Double, depth: Double, panelWidth: Double, panelDepth: Double, height: Double) -> Double {
        let samples = 12
        let heightSquared = height * height
        var sum = 0.0
        func midpoint(_ index: Int, _ span: Double) -> Double {
            ((Double(index) + 0.5) / Double(samples) - 0.5) * span
        }
        // Keep the shared emitter order and arithmetic for exact saved-head replay.
        for floorX in 0..<samples {
            for floorZ in 0..<samples {
                for panelX in 0..<samples {
                    let dx = midpoint(floorX, width) - midpoint(panelX, panelWidth)
                    for panelZ in 0..<samples {
                        let dz = midpoint(floorZ, depth) - midpoint(panelZ, panelDepth)
                        let distanceSquared = heightSquared + dx * dx + dz * dz
                        sum += heightSquared / (Double.pi * distanceSquared * distanceSquared)
                    }
                }
            }
        }
        return sum / Double(samples * samples * samples * samples)
    }
}

/// Correspondence for the shared room-local recipe. This does not grant
/// authoring authority; callers still need the current session and saved head.
public enum RoomBaseLightIntent {
    private static let details = NativeLightFixture.namespace
    private static let attachments = "org.exhibitos.studio/light-attachments"
    private static var invalid: AuthoringFailure { AuthoringFailure("LIGHT_FIELDS_INVALID") }
    private static func same(_ a: String?, _ b: String) -> Bool { a?.lowercased() == b.lowercased() }
    /// Inspect presence only to decide whether the new strict intent parser is
    /// needed. Opaque older/future envelopes keep the existing basic-light route.
    static func hasPlacementMarker(in candidate: DocumentValue) -> Bool {
        let value = candidate["extensions"][details]
        guard value["version"] == .integer(1), let rows = value["lights"].items else { return false }
        return rows.contains { row in
            guard case .object(let fields) = row else { return false }
            return fields["basePlacement"] != nil
        }
    }
    static func checkMarker(_ settings: DocumentValue) throws {
        guard case .object(let fields) = settings else { throw invalid }
        if let value = fields["basePlacement"] {
            guard settings["purpose"] == .string("room-base"), let raw = value.string,
                  RoomBaseLightPlacement(rawValue: raw) != nil else { throw invalid }
        }
    }
    public static func placement(in draft: NativeExhibitionDraft, lightId: String) throws -> RoomBaseLightPlacement? {
        let settings = try NativeLightFixture.settings(in: draft, lightId: lightId)
        try checkMarker(settings)
        return settings["basePlacement"].string.flatMap(RoomBaseLightPlacement.init(rawValue:))
    }
    public static func range(width: Double, height: Double, depth: Double) throws -> Double {
        guard [width, height, depth].allSatisfy({ $0.isFinite && $0 > 0 }) else { throw invalid }
        let values = [width / 2, depth / 2, height * 0.9]
        let maximum = values.max()!
        // Match the pinned Node24 three-argument Math.hypot evaluation: scale
        // before squaring, retaining the compensation at each addition.
        var sum = 0.0, compensation = 0.0
        for value in values {
            let normalized = value / maximum
            let summand = normalized * normalized - compensation
            let preliminary = sum + summand
            compensation = (preliminary - sum) - summand
            sum = preliminary
        }
        return min(50, max(0.1, sum.squareRoot() * maximum))
    }
    private static func baseLight(in candidate: DocumentValue, roomId: String) throws -> (Int, DocumentValue, Int)? {
        let bindings = try NativeLightFixture.bindings(in: candidate)
        guard let lights = candidate["lights"].items else { throw invalid }
        let matches = lights.enumerated().compactMap { index, light -> (Int, DocumentValue, Int)? in
            guard same(light["roomId"].string, roomId), let id = light["id"].string,
                  let binding = bindings.firstIndex(where: { same($0["lightId"].string, id) && $0["purpose"] == .string("room-base") }) else { return nil }
            return (index, light, binding)
        }
        guard matches.count <= 1 else { throw invalid }; return matches.first
    }
    static func reflow(_ candidate: DocumentValue, roomId: String) throws -> DocumentValue {
        guard let (lightIndex, light, bindingIndex) = try baseLight(in: candidate, roomId: roomId) else { return candidate }
        var bindings = try NativeLightFixture.bindings(in: candidate)
        guard bindings[bindingIndex]["basePlacement"] == .string("auto") else { return candidate }
        guard let room = candidate["rooms"].items?.first(where: { same($0["id"].string, roomId) }),
              var lights = candidate["lights"].items else { throw invalid }
        if bindings[bindingIndex]["ceilingFill"] != .null {
            guard light["type"] == .string("area"), light["unit"] == .string("lumen") else { throw invalid }
            let brightness = try RoomCeilingFill.brightness(in: bindings[bindingIndex]["ceilingFill"])
            let emitter = try RoomCeilingFill.emitter(dimensions: RoomCeilingFill.dimensions(in: candidate, room: room), brightness: brightness)
            for key in ["dimensions", "transform", "intensity"] { lights[lightIndex][key] = emitter[key] }
            var next = candidate
            next["lights"] = .array(lights)
            return next
        }
        guard light["type"] == .string("point"),
              let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double,
              let depth = room["dimensions"]["depth"].double,
              case .object(var row) = light else { throw invalid }
        var pose = light["transform"]; pose["position"] = .vector([0, height * 0.9, 0]); row["transform"] = pose
        row.removeValue(forKey: "targetPlacementId")
        lights[lightIndex] = .object(row)
        guard case .object(var binding) = bindings[bindingIndex] else { throw invalid }
        binding["range"] = .number(try range(width: width, height: height, depth: depth)); binding.removeValue(forKey: "aim")
        bindings[bindingIndex] = .object(binding)
        var next = candidate; next["lights"] = .array(lights); next["extensions"][details] = .object(["version": .integer(1), "lights": .array(bindings)])
        return next
    }
    static func setCeilingFillBrightness(_ brightness: Double, lightId: String, in candidate: DocumentValue) throws -> DocumentValue {
        var bindings = try NativeLightFixture.bindings(in: candidate)
        guard var lights = candidate["lights"].items, let lightIndex = lights.firstIndex(where: { same($0["id"].string, lightId) }),
              let binding = bindings.firstIndex(where: { same($0["lightId"].string, lightId) }),
              bindings[binding]["purpose"] == .string("room-base"),
              let roomId = lights[lightIndex]["roomId"].string,
              let room = candidate["rooms"].items?.first(where: { same($0["id"].string, roomId) }) else { throw invalid }
        let old = try RoomCeilingFill.brightness(in: bindings[binding]["ceilingFill"])
        if old == brightness { return candidate }
        lights[lightIndex]["intensity"] = try RoomCeilingFill.emitter(dimensions: room["dimensions"], brightness: brightness)["intensity"]
        bindings[binding]["ceilingFill"]["brightness"] = .number(brightness)
        var next = candidate
        next["lights"] = .array(lights)
        next["extensions"][details]["lights"] = .array(bindings)
        return next
    }
    static func setPlacement(_ placement: RoomBaseLightPlacement, lightId: String, in candidate: DocumentValue) throws -> DocumentValue {
        var bindings = try NativeLightFixture.bindings(in: candidate)
        guard let index = bindings.firstIndex(where: { same($0["lightId"].string, lightId) && $0["purpose"] == .string("room-base") }),
              let light = candidate["lights"].items?.first(where: { same($0["id"].string, lightId) }),
              let room = light["roomId"].string else { throw invalid }
        bindings[index]["basePlacement"] = .string(placement.rawValue)
        var next = candidate; next["extensions"][details] = .object(["version": .integer(1), "lights": .array(bindings)])
        if placement == .auto {
            if next["extensions"][attachments] != .null {
                guard let rows = next["extensions"][attachments]["lights"].items else { throw invalid }
                next["extensions"][attachments]["lights"] = .array(rows.filter { !same($0["lightId"].string, lightId) })
            }
            next = try reflow(next, roomId: room)
        }
        return next
    }
    static func manualize(_ candidate: DocumentValue, lightId: String) throws -> DocumentValue {
        var bindings = try NativeLightFixture.bindings(in: candidate)
        guard let index = bindings.firstIndex(where: { same($0["lightId"].string, lightId) }),
              bindings[index]["basePlacement"] == .string("auto") else { return candidate }
        bindings[index]["basePlacement"] = .string("manual")
        var next = candidate; next["extensions"][details] = .object(["version": .integer(1), "lights": .array(bindings)])
        return next
    }
    static func preserveCeilingFillAfterCeilingEdit(_ candidate: DocumentValue, roomId: String) throws -> DocumentValue {
        guard let (_, light, index) = try baseLight(in: candidate, roomId: roomId),
              let id = light["id"].string,
              let room = candidate["rooms"].items?.first(where: { same($0["id"].string, roomId) }) else { return candidate }
        let binding = try NativeLightFixture.bindings(in: candidate)[index]
        guard binding["basePlacement"] == .string("auto"), binding["ceilingFill"] != .null,
              (try? RoomCeilingFill.dimensions(in: candidate, room: room)) == nil else { return candidate }
        return try manualize(candidate, lightId: id)
    }
    /// Only the rotation representation receives the shared semantic tolerance.
    /// Position, scale, the emitted candidate and every other field stay exact.
    static func samePose(_ a: DocumentValue, _ b: DocumentValue) -> Bool {
        guard a["position"] == b["position"], a["scale"] == b["scale"],
              let aa = a["rotation"].items, let bb = b["rotation"].items, aa.count == 4, bb.count == 4 else { return false }
        let x = aa.compactMap(\.double), y = bb.compactMap(\.double)
        guard x.count == 4, y.count == 4, (x + y).allSatisfy(\.isFinite) else { return false }
        func norm(_ values: [Double]) -> Double {
            let maximum = values.map(abs).max()!
            guard maximum > 0 else { return 0 }
            var sum = 0.0, compensation = 0.0
            for value in values {
                let n = value / maximum, term = n * n - compensation, previous = sum
                sum += term; compensation = (sum - previous) - term
            }
            return sum.squareRoot() * maximum
        }
        let an = norm(x), bn = norm(y)
        guard an.isFinite, bn.isFinite, an > 0, bn > 0 else { return false }
        return [1.0, -1.0].contains { sign in zip(x, y).allSatisfy { pair in abs(pair.0 / an - sign * pair.1 / bn) <= 1e-12 } }
    }
    static func settingsForEdit(_ settings: DocumentValue, light: DocumentValue, original: DocumentValue,
                                base: DocumentValue, mounted: Bool = false) throws -> DocumentValue {
        guard case .object(var fields) = settings, let id = original["id"].string else { throw invalid }
        let prior = try NativeLightFixture.bindings(in: base).first { same($0["lightId"].string, id) }
        var marker = settings
        if fields["purpose"] == nil, let purpose = prior?["purpose"], purpose != .null { marker["purpose"] = purpose }
        try checkMarker(marker)
        if let supplied = fields["basePlacement"], supplied != prior?["basePlacement"] { throw invalid }
        if let supplied = fields["purpose"], supplied != prior?["purpose"] { throw invalid }
        if let supplied = fields["ceilingFill"], supplied != prior?["ceilingFill"] { throw invalid }
        if let fill = prior?["ceilingFill"], fill != .null { fields["ceilingFill"] = fill }
        func spatial(_ row: DocumentValue) -> DocumentValue { .array([row["type"], row["dimensions"], row["beamAngle"], row["targetPlacementId"]]) }
        let spatialChanged = !samePose(original["transform"], light["transform"]) || spatial(original) != spatial(light)
            || (fields["aim"] != nil && settings["aim"] != (prior?["aim"] ?? .null))
            || (fields["range"] != nil && settings["range"] != (prior?["range"] ?? .null))
        let old = prior?["basePlacement"].string.flatMap(RoomBaseLightPlacement.init(rawValue:))
        let placement: RoomBaseLightPlacement? = old == .auto && (spatialChanged || mounted) ? .manual : old
        if placement == .auto, fields["range"] == nil, let range = prior?["range"] { fields["range"] = range }
        if let purpose = prior?["purpose"], purpose != .null { fields["purpose"] = purpose }
        if let placement { fields["basePlacement"] = .string(placement.rawValue) }
        return .object(fields)
    }
}
