// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Native metadata only. Shared Viewer owns PBR resolution, floor hits and audio.
public struct FloorSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/floors"
    public enum Preset: String, CaseIterable, Sendable { case wood, marble, tile, carpet }
    public enum Field: String, CaseIterable, Sendable {
        case color, roughness, metalness, repeatMeters, rotation, texturePack, normalScale, aoStrength, sound
    }
    public struct Target: Equatable, Hashable, Sendable {
        public enum Kind: String, Sendable { case surface, roomBase = "room-base" }
        public let kind: Kind
        public let id: String
        public init(kind: Kind, id: String) { self.kind = kind; self.id = id }
        public var key: String { kind.rawValue + ":" + id.lowercased() }
        public var value: DocumentValue { .object(["kind": .string(kind.rawValue), kind == .surface ? "surfaceId" : "roomId": .string(id)]) }
    }
    public private(set) var value: DocumentValue
    public var preset: Preset { Preset(rawValue: value["preset"].string!)! }
    public var target: Target { try! Self.target(value["target"]) }
    /// Exact shared floor-v1 authored defaults (provider maps remain factors).
    public static func defaults(_ preset: Preset) -> [Field: DocumentValue] {
        let pack: String, meters: Double
        switch preset {
        case .wood: pack = "WoodFloor051"; meters = 1.8
        case .marble: pack = "Marble006"; meters = 1
        case .tile: pack = "Tiles074"; meters = 2.1
        case .carpet: pack = "Carpet004"; meters = 1
        }
        return [.color: .string("#ffffff"), .roughness: .number(1), .metalness: .number(0),
                .repeatMeters: .vector([meters, meters]), .rotation: .number(0),
                .texturePack: .object(["id": .string(pack), "version": .integer(1)]),
                .normalScale: .number(1), .aoStrength: .number(1),
                .sound: .object(["mode": .string("material-default"), "gain": .number(1)])]
    }
    public init(value: DocumentValue, candidate: DocumentValue) throws {
        try Self.validate(value, candidate: candidate)
        self.value = value
    }
    /// Defaults are supplied from the shared preset table, never inferred from
    /// names/colors or duplicated as a second renderer material implementation.
    public static func applying(preset: Preset, defaults: [Field: DocumentValue], fields: Set<Field>, target: Target, prior: Self?, candidate: DocumentValue) throws -> Self {
        var result = prior?.value ?? .object(["target": target.value, "preset": .string(preset.rawValue)])
        // New binding requires an explicit complete base even for a scoped edit.
        if prior == nil {
            guard let repeatValue = defaults[.repeatMeters], let rotation = defaults[.rotation] else { throw invalid() }
            result["repeatMeters"] = repeatValue; result["rotation"] = rotation
        }
        // Appearance-only never silently retargets an authored default sound.
        // When changing base, freeze every unselected inherited visual value.
        let keepBase = prior.map { value in
            value.value["sound"]["mode"].string == "material-default" && !fields.contains(.sound)
                && Self.soundFamily(value.preset) != Self.soundFamily(preset)
        } ?? false
        let base = keepBase ? prior!.preset : preset
        if let prior, base != prior.preset {
            let effective = try prior.effectiveAppearance(candidate: candidate)
            for field in Field.allCases where field != .sound && !fields.contains(field) {
                guard let old = effective[field] else { throw invalid() }
                if [.color, .roughness, .metalness].contains(field) {
                    var material = try optionalObject(result["materialOverride"])
                    material[field.rawValue] = old; result["materialOverride"] = .object(material)
                } else { result[field.rawValue] = old }
            }
        }
        result["preset"] = .string(base.rawValue)
        for field in fields {
            guard let replacement = defaults[field] else { throw invalid() }
            if [.color, .roughness, .metalness].contains(field) {
                var material = try optionalObject(result["materialOverride"])
                material[field.rawValue] = replacement; result["materialOverride"] = .object(material)
            } else { result[field.rawValue] = replacement }
        }
        return try Self(value: result, candidate: candidate)
    }
    public func effectiveAppearance(candidate: DocumentValue) throws -> [Field: DocumentValue] {
        var result = Self.defaults(preset)
        if target.kind == .surface, case .object(let assignments) = candidate["extensions"]["org.exhibitos.studio/materials"]["surfaces"],
           let legacy = assignments.first(where: { $0.key.lowercased() == target.id.lowercased() })?.value {
            for field in [Field.color, .roughness, .metalness] { result[field] = legacy[field.rawValue] }
        }
        let row = try Self.object(value)
        for field in Field.allCases where field != .sound {
            if [.color, .roughness, .metalness].contains(field) {
                if case .object(let overrides) = row["materialOverride"], let explicit = overrides[field.rawValue] { result[field] = explicit }
            } else if let explicit = row[field.rawValue] { result[field] = explicit }
        }
        return result
    }
    public func replacing(_ field: Field, with replacement: DocumentValue, candidate: DocumentValue) throws -> Self {
        var next = value
        if [.color, .roughness, .metalness].contains(field) {
            var material = try Self.optionalObject(next["materialOverride"])
            material[field.rawValue] = replacement; next["materialOverride"] = .object(material)
        } else { next[field.rawValue] = replacement }
        return try Self(value: next, candidate: candidate)
    }
    private static func soundFamily(_ preset: Preset) -> String {
        preset == .marble || preset == .tile ? "stone" : preset.rawValue
    }
    private static func invalid() -> AuthoringFailure { AuthoringFailure("FLOOR_SETTINGS_INVALID") }
    private static func object(_ value: DocumentValue) throws -> [String: DocumentValue] {
        guard case .object(let fields) = value else { throw invalid() }; return fields
    }
    private static func optionalObject(_ value: DocumentValue) throws -> [String: DocumentValue] { value == .null ? [:] : try object(value) }
    private static func keys(_ value: DocumentValue, required: Set<String>, optional: Set<String> = []) throws -> [String: DocumentValue] {
        let fields = try object(value)
        guard required.isSubset(of: Set(fields.keys)), Set(fields.keys).isSubset(of: required.union(optional)) else { throw invalid() }; return fields
    }
    private static func uuid(_ id: String) -> Bool { UUID(uuidString: id)?.uuidString.lowercased() == id.lowercased() }
    private static func bounded(_ value: DocumentValue?, _ range: ClosedRange<Double>) throws {
        guard let number = value?.double, number.isFinite, range.contains(number) else { throw invalid() }
    }
    private static func target(_ value: DocumentValue) throws -> Target {
        guard let raw = value["kind"].string, let kind = Target.Kind(rawValue: raw) else { throw invalid() }
        let key = kind == .surface ? "surfaceId" : "roomId"
        _ = try keys(value, required: ["kind", key])
        guard let id = value[key].string, uuid(id) else { throw invalid() }
        return Target(kind: kind, id: id)
    }
    public static func targets(in candidate: DocumentValue) throws -> [Target] {
        var result: [Target] = [], seen = Set<String>()
        for (key, kind) in [("surfaces", Target.Kind.surface), ("rooms", .roomBase)] {
            guard let rows = candidate[key].items else { throw invalid() }
            for row in rows {
                if kind == .surface && row["type"].string != "floor" { continue }
                guard let id = row["id"].string, uuid(id) else { throw invalid() }
                let item = Target(kind: kind, id: id)
                guard seen.insert(item.key).inserted else { throw invalid() }; result.append(item)
            }
        }
        return result
    }
    public static func isActive(_ target: Target, in candidate: DocumentValue) -> Bool {
        target.kind == .surface || !(candidate["surfaces"].items ?? []).contains { $0["type"].string == "floor" && $0["roomId"].string?.lowercased() == target.id.lowercased() }
    }
    private static func validate(_ row: DocumentValue, candidate: DocumentValue) throws {
        let v = try keys(row, required: ["target", "preset", "repeatMeters", "rotation"], optional: ["materialOverride", "texturePack", "normalScale", "aoStrength", "sound"])
        let target = try target(row["target"])
        guard try targets(in: candidate).contains(where: { $0.key == target.key }), let name = v["preset"]?.string, Preset(rawValue: name) != nil, let repeatValues = v["repeatMeters"]?.items, repeatValues.count == 2 else { throw invalid() }
        for number in repeatValues { try bounded(number, 0.05...20) }; try bounded(v["rotation"], -Double.pi...Double.pi)
        if let material = v["materialOverride"] {
            let fields = try keys(material, required: [], optional: ["color", "roughness", "metalness"])
            guard !fields.isEmpty else { throw invalid() }
            if let color = fields["color"] {
                guard let text = color.string, text.range(of: "^#[0-9a-f]{6}$", options: .regularExpression) != nil else { throw invalid() }
            }
            for key in ["roughness", "metalness"] { if let value = fields[key] { try bounded(value, 0...1) } }
        }
        if let pack = v["texturePack"] {
            let p = try keys(pack, required: ["id", "version"])
            guard let id = p["id"]?.string, ["WoodFloor051", "Marble006", "Tiles074", "Carpet004"].contains(id), p["version"]?.double == 1 else { throw invalid() }
        }
        if let normal = v["normalScale"] { try bounded(normal, 0...2) }
        if let ao = v["aoStrength"] { try bounded(ao, 0...1) }
        if let sound = v["sound"] {
            let s = try keys(sound, required: ["mode", "gain"], optional: ["assetIds"])
            guard let mode = s["mode"]?.string, ["material-default", "explicit", "none"].contains(mode) else { throw invalid() }
            try bounded(s["gain"], 0...1)
            if mode == "explicit" {
                guard let assets = s["assetIds"]?.items, (1...16).contains(assets.count) else { throw invalid() }
                let media = candidate["mediaAssets"].items ?? []; var seen = Set<String>()
                for asset in assets {
                    guard let id = asset.string, uuid(id), seen.insert(id.lowercased()).inserted,
                          media.contains(where: { $0["id"].string?.lowercased() == id.lowercased() && $0["mime"].string == "audio/wav" }) else { throw invalid() }
                }
            } else if s["assetIds"] != nil { throw invalid() }
        }
    }
    public static func all(in candidate: DocumentValue) throws -> [Self] {
        let root = try object(candidate)
        guard let extensions = root["extensions"] else { return [] }
        guard let value = try object(extensions)[namespace] else { return [] }
        let fields = try keys(value, required: ["version", "bindings"])
        guard fields["version"]?.double == 1, let rows = fields["bindings"]?.items, rows.count <= 64, try JSONEncoder().encode(value).count <= 16_384 else { throw invalid() }
        var seen = Set<String>()
        return try rows.map { row in
            let item = try Self(value: row, candidate: candidate)
            guard seen.insert(item.target.key).inserted else { throw invalid() }; return item
        }
    }
    public static func read(from draft: NativeExhibitionDraft, target: Target) throws -> Self? { try all(in: draft.candidate).first { $0.target.key == target.key } }
    /// Authored presence counts, including zero/default-equal values. Legacy
    /// extension values are only inspected; never rewritten by floor edits.
    public static func customizedFields(target: Target, candidate: DocumentValue) throws -> Set<Field> {
        var fields = Set<Field>()
        if let binding = try all(in: candidate).first(where: { $0.target.key == target.key }) {
            let row = try object(binding.value)
            fields.formUnion([.repeatMeters, .rotation, .texturePack]) // authored preset is an appearance choice
            if let material = row["materialOverride"] { for key in try object(material).keys { if let field = Field(rawValue: key) { fields.insert(field) } } }
            for key in ["normalScale", "aoStrength", "sound"] { if row[key] != nil, let field = Field(rawValue: key) { fields.insert(field) } }
        }
        if target.kind == .surface {
            if case .object(let materials) = candidate["extensions"]["org.exhibitos.studio/materials"]["surfaces"], materials.keys.contains(where: { $0.lowercased() == target.id.lowercased() }) { fields.formUnion([.color, .roughness, .metalness]) }
            if (candidate["extensions"]["org.exhibitos.viewer/experience"]["footsteps"].items ?? []).contains(where: { $0["surfaceId"].string?.lowercased() == target.id.lowercased() }) { fields.insert(.sound) }
        }
        return fields
    }
    public static func set(_ settings: Self?, target: Target, on draft: inout NativeExhibitionDraft) throws {
        guard let actual = try targets(in: draft.candidate).first(where: { $0.key == target.key }) else { throw invalid() }
        var rows = try all(in: draft.candidate).map(\.value)
        let index = rows.firstIndex { (try? Self.target($0["target"]).key) == target.key }
        if index == nil && settings == nil { return }
        if let index, settings?.value == rows[index] { return }
        var encoded = settings?.value
        if encoded != nil {
            guard settings?.target.key == target.key else { throw invalid() }
            encoded?["target"] = actual.value
            try validate(encoded!, candidate: draft.candidate)
        }
        if let index { if let encoded { rows[index] = encoded } else { rows.remove(at: index) } }
        else if let encoded { guard rows.count < 64 else { throw invalid() }; rows.append(encoded) }
        var candidate = draft.candidate, extensions = try optionalObject(draft.candidate["extensions"])
        if rows.isEmpty { extensions.removeValue(forKey: namespace) }
        else { extensions[namespace] = .object(["version": .integer(1), "bindings": .array(rows)]) }
        candidate["extensions"] = .object(extensions)
        _ = try all(in: candidate)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        if candidate != draft.candidate { try draft.replaceCandidate(candidate) }
    }
}

extension NativeExhibitionDraft {
    public mutating func setFloor(_ settings: FloorSettings?, target: FloorSettings.Target) throws { try FloorSettings.set(settings, target: target, on: &self) }
}
