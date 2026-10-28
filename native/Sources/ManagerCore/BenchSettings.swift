// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Native consumer of the shared benches v1 profile. Renderer, floor/obstacle
/// admission and seated return remain owned by the shared Viewer implementation.
public struct BenchSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/benches"
    public static let presentationNamespace = "org.exhibitos.studio/presentation"
    public enum Kind: String, CaseIterable, Sendable { case slab, backed }
    public let id: String, name: String, roomId: String, viewpointId: String
    public let kind: Kind
    public let position: [Double]
    public let yaw: Double
    public let width: Double, depth: Double, height: Double // Height is the seat top.
    public let material: InstallationSettings.Material

    public init(id: String, name: String, roomId: String, kind: Kind, position: [Double], yaw: Double,
                width: Double, depth: Double, height: Double, material: InstallationSettings.Material, viewpointId: String) throws {
        guard Self.uuid(id), Self.uuid(roomId), Self.uuid(viewpointId), !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, name.utf16.count <= 128,
              Self.point(position), yaw.isFinite, (-Double.pi...Double.pi).contains(yaw),
              Self.bounded(width, 0.6...3), Self.bounded(depth, 0.3...1), Self.bounded(height, 0.3...0.65) else { throw Self.invalid() }
        do { try material.validate(frame: false) } catch { throw Self.invalid() }
        self.id = id; self.name = name; self.roomId = roomId; self.kind = kind; self.position = position; self.yaw = yaw
        self.width = width; self.depth = depth; self.height = height; self.material = material; self.viewpointId = viewpointId
    }
    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == Set(["id", "name", "roomId", "kind", "position", "yaw", "dimensions", "material", "viewpointId"]),
              let id = value["id"].string, let name = value["name"].string, let room = value["roomId"].string,
              let rawKind = value["kind"].string, let kind = Kind(rawValue: rawKind),
              let position = Self.numbers(value["position"], count: 3), let yaw = value["yaw"].double,
              case .object(let dimensions) = value["dimensions"], Set(dimensions.keys) == Set(["width", "depth", "height"]),
              let width = dimensions["width"]?.double, let depth = dimensions["depth"]?.double,
              let height = dimensions["height"]?.double, let viewpoint = value["viewpointId"].string else { throw Self.invalid() }
        let material: InstallationSettings.Material
        do { material = try InstallationSettings.readMaterial(value["material"], frame: false) } catch { throw Self.invalid() }
        try self.init(id: id, name: name, roomId: room, kind: kind, position: position, yaw: yaw,
                      width: width, depth: depth, height: height, material: material, viewpointId: viewpoint)
    }
    public var value: DocumentValue {
        .object(["id": .string(id), "name": .string(name), "roomId": .string(roomId), "kind": .string(kind.rawValue),
                 "position": .vector(position), "yaw": .number(yaw), "dimensions": .object(["width": .number(width), "depth": .number(depth), "height": .number(height)]),
                 "material": material.value, "viewpointId": .string(viewpointId)])
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("BENCH_FIELDS_INVALID") }
    static func uuid(_ value: String) -> Bool { UUID(uuidString: value) != nil }
    static func bounded(_ value: Double, _ range: ClosedRange<Double>) -> Bool { value.isFinite && range.contains(value) }
    static func point(_ value: [Double]) -> Bool { value.count == 3 && value.allSatisfy { bounded($0, -10_000...10_000) } }
    static func numbers(_ value: DocumentValue, count: Int) -> [Double]? {
        guard let values = value.items, values.count == count, values.allSatisfy({ $0.double?.isFinite == true }) else { return nil }
        return values.map { $0.double! }
    }
    static func extensionValue(_ name: String, in draft: NativeExhibitionDraft) throws -> DocumentValue? {
        guard case .object(let candidate) = draft.candidate else { throw invalid() }
        guard let raw = candidate["extensions"] else { return nil }
        guard case .object(let extensions) = raw else { throw invalid() }
        return extensions[name]
    }
    /// Metadata/reference checks only. Full shared validation precedes any
    /// preview/apply, including generated bounds and the seat camera profile.
    public static func list(in draft: NativeExhibitionDraft) throws -> [Self] {
        guard let raw = try extensionValue(namespace, in: draft) else { return [] }
        guard case .object(let fields) = raw, Set(fields.keys) == Set(["version", "benches"]),
              raw["version"] == .integer(1), let rows = raw["benches"].items, rows.count <= 32,
              try JSONEncoder().encode(raw).count <= 32_768 else { throw invalid() }
        let viewpoints = try BenchViewpoint.list(in: draft)
        let rooms = Set(draft.rooms.compactMap { $0["id"].string?.lowercased() })
        var seen = Set<String>()
        return try rows.map { row in
            let bench = try Self(value: row)
            guard seen.insert(bench.id.lowercased()).inserted, rooms.contains(bench.roomId.lowercased()),
                  viewpoints.contains(where: { $0.id.lowercased() == bench.viewpointId.lowercased() && $0.roomId.lowercased() == bench.roomId.lowercased() }) else { throw invalid() }
            return bench
        }
    }
}

