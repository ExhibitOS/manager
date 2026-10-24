// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Bounded metadata for an existing exhibition placement. Geometry, floor
/// intersections and affine alignment belong to the shared Viewer resolver.
public struct InstallationSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/installation"
    public enum Preset: String, CaseIterable, Sendable { case paint, wood, stone, metal }
    public struct Material: Equatable, Sendable {
        public var preset: Preset
        public var color: String
        public var roughness: Double
        public var metalness: Double
        public init(preset: Preset = .paint, color: String = "#F2F2F2", roughness: Double = 0.7, metalness: Double = 0) {
            self.preset = preset; self.color = color; self.roughness = roughness; self.metalness = metalness
        }
        public static func defaults(_ preset: Preset) -> Self {
            switch preset {
            case .paint: Self()
            case .wood: Self(preset: .wood, color: "#9B7653", roughness: 0.65)
            case .stone: Self(preset: .stone, color: "#B8B8B4", roughness: 0.8)
            case .metal: Self(preset: .metal, color: "#BFC2C7", roughness: 0.35, metalness: 1)
            }
        }
        public static func == (lhs: Self, rhs: Self) -> Bool {
            lhs.preset == rhs.preset && lhs.color.lowercased() == rhs.color.lowercased() && lhs.roughness == rhs.roughness && lhs.metalness == rhs.metalness
        }
        fileprivate var value: DocumentValue { .object(["preset": .string(preset.rawValue), "color": .string(color), "roughness": .number(roughness), "metalness": .number(metalness)]) }
        fileprivate func validate(frame: Bool) throws {
            guard InstallationSettings.color(color), InstallationSettings.bounded(roughness, 0...1), InstallationSettings.bounded(metalness, 0...1), !frame || preset != .stone else { throw InstallationSettings.invalid() }
        }
    }
    public struct Pedestal: Equatable, Sendable {
        public enum Kind: String, CaseIterable, Sendable { case none, box, cylinder }
        public var kind: Kind
        /// Empty until the author explicitly supplies a room-local base point.
        public var position: [Double]
        public var yaw: Double
        public var width: Double
        public var depth: Double
        public var height: Double
        public var diameter: Double
        public var material: Material
        public init(kind: Kind = .none, position: [Double] = [], yaw: Double = 0, width: Double = 1, depth: Double = 1, height: Double = 0.8, diameter: Double = 1, material: Material = Material()) {
            self.kind = kind; self.position = position; self.yaw = yaw; self.width = width; self.depth = depth; self.height = height; self.diameter = diameter; self.material = material
        }
        public static func == (lhs: Self, rhs: Self) -> Bool {
            guard lhs.kind == rhs.kind else { return false }
            if lhs.kind == .none { return true }
            return lhs.position == rhs.position && lhs.yaw == rhs.yaw && lhs.height == rhs.height && lhs.material == rhs.material && (lhs.kind == .box ? lhs.width == rhs.width && lhs.depth == rhs.depth : lhs.diameter == rhs.diameter)
        }
        fileprivate func validate() throws {
            guard kind != .none else { return }
            guard position.count == 3, position.allSatisfy({ InstallationSettings.bounded($0, -10000...10000) }), InstallationSettings.bounded(yaw, -Double.pi...Double.pi), InstallationSettings.bounded(height, 0.01...5), kind == .box ? InstallationSettings.bounded(width, 0.01...10) && InstallationSettings.bounded(depth, 0.01...10) : InstallationSettings.bounded(diameter, 0.01...10) else { throw InstallationSettings.invalid() }
            try material.validate(frame: false)
        }
        fileprivate var value: DocumentValue {
            if kind == .none { return .object(["kind": .string("none")]) }
            let dimensions: DocumentValue = kind == .box ? .object(["width": .number(width), "depth": .number(depth), "height": .number(height)]) : .object(["diameter": .number(diameter), "height": .number(height)])
            return .object(["kind": .string(kind.rawValue), "position": .vector(position), "yaw": .number(yaw), "dimensions": dimensions, "material": material.value])
        }
    }
    public struct Frame: Equatable, Sendable {
        public enum Kind: String, CaseIterable, Sendable { case none, flat, stepped, floating }
        public var kind: Kind
        public var borderWidth: Double
        public var thickness: Double
        public var depth: Double
        public var gap: Double
        public var matWidth: Double
        public var matColor: String
        public var material: Material
        public init(kind: Kind = .none, borderWidth: Double = 0.04, thickness: Double = 0.02, depth: Double = 0.015, gap: Double = 0, matWidth: Double = 0, matColor: String = "#FFFFFF", material: Material = Material()) {
            self.kind = kind; self.borderWidth = borderWidth; self.thickness = thickness; self.depth = depth; self.gap = gap; self.matWidth = matWidth; self.matColor = matColor; self.material = material
        }
        public static func == (lhs: Self, rhs: Self) -> Bool {
            guard lhs.kind == rhs.kind else { return false }
            if lhs.kind == .none { return true }
            return lhs.borderWidth == rhs.borderWidth && lhs.thickness == rhs.thickness && lhs.depth == rhs.depth && lhs.gap == rhs.gap && lhs.matWidth == rhs.matWidth && lhs.matColor.lowercased() == rhs.matColor.lowercased() && lhs.material == rhs.material
        }
        fileprivate func validate() throws {
            guard kind != .none else { return }
            guard InstallationSettings.bounded(borderWidth, 0.005...0.5), InstallationSettings.bounded(thickness, 0.002...0.2), InstallationSettings.bounded(depth, 0...0.5), InstallationSettings.bounded(gap, 0...0.2), kind == .floating || gap == 0, InstallationSettings.bounded(matWidth, 0...0.5), InstallationSettings.color(matColor) else { throw InstallationSettings.invalid() }
            try material.validate(frame: true)
        }
        fileprivate var value: DocumentValue {
            if kind == .none { return .object(["kind": .string("none")]) }
            return .object(["kind": .string(kind.rawValue), "borderWidth": .number(borderWidth), "thickness": .number(thickness), "depth": .number(depth), "gap": .number(gap), "matWidth": .number(matWidth), "matColor": .string(matColor), "material": material.value])
        }
    }
    public var pedestal: Pedestal?
    public var frame: Frame?
    public init(pedestal: Pedestal? = nil, frame: Frame? = nil) { self.pedestal = pedestal; self.frame = frame }
    private static func invalid() -> AuthoringFailure { AuthoringFailure("INSTALLATION_SETTINGS_INVALID") }
    private static func bounded(_ value: Double, _ range: ClosedRange<Double>) -> Bool { value.isFinite && range.contains(value) }
    private static func color(_ value: String) -> Bool { value.utf8.count == 7 && value.first == "#" && value.dropFirst().utf8.allSatisfy { (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) } }
    private static func uuid(_ value: String) -> Bool { UUID(uuidString: value)?.uuidString.lowercased() == value.lowercased() }
    private static func object(_ value: DocumentValue) throws -> [String: DocumentValue] { guard case .object(let fields) = value else { throw invalid() }; return fields }
    private static func object(_ value: DocumentValue, keys: Set<String>) throws -> [String: DocumentValue] { let fields = try object(value); guard Set(fields.keys) == keys else { throw invalid() }; return fields }
    private static func number(_ value: DocumentValue?) throws -> Double { guard let number = value?.double, number.isFinite else { throw invalid() }; return number }
    private static func readMaterial(_ value: DocumentValue, frame: Bool) throws -> Material {
        let v = try object(value, keys: ["preset", "color", "roughness", "metalness"])
        guard let raw = v["preset"]?.string, let preset = Preset(rawValue: raw), let color = v["color"]?.string else { throw invalid() }
        let material = Material(preset: preset, color: color, roughness: try number(v["roughness"]), metalness: try number(v["metalness"]))
        try material.validate(frame: frame); return material
    }
    private static func readPedestal(_ value: DocumentValue) throws -> Pedestal {
        let raw = try object(value)
        guard let name = raw["kind"]?.string, let kind = Pedestal.Kind(rawValue: name) else { throw invalid() }
        if kind == .none { _ = try object(value, keys: ["kind"]); return Pedestal() }
        let v = try object(value, keys: ["kind", "position", "yaw", "dimensions", "material"])
        guard let point = v["position"]?.items else { throw invalid() }
        let d = try object(v["dimensions"]!, keys: kind == .box ? ["width", "depth", "height"] : ["diameter", "height"])
        var result = Pedestal(kind: kind, position: try point.map { try number($0) }, yaw: try number(v["yaw"]), height: try number(d["height"]), material: try readMaterial(v["material"]!, frame: false))
        if kind == .box { result.width = try number(d["width"]); result.depth = try number(d["depth"]) } else { result.diameter = try number(d["diameter"]) }
        try result.validate(); return result
    }
    private static func readFrame(_ value: DocumentValue) throws -> Frame {
        let raw = try object(value)
        guard let name = raw["kind"]?.string, let kind = Frame.Kind(rawValue: name) else { throw invalid() }
        if kind == .none { _ = try object(value, keys: ["kind"]); return Frame() }
        let v = try object(value, keys: ["kind", "borderWidth", "thickness", "depth", "gap", "matWidth", "matColor", "material"])
        guard let color = v["matColor"]?.string else { throw invalid() }
        let result = Frame(kind: kind, borderWidth: try number(v["borderWidth"]), thickness: try number(v["thickness"]), depth: try number(v["depth"]), gap: try number(v["gap"]), matWidth: try number(v["matWidth"]), matColor: color, material: try readMaterial(v["material"]!, frame: true))
        try result.validate(); return result
    }
    private static func placements(_ candidate: DocumentValue) throws -> [String: String] {
        guard let rows = candidate["placements"].items else { throw invalid() }
        var ids: [String: String] = [:]
        for row in rows {
            guard let id = row["id"].string, uuid(id), ids[id.lowercased()] == nil else { throw invalid() }
            ids[id.lowercased()] = id
        }
        return ids
    }
    private struct Binding { let id: String; let raw: DocumentValue; let settings: InstallationSettings }
    private static func bindings(_ candidate: DocumentValue, checkReferences: Bool = true) throws -> [Binding]? {
        let root = try object(candidate)
        guard let extensions = root["extensions"] else { return nil }
        let all = try object(extensions)
        guard let encoded = all[namespace] else { return nil }
        let value = try object(encoded, keys: ["version", "placements"])
        guard value["version"]?.double == 1, let rows = value["placements"]?.items, rows.count <= 128 else { throw invalid() }
        let targets = checkReferences ? try placements(candidate) : [:]
        var ids = Set<String>()
        return try rows.map { row in
            let fields = try object(row)
            guard fields["placementId"] != nil, Set(fields.keys).isSubset(of: ["placementId", "pedestal", "frame"]), let id = fields["placementId"]?.string, uuid(id), ids.insert(id.lowercased()).inserted, !checkReferences || targets[id.lowercased()] != nil else { throw invalid() }
            return Binding(id: id, raw: row, settings: Self(pedestal: try fields["pedestal"].map(readPedestal), frame: try fields["frame"].map(readFrame)))
        }
    }
    private func record(id: String) throws -> DocumentValue {
        try pedestal?.validate(); try frame?.validate()
        var fields: [String: DocumentValue] = ["placementId": .string(id)]
        if let pedestal { fields["pedestal"] = pedestal.value }
        if let frame { fields["frame"] = frame.value }
        return .object(fields)
    }
    public static func read(from draft: NativeExhibitionDraft, placementId: String) throws -> Self? {
        guard uuid(placementId), try placements(draft.candidate)[placementId.lowercased()] != nil else { throw invalid() }
        return try bindings(draft.candidate)?.first(where: { $0.id.lowercased() == placementId.lowercased() })?.settings
    }
    fileprivate static func set(_ settings: Self?, on draft: inout NativeExhibitionDraft, placementId: String) throws {
        guard uuid(placementId), let actual = try placements(draft.candidate)[placementId.lowercased()] else { throw invalid() }
        let old = try bindings(draft.candidate) ?? []
        let index = old.firstIndex(where: { $0.id.lowercased() == placementId.lowercased() })
        let encoded = try settings?.record(id: actual)
        if index.map({ old[$0].settings }) == settings { return }
        var rows = old.map(\.raw)
        if let index { if let encoded { rows[index] = encoded } else { rows.remove(at: index) } }
        else if let encoded { guard rows.count < 128 else { throw invalid() }; rows.append(encoded) }
        var candidate = draft.candidate
        var extensions = try object(candidate)["extensions"].map(object) ?? [:]
        if rows.isEmpty { extensions.removeValue(forKey: namespace) }
        else { extensions[namespace] = .object(["version": .integer(1), "placements": .array(rows)]) }
        candidate["extensions"] = .object(extensions)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        try draft.replaceCandidate(candidate)
    }
    /// Native identity-copy retains placement IDs. OEX import supplies its real
    /// placement map. Only this typed reference is rewritten; opaque text stays.
    public static func remappingPlacementReferences(in candidate: DocumentValue, using replacements: [String: String]? = nil) throws -> DocumentValue {
        guard let rows = try bindings(candidate, checkReferences: false) else { return candidate }
        let targets = try placements(candidate)
        var normalized: [String: String] = [:], destinations = Set<String>()
        for (old, new) in replacements ?? targets {
            guard uuid(old), uuid(new), normalized[old.lowercased()] == nil, destinations.insert(new.lowercased()).inserted, targets[new.lowercased()] != nil else { throw invalid() }
            normalized[old.lowercased()] = new
        }
        var used = Set<String>()
        let mapped = try rows.map { binding -> DocumentValue in
            guard let mapped = normalized[binding.id.lowercased()], let actual = targets[mapped.lowercased()], used.insert(actual.lowercased()).inserted else { throw invalid() }
            var row = binding.raw; row["placementId"] = .string(actual); return row
        }
        var result = candidate
        result["extensions"][namespace] = .object(["version": .integer(1), "placements": .array(mapped)])
        return result
    }
}

