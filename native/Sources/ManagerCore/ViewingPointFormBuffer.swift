// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Raw native input for the existing Presentation viewpoint shape. This appends
/// metadata only; shared frame admission and visitor safety remain authoritative.
public struct ViewingPointFormBuffer: Equatable, Sendable {
    public let id: String
    public var roomId: String
    public var name = "새 감상 시점"
    public var x = "0.0", y = "1.65", z: String
    public var targetX = "0.0", targetY = "1.65", targetZ = "-2.0"
    public var fov = "50.0"

    public init(draft: NativeExhibitionDraft, roomId: String) throws {
        guard let room = draft.rooms.first(where: { $0["id"].string == roomId }),
              let depth = room["dimensions"]["depth"].double, depth.isFinite, depth > 0 else { throw BenchSettings.invalid() }
        id = UUID().uuidString.lowercased(); self.roomId = roomId
        z = String(min(1, depth / 4)); targetZ = String(-min(2, depth / 4))
    }

    public func candidate(in draft: NativeExhibitionDraft) throws -> DocumentValue {
        let current = try BenchViewpoint.list(in: draft)
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, current.count < 64, !current.contains(where: { $0.id.lowercased() == id.lowercased() }),
              let room = draft.rooms.first(where: { $0["id"].string == roomId }) else { throw BenchSettings.invalid() }
        func number(_ text: String) throws -> Double {
            guard let value = Double(text.trimmingCharacters(in: .whitespacesAndNewlines)), value.isFinite else { throw BenchSettings.invalid() }
            return value
        }
        let view = try BenchViewpoint(id: id, name: name, roomId: roomId,
            position: [number(x), number(y), number(z)],
            target: [number(targetX), number(targetY), number(targetZ)], fov: number(fov))
        guard let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double,
              let depth = room["dimensions"]["depth"].double,
              [width, height, depth].allSatisfy({ $0.isFinite && $0 > 0 }),
              abs(view.position[0]) <= width / 2, (0...height).contains(view.position[1]),
              abs(view.position[2]) <= depth / 2 else { throw BenchSettings.invalid() }
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        var presentation = try BenchSettings.extensionValue(BenchSettings.presentationNamespace, in: draft)
            ?? .object(["version": .integer(1), "viewpoints": .array([]), "credits": .string("")])
        guard var rows = presentation["viewpoints"].items else { throw BenchSettings.invalid() }
        rows.append(view.value); presentation["viewpoints"] = .array(rows)
        guard try JSONEncoder().encode(presentation).count <= 32_768 else { throw BenchSettings.invalid() }
        candidate["extensions"][BenchSettings.presentationNamespace] = presentation
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
}