/// Adapter for the EXISTING Presentation v1 viewpoint shape, not a new camera
/// namespace. Bench commands may add a fresh record; never modify a shared one.
public struct BenchViewpoint: Equatable, Sendable {
    public let id: String, name: String, roomId: String
    public let position: [Double], target: [Double]
    public let fov: Double
    public init(id: String, name: String, roomId: String, position: [Double], target: [Double], fov: Double) throws {
        guard BenchSettings.uuid(id), BenchSettings.uuid(roomId), (1...512).contains(name.utf16.count),
              BenchSettings.point(position), BenchSettings.point(target), BenchSettings.bounded(fov, 10...120),
              zip(position, target).contains(where: { abs($0.0 - $0.1) >= 1e-8 }) else { throw BenchSettings.invalid() }
        self.id = id; self.name = name; self.roomId = roomId; self.position = position; self.target = target; self.fov = fov
    }
    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == Set(["id", "name", "roomId", "position", "target", "fov"]),
              let id = value["id"].string, let name = value["name"].string, let room = value["roomId"].string,
              let position = BenchSettings.numbers(value["position"], count: 3), let target = BenchSettings.numbers(value["target"], count: 3),
              let fov = value["fov"].double else { throw BenchSettings.invalid() }
        try self.init(id: id, name: name, roomId: room, position: position, target: target, fov: fov)
    }
    public var value: DocumentValue { .object(["id": .string(id), "name": .string(name), "roomId": .string(roomId), "position": .vector(position), "target": .vector(target), "fov": .number(fov)]) }
    public static func list(in draft: NativeExhibitionDraft) throws -> [Self] {
        guard let raw = try BenchSettings.extensionValue(BenchSettings.presentationNamespace, in: draft) else { return [] }
        guard case .object(let fields) = raw, Set(fields.keys).isSubset(of: ["version", "viewpoints", "credits", "startCamera", "watermark"]),
              raw["version"] == .integer(1), let rows = raw["viewpoints"].items, rows.count <= 64,
              let credits = raw["credits"].string, credits.utf16.count <= 4096,
              try JSONEncoder().encode(raw).count <= 32_768 else { throw BenchSettings.invalid() }
        if let watermark = fields["watermark"] { _ = try PublicationWatermark(value: watermark) }
        var seen = Set<String>()
        return try rows.map { value in
            let result = try Self(value: value)
            guard seen.insert(result.id.lowercased()).inserted,
                  draft.rooms.contains(where: { $0["id"].string?.lowercased() == result.roomId.lowercased() }) else { throw BenchSettings.invalid() }
            return result
        }
    }
}

/// Single native text buffer, including invalid/incomplete input. Creating or
/// copying a viewpoint pins one fresh UUID for all previews/retries of this form.
public struct BenchFormBuffer: Equatable, Sendable {
    public let id: String, freshViewpointId: String
    public var name: String, roomId: String
    public var kind: BenchSettings.Kind = .slab
    public var x = "0.0", y = "0.0", z = "0.0", yawDegrees = "0.0"
    public var width = "1.5", depth = "0.5", height = "0.45"
    public var materialPreset = InstallationSettings.Preset.wood
    public var color = "#9B7653", roughness = "0.65", metalness = "0.0"
    public var usesFreshViewpoint: Bool
    public var viewpointId: String
    public var viewpointName: String
    public var viewX = "0.0", viewY = "1.2", viewZ = "0.0"
    public var targetX = "0.0", targetY = "1.2", targetZ = "-2.0", fov = "50.0"
    private let originalYaw: Double
    private let originalYawDegrees: String

