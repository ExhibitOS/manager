// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Absence is historical UNKNOWN, never an inferred automatic setting.
public enum RoomBaseLightPlacement: String, Codable, Sendable {
    case auto, manual
    public static let profile = "room-base-light-placement-v1"
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
        guard light["type"] == .string("point"), let room = candidate["rooms"].items?.first(where: { same($0["id"].string, roomId) }),
              let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double,
              let depth = room["dimensions"]["depth"].double, var lights = candidate["lights"].items,
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
