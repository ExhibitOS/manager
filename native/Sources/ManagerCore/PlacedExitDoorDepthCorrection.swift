// SPDX-License-Identifier: Apache-2.0
import Foundation
import simd

/// Move an explicitly selected existing door and passive sign along their
/// existing host's inward normal. This never adds or replaces a placement.
public struct PlacedExitDoorDepthCorrection: Sendable {
    public static let signModelSHA256 = "d8cf449146e1cc8b9174d1a550a44bc41735487ec1fe68a9deb363faba02e44c"
    public let door: WorkspaceFurnishingPlacement, sign: WorkspaceFurnishingPlacement
    public let depthOffset: Double
    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == ["door", "sign", "depthOffset"],
              let offset = value["depthOffset"].double, offset.isFinite, offset > 0, offset <= 0.08 else { throw Self.invalid() }
        func placement(_ value: DocumentValue) throws -> WorkspaceFurnishingPlacement {
            let result = try JSONDecoder().decode(WorkspaceFurnishingPlacement.self, from: JSONEncoder().encode(value))
            guard try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(result)) == value else { throw Self.invalid() }
            return result
        }
        door = try placement(value["door"]); sign = try placement(value["sign"]); depthOffset = offset
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("PLACED_EXIT_DOOR_DEPTH_INVALID") }
    func applying(to draft: NativeExhibitionDraft, registeredDoor: WorkspaceFurnishing, registeredSign: WorkspaceFurnishing) throws -> NativeExhibitionDraft {
        let namespace = WorkspaceFurnishingPlacement.namespace
        var ext = draft.candidate["extensions"][namespace]
        guard var rows = ext["placements"].items, let descriptors = ext["furnishings"].items,
              door.id != sign.id, door.furnishingId != sign.furnishingId, door.roomId == sign.roomId,
              door.furnishingVersion == 1, sign.furnishingVersion == 1,
              door.position.count == 3, sign.position.count == 3,
              door.position.allSatisfy(\.isFinite), sign.position.allSatisfy(\.isFinite),
              door.position[0] == sign.position[0], door.position[2] == sign.position[2], door.yaw == sign.yaw,
              registeredDoor.id == door.furnishingId, registeredDoor.version == 1,
              registeredDoor.details.kind == .barrier, registeredDoor.details.model.sha256 == PlacedExitDoorSettings.modelSHA256,
              registeredDoor.details.model.bytes == 8684, registeredDoor.details.rights.permissions.display,
              registeredSign.id == sign.furnishingId, registeredSign.version == 1,
              registeredSign.details.kind == .infoStand, registeredSign.details.model.sha256 == Self.signModelSHA256,
              registeredSign.details.model.bytes == 10596, registeredSign.details.rights.permissions.display,
              let settings = try PlacedExitDoorSettings.read(in: draft, placementId: door.id),
              let target = PlacedExitDoorSettings.targets(in: draft).first(where: { $0.id == door.id }),
              target.wallIds.contains(settings.surfaceId),
              let wall = draft.candidate["surfaces"].items?.first(where: { $0["id"] == .string(settings.surfaceId) }),
              let rotation = wall["transform"]["rotation"].items?.compactMap(\.double), rotation.count == 4 else { throw Self.invalid() }
        func value<T: Encodable>(_ item: T) throws -> DocumentValue { try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(item)) }
        for (placement, descriptor) in [(door, registeredDoor), (sign, registeredSign)] {
            guard rows.filter({ $0["id"] == .string(placement.id) }) == [try value(placement)],
                  descriptors.filter({ $0["id"] == .string(descriptor.id) }) == [try value(descriptor)] else { throw Self.invalid() }
        }
        let quaternion = simd_quatd(ix: rotation[0], iy: rotation[1], iz: rotation[2], r: rotation[3])
        let normal = quaternion.act(SIMD3<Double>(0, 0, 1))
        guard abs(quaternion.length - 1) <= 1e-6, abs(normal.y) <= 1e-6 else { throw Self.invalid() }
        for placement in [door, sign] {
            guard let index = rows.firstIndex(where: { $0["id"] == .string(placement.id) }) else { throw Self.invalid() }
            let next = [placement.position[0] + normal.x * depthOffset, placement.position[1], placement.position[2] + normal.z * depthOffset]
            guard let room = draft.rooms.first(where: { $0["id"] == .string(placement.roomId) }),
                  let width = room["dimensions"]["width"].double, let height = room["dimensions"]["height"].double,
                  let depth = room["dimensions"]["depth"].double,
                  abs(next[0]) <= width / 2, (0...height).contains(next[1]), abs(next[2]) <= depth / 2 else { throw Self.invalid() }
            rows[index]["position"] = .vector(next)
        }
        ext["placements"] = .array(rows)
        var candidate = draft.candidate; candidate["extensions"][namespace] = ext
        var result = draft; try result.replaceCandidate(candidate)
        // Reuse the existing door/wall relationship check; do not expand the
        // host's anchor tolerance or change its destination/link settings.
        guard try settings.candidate(in: result) == candidate else { throw Self.invalid() }
        return result
    }
}