extension NativeExhibitionDraft {
    public func installationSettings(for placementId: String) throws -> InstallationSettings? { try InstallationSettings.read(from: self, placementId: placementId) }
    public mutating func setInstallation(_ settings: InstallationSettings?, for placementId: String) throws { try InstallationSettings.set(settings, on: &self, placementId: placementId) }
    /// Closed alignment results may translate this placement only. Configuration
    /// is explicitly applied beforehand; no WebView extension write is allowed.
    public mutating func acceptInstallationTranslation(_ candidate: DocumentValue, placementId: String, axes: Set<Int>) throws {
        guard !axes.isEmpty, axes.isSubset(of: [0, 1, 2]), case .object(let old) = self.candidate, case .object(let next) = candidate, old.filter({ $0.key != "placements" }) == next.filter({ $0.key != "placements" }), let before = old["placements"]?.items, let after = next["placements"]?.items, before.count == after.count else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        var found = false
        for index in before.indices {
            if before[index]["id"].string?.lowercased() == placementId.lowercased() {
                guard !found, case .object(let prior) = before[index], case .object(let proposed) = after[index], prior.filter({ $0.key != "transform" }) == proposed.filter({ $0.key != "transform" }), case .object(let oldPose) = prior["transform"], case .object(let newPose) = proposed["transform"], oldPose.filter({ $0.key != "position" }) == newPose.filter({ $0.key != "position" }), let p = oldPose["position"]?.items, let q = newPose["position"]?.items, p.count == 3, q.count == 3 else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
                for axis in 0..<3 {
                    guard let value = q[axis].double, value.isFinite, abs(value) <= 1_000_000, axes.contains(axis) || p[axis] == q[axis] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
                }
                found = true
            } else { guard before[index] == after[index] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") } }
        }
        guard found, try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        if candidate != self.candidate { try replaceCandidate(candidate) }
    }
}