    public init(roomId: String, name: String = "새 벤치") throws {
        guard BenchSettings.uuid(roomId) else { throw BenchSettings.invalid() }
        id = UUID().uuidString.lowercased(); freshViewpointId = UUID().uuidString.lowercased()
        self.roomId = roomId; self.name = name; usesFreshViewpoint = true
        viewpointId = ""; viewpointName = name + " 감상"
        originalYaw = 0; originalYawDegrees = "0.0"
    }
    public init(draft: NativeExhibitionDraft, benchId: String) throws {
        guard let bench = try BenchSettings.list(in: draft).first(where: { $0.id == benchId }),
              let view = try BenchViewpoint.list(in: draft).first(where: { $0.id.lowercased() == bench.viewpointId.lowercased() }) else { throw AuthoringFailure("BENCH_SELECTION_CHANGED") }
        id = bench.id; freshViewpointId = UUID().uuidString.lowercased(); name = bench.name; roomId = bench.roomId; kind = bench.kind
        x = String(bench.position[0]); y = String(bench.position[1]); z = String(bench.position[2])
        originalYaw = bench.yaw; originalYawDegrees = String(bench.yaw * 180 / .pi); yawDegrees = originalYawDegrees
        width = String(bench.width); depth = String(bench.depth); height = String(bench.height)
        materialPreset = bench.material.preset; color = bench.material.color; roughness = String(bench.material.roughness); metalness = String(bench.material.metalness)
        usesFreshViewpoint = false; viewpointId = bench.viewpointId; viewpointName = view.name
        viewX = String(view.position[0]); viewY = String(view.position[1]); viewZ = String(view.position[2])
        targetX = String(view.target[0]); targetY = String(view.target[1]); targetZ = String(view.target[2]); fov = String(view.fov)
    }
    private func number(_ text: String) throws -> Double {
        guard let n = Double(text.trimmingCharacters(in: .whitespacesAndNewlines)), n.isFinite else { throw BenchSettings.invalid() }; return n
    }
    public func settings() throws -> BenchSettings {
        let material = InstallationSettings.Material(preset: materialPreset, color: color, roughness: try number(roughness), metalness: try number(metalness))
        return try BenchSettings(id: id, name: name, roomId: roomId, kind: kind, position: [number(x), number(y), number(z)],
                                 yaw: yawDegrees == originalYawDegrees ? originalYaw : number(yawDegrees) * .pi / 180,
                                 width: number(width), depth: number(depth), height: number(height), material: material,
                                 viewpointId: usesFreshViewpoint ? freshViewpointId : viewpointId)
    }
    public func newViewpoint() throws -> BenchViewpoint? {
        guard usesFreshViewpoint else { return nil }
        return try BenchViewpoint(id: freshViewpointId, name: viewpointName, roomId: roomId,
                                 position: [number(viewX), number(viewY), number(viewZ)],
                                 target: [number(targetX), number(targetY), number(targetZ)], fov: number(fov))
    }
    public mutating func applyMaterialPreset(_ preset: InstallationSettings.Preset) {
        let material = InstallationSettings.Material.defaults(preset)
        materialPreset = preset; color = material.color; roughness = String(material.roughness); metalness = String(material.metalness)
    }
    /// Explicit author action. It creates a new camera intent and keeps every
    /// existing viewpoint untouched; collision/visibility still needs Viewer.
    public mutating func matchNewViewpointToBench() throws {
        var probe = self; probe.usesFreshViewpoint = true
        let bench = try probe.settings(), eye = [bench.position[0], bench.position[1] + bench.height + 0.75, bench.position[2]]
        let target = [eye[0] - sin(bench.yaw) * 2, eye[1], eye[2] - cos(bench.yaw) * 2]
        usesFreshViewpoint = true; viewpointName = name + " 감상"
        viewX = String(eye[0]); viewY = String(eye[1]); viewZ = String(eye[2])
        targetX = String(target[0]); targetY = String(target[1]); targetZ = String(target[2])
    }
}
