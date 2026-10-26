// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Metadata adapter only. The existing shared validator admits complete plane,
/// room/world transforms and Float32 geometry before native Preview/Apply.
public struct PlaqueSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/plaques"
    public enum Source: Equatable, Sendable {
        case exhibition, room, artwork(placementId: String)
        public var value: DocumentValue {
            switch self {
            case .exhibition: .object(["kind": .string("exhibition")])
            case .room: .object(["kind": .string("room")])
            case .artwork(let id): .object(["kind": .string("artwork"), "placementId": .string(id)])
            }
        }
        init(value: DocumentValue) throws {
            guard case .object(let fields) = value else { throw PlaqueSettings.invalid() }
            switch value["kind"].string {
            case "exhibition" where Set(fields.keys) == ["kind"]: self = .exhibition
            case "room" where Set(fields.keys) == ["kind"]: self = .room
            case "artwork" where Set(fields.keys) == ["kind", "placementId"]:
                guard let id = value["placementId"].string, BenchSettings.uuid(id) else { throw PlaqueSettings.invalid() }
                self = .artwork(placementId: id)
            default: throw PlaqueSettings.invalid()
            }
        }
    }
    public let id: String, roomId: String
    public let source: Source
    public let position: [Double], rotation: [Double] // Unit quaternion; plane front +Z.
    public let width: Double, height: Double
    public let visible: Bool
    public init(id: String, roomId: String, source: Source, position: [Double], rotation: [Double], width: Double, height: Double, visible: Bool) throws {
        guard BenchSettings.uuid(id), BenchSettings.uuid(roomId), BenchSettings.point(position),
              rotation.count == 4, rotation.allSatisfy(\.isFinite),
              abs(sqrt(rotation.reduce(0) { $0 + $1 * $1 }) - 1) <= 1e-6,
              BenchSettings.bounded(width, 0.2...2), BenchSettings.bounded(height, 0.15...1.2) else { throw Self.invalid() }
        if case .artwork(let id) = source, !BenchSettings.uuid(id) { throw Self.invalid() }
        self.id = id; self.roomId = roomId; self.source = source; self.position = position; self.rotation = rotation
        self.width = width; self.height = height; self.visible = visible
    }
    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == ["id", "roomId", "source", "position", "rotation", "dimensions", "visible"],
              let id = value["id"].string, let room = value["roomId"].string,
              let position = BenchSettings.numbers(value["position"], count: 3), let rotation = BenchSettings.numbers(value["rotation"], count: 4),
              case .object(let size) = value["dimensions"], Set(size.keys) == ["width", "height"],
              let width = size["width"]?.double, let height = size["height"]?.double, case .bool(let visible) = value["visible"] else { throw Self.invalid() }
        try self.init(id: id, roomId: room, source: Source(value: value["source"]), position: position, rotation: rotation, width: width, height: height, visible: visible)
    }
    public var value: DocumentValue {
        .object(["id": .string(id), "roomId": .string(roomId), "source": source.value,
                 "position": .vector(position), "rotation": .vector(rotation),
                 "dimensions": .object(["width": .number(width), "height": .number(height)]), "visible": .bool(visible)])
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("PLAQUE_FIELDS_INVALID") }
    public static func list(in draft: NativeExhibitionDraft) throws -> [Self] {
        guard let raw = try BenchSettings.extensionValue(namespace, in: draft) else { return [] }
        guard case .object(let fields) = raw, Set(fields.keys) == ["version", "plaques"], raw["version"] == .integer(1),
              let rows = raw["plaques"].items, rows.count <= 32, try JSONEncoder().encode(raw).count <= 16_384 else { throw invalid() }
        var seen = Set<String>()
        return try rows.map { row in
            let plaque = try Self(value: row)
            guard seen.insert(plaque.id.lowercased()).inserted,
                  draft.rooms.contains(where: { $0["id"].string?.lowercased() == plaque.roomId.lowercased() }) else { throw invalid() }
            if case .artwork(let id) = plaque.source {
                guard draft.placements.contains(where: { $0["id"].string?.lowercased() == id.lowercased() && $0["roomId"].string?.lowercased() == plaque.roomId.lowercased() }) else { throw invalid() }
            }
            return plaque
        }
    }
}

/// No text body is stored here. Existing exhibition/room/artwork metadata is
/// referenced, while every raw pose/size input remains in the model buffer.
public struct PlaqueFormBuffer: Equatable, Sendable {
    public enum Facing: String, CaseIterable, Sendable { case forward, back, left, right }
    public enum SourceKind: String, CaseIterable, Sendable { case exhibition, room, artwork }
    public let id: String
    public var roomId: String
    public var sourceKind: SourceKind = .exhibition
    public var placementId = ""
    public var x = "0.0", y = "1.5", z = "0.0"
    public var rotationX = "0.0", rotationY = "0.0", rotationZ = "0.0", rotationW = "1.0"
    public var width = "0.8", height = "0.4"
    public var visible = true
    public init(roomId: String) throws {
        guard BenchSettings.uuid(roomId) else { throw PlaqueSettings.invalid() }
        id = UUID().uuidString.lowercased(); self.roomId = roomId
    }
    public init(draft: NativeExhibitionDraft, plaqueId: String) throws {
        guard let plaque = try PlaqueSettings.list(in: draft).first(where: { $0.id == plaqueId }) else { throw AuthoringFailure("PLAQUE_SELECTION_CHANGED") }
        id = plaque.id; roomId = plaque.roomId
        switch plaque.source {
        case .exhibition: sourceKind = .exhibition
        case .room: sourceKind = .room
        case .artwork(let id): sourceKind = .artwork; placementId = id
        }
        x = String(plaque.position[0]); y = String(plaque.position[1]); z = String(plaque.position[2])
        rotationX = String(plaque.rotation[0]); rotationY = String(plaque.rotation[1]); rotationZ = String(plaque.rotation[2]); rotationW = String(plaque.rotation[3])
        width = String(plaque.width); height = String(plaque.height); visible = plaque.visible
    }
    public func settings() throws -> PlaqueSettings {
        func number(_ text: String) throws -> Double {
            guard let value = Double(text.trimmingCharacters(in: .whitespacesAndNewlines)), value.isFinite else { throw PlaqueSettings.invalid() }; return value
        }
        let source: PlaqueSettings.Source
        switch sourceKind { case .exhibition: source = .exhibition; case .room: source = .room; case .artwork: source = .artwork(placementId: placementId) }
        return try PlaqueSettings(id: id, roomId: roomId, source: source, position: [number(x), number(y), number(z)],
                                 rotation: [number(rotationX), number(rotationY), number(rotationZ), number(rotationW)],
                                 width: number(width), height: number(height), visible: visible)
    }
    public mutating func face(_ direction: Facing) {
        rotationX = "0.0"; rotationZ = "0.0"
        switch direction {
        case .forward: rotationY = "0.0"; rotationW = "1.0"
        case .back: rotationY = "1.0"; rotationW = "0.0"
        case .left: rotationY = String(-sqrt(0.5)); rotationW = String(sqrt(0.5))
        case .right: rotationY = String(sqrt(0.5)); rotationW = String(sqrt(0.5))
        }
    }
}
