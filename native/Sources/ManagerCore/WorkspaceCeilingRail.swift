// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A passive visual attachment; no collision, support or electrical behavior is implied.
public struct WorkspaceCeilingRailAttachment: Codable, Equatable, Sendable {
    public let placementId: String, surfaceId: String
    public let anchor: [Double], yaw: Double, gap: Double
    public init(placementId: String, surfaceId: String, anchor: [Double], yaw: Double, gap: Double) {
        self.placementId = placementId; self.surfaceId = surfaceId; self.anchor = anchor; self.yaw = yaw; self.gap = gap
    }
}

public enum WorkspaceCeilingRail {
    private static let ns = WorkspaceFurnishingPlacement.namespace
    private static let eps = 0.000001
    private static func invalid() -> AuthoringFailure { AuthoringFailure("CEILING_RAIL_INVALID") }
    private static func fields(_ value: DocumentValue, _ keys: Set<String>) -> Bool {
        guard case .object(let object) = value else { return false }; return Set(object.keys) == keys
    }
    private static func vector(_ value: DocumentValue, _ count: Int) -> [Double]? {
        guard let rows = value.items, rows.count == count else { return nil }
        let numbers = rows.compactMap(\.double)
        return numbers.count == count && numbers.allSatisfy(\.isFinite) ? numbers : nil
    }
    private static func same(_ a: String?, _ b: String?) -> Bool {
        guard let a, let b else { return false }; return a.lowercased() == b.lowercased()
    }
    private static func close(_ a: Double, _ b: Double) -> Bool { a.isFinite && b.isFinite && abs(a-b) <= eps }
    private static func multiply(_ a: [[Double]], _ b: [[Double]]) -> [[Double]] {
        (0..<4).map { r in (0..<4).map { c in (0..<4).reduce(0.0) { $0 + a[r][$1] * b[$1][c] } } }
    }
    private static func point(_ m: [[Double]], _ p: [Double]) -> [Double] {
        (0..<3).map { m[$0][0]*p[0] + m[$0][1]*p[1] + m[$0][2]*p[2] + m[$0][3] }
    }
    private static func pose(_ transform: DocumentValue, level: Bool = false) throws -> [[Double]] {
        guard let p = vector(transform["position"], 3), let q = vector(transform["rotation"], 4),
              let scale = vector(transform["scale"], 3), scale.allSatisfy({ $0 == 1 }),
              abs(q.reduce(0, { $0 + $1*$1 }) - 1) <= eps,
              !level || (abs(q[0]) <= eps && abs(q[2]) <= eps) else { throw invalid() }
        let x=q[0], y=q[1], z=q[2], w=q[3]
        return [[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w), p[0]],
                [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w), p[1]],
                [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y), p[2]], [0,0,0,1]]
    }
    private static func local(_ a: WorkspaceCeilingRailAttachment) -> [[Double]] {
        let c=cos(a.yaw), s=sin(a.yaw)
        let t: [[Double]] = [[1,0,0,a.anchor[0]],[0,1,0,a.anchor[1]],[0,0,1,a.gap],[0,0,0,1]]
        let rz: [[Double]] = [[c,-s,0,0],[s,c,0,0],[0,0,1,0],[0,0,0,1]]
        let rx: [[Double]] = [[1,0,0,0],[0,0,1,0],[0,-1,0,0],[0,0,0,1]]
        return multiply(multiply(t, rz), rx)
    }
    private static func checkedAttachment(_ value: DocumentValue) throws -> WorkspaceCeilingRailAttachment {
        guard fields(value, ["placementId", "surfaceId", "anchor", "yaw", "gap"]),
              let placement = value["placementId"].string, workspaceFurnishingID(placement),
              let surface = value["surfaceId"].string, workspaceFurnishingID(surface),
              let anchor = vector(value["anchor"], 2), anchor.allSatisfy({ abs($0) <= 10_000 }),
              let yaw = value["yaw"].double, yaw.isFinite, yaw >= -Double.pi, yaw < Double.pi,
              let gap = value["gap"].double, gap.isFinite, (0...0.1).contains(gap) else { throw invalid() }
        return .init(placementId: placement, surfaceId: surface, anchor: anchor, yaw: yaw, gap: gap)
    }
    private static func dimensions(_ value: DocumentValue, _ keys: [String]) throws -> [Double] {
        let result = keys.compactMap { value[$0].double }
        guard result.count == keys.count, result.allSatisfy({ $0.isFinite && $0 > 0 }) else { throw invalid() }
        return result
    }
    /// Checks every v11 rail binding against measured source bounds and the actual ceiling.
    public static func validate(candidate: DocumentValue) throws {
        let ext = candidate["extensions"][ns]
        guard fields(ext, ["version", "furnishings", "placements", "frameAttachments", "openingAttachments", "ceilingAttachments"]),
              ext["version"] == .integer(11), let descriptors = ext["furnishings"].items, !descriptors.isEmpty, descriptors.count <= 64,
              let placements = ext["placements"].items, placements.count <= 128,
              let attachments = ext["ceilingAttachments"].items, attachments.count <= 128,
              let frames = ext["frameAttachments"].items, frames.count <= 128,
              let openings = ext["openingAttachments"].items, openings.count <= 128,
              let surfaces = candidate["surfaces"].items, let rooms = candidate["rooms"].items else { throw invalid() }
        var occupied = Set<String>()
        for name in ["rooms", "surfaces", "openings", "placements", "lights", "mediaAssets", "artworks"] {
            for item in candidate[name].items ?? [] {
                for key in [item["id"], item["revisionId"]] + (item["assets"].items ?? []).map({ $0["id"] }) {
                    if let id = key.string { occupied.insert(id.lowercased()) }
                }
            }
        }
        let extensions = candidate["extensions"]
        if let id = extensions["org.exhibitos.viewer/space-still"]["asset"]["id"].string {
            occupied.insert(id.lowercased())
        }
        for (namespace, collection) in [("org.exhibitos.studio/furnishing-visuals", "assets"),
                                        ("org.exhibitos.studio/benches", "benches"),
                                        ("org.exhibitos.studio/workspace-materials", "materials")] {
            for item in extensions[namespace][collection].items ?? [] {
                if let id = item["id"].string { occupied.insert(id.lowercased()) }
            }
        }
        var ids = Set<String>(), railIDs = Set<String>()
        for descriptor in descriptors {
            guard let id = descriptor["id"].string, workspaceFurnishingID(id), !occupied.contains(id), ids.insert(id).inserted else { throw invalid() }
            if descriptor["kind"] == .string("ceiling-rail") {
                guard fields(descriptor, ["id", "version", "kind", "name", "rights", "provenance", "model", "fit"]),
                      fields(descriptor["model"], ["mime", "bytes", "sha256"]) else { throw invalid() }
                let item = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
                guard item.fit?.profile == .passiveCeilingRail else { throw invalid() }
                railIDs.insert(id)
            }
        }
        var placementIDs = Set<String>()
        for placement in placements {
            guard let id = placement["id"].string, workspaceFurnishingID(id), !occupied.contains(id),
                  !ids.contains(id), placementIDs.insert(id).inserted,
                  let furnishing = placement["furnishingId"].string, ids.contains(furnishing),
                  placement["furnishingVersion"] == .integer(1) else { throw invalid() }
        }
        let rails = placements.filter { $0["furnishingId"].string.map(railIDs.contains) == true }
        guard Set(rails.compactMap { $0["furnishingId"].string }) == railIDs else { throw invalid() }
        var seen = Set<String>()
        for raw in attachments {
            let a = try checkedAttachment(raw)
            let matched = placements.filter { same($0["id"].string, a.placementId) }
            guard seen.insert(a.placementId).inserted, matched.count == 1,
                  fields(matched[0], ["id", "roomId", "furnishingId", "furnishingVersion", "position", "yaw", "scale"]),
                  let descriptor = descriptors.first(where: { $0["id"] == matched[0]["furnishingId"] }),
                  descriptor["kind"] == .string("ceiling-rail"), matched[0]["furnishingVersion"] == .integer(1),
                  matched[0]["scale"].double == 1,
                  let position = vector(matched[0]["position"], 3),
                  let yaw = matched[0]["yaw"].double, yaw.isFinite, yaw >= -Double.pi, yaw < Double.pi else { throw invalid() }
            let selected = surfaces.filter { same($0["id"].string, a.surfaceId) }
            guard selected.count == 1, selected[0]["type"] == .string("ceiling") else { throw invalid() }
            let roomRows = rooms.filter { same($0["id"].string, selected[0]["roomId"].string) }
            guard roomRows.count == 1, same(matched[0]["roomId"].string, roomRows[0]["id"].string) else { throw invalid() }
            let room = roomRows[0], surface = selected[0]
            let rd = try dimensions(room["dimensions"], ["width", "height", "depth"])
            let sd = try dimensions(surface["dimensions"], ["width", "height"])
            _ = try pose(room["transform"], level: true)
            let surfaceMatrix = try pose(surface["transform"])
            guard close(surfaceMatrix[0][2], 0), close(surfaceMatrix[1][2], -1), close(surfaceMatrix[2][2], 0) else { throw invalid() }
            for x in [-1.0, 1.0] { for y in [-1.0, 1.0] {
                guard close(point(surfaceMatrix, [x*sd[0]/2, y*sd[1]/2, 0])[1], rd[1]) else { throw invalid() }
            } }
            let geometry = descriptor["fit"]["geometry"]
            guard let lo = vector(geometry["min"], 3), let hi = vector(geometry["max"], 3) else { throw invalid() }
            let localMatrix = local(a), matrix = multiply(surfaceMatrix, localMatrix), c=cos(yaw), s=sin(yaw)
            let stored: [[Double]] = [[c,0,s,position[0]],[0,1,0,position[1]],[-s,0,c,position[2]],[0,0,0,1]]
            guard (0..<4).allSatisfy({ r in (0..<4).allSatisfy({ close(matrix[r][$0], stored[r][$0]) }) }) else { throw invalid() }
            for x in [lo[0],hi[0]] { for y in [lo[1],hi[1]] { for z in [lo[2],hi[2]] {
                let ceiling = point(localMatrix,[x,y,z]), placed = point(matrix,[x,y,z])
                guard (ceiling + placed).allSatisfy(\.isFinite), abs(ceiling[0]) <= sd[0]/2+eps,
                      abs(ceiling[1]) <= sd[1]/2+eps, abs(placed[0]) <= rd[0]/2+eps,
                      placed[1] >= -eps, placed[1] <= rd[1]+eps, abs(placed[2]) <= rd[2]/2+eps else { throw invalid() }
            } } }
        }
        guard rails.count == seen.count, rails.allSatisfy({ $0["id"].string.map(seen.contains) == true }) else { throw invalid() }
    }

    /// Produces one derived unit-scale placement and attachment in the existing draft.
    public static func applying(_ attachment: WorkspaceCeilingRailAttachment, furnishing: WorkspaceFurnishing,
                                to draft: NativeExhibitionDraft, supportedExtensionVersion: Int64) throws -> NativeExhibitionDraft? {
        guard supportedExtensionVersion >= 11, furnishing.details.kind == .ceilingRail,
              furnishing.fit?.profile == .passiveCeilingRail else { throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE") }
        let encoded = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(attachment))
        let a = try checkedAttachment(encoded)
        let surfaces = draft.candidate["surfaces"].items?.filter { same($0["id"].string, a.surfaceId) } ?? []
        guard surfaces.count == 1, let roomId = surfaces[0]["roomId"].string else { throw invalid() }
        let matrix = multiply(try pose(surfaces[0]["transform"]), local(a))
        var yaw = atan2(-matrix[2][0], matrix[0][0]); if yaw >= Double.pi { yaw -= 2*Double.pi }
        if yaw == 0 { yaw = 0 }
        let placement: DocumentValue = .object(["id": .string(a.placementId), "roomId": .string(roomId),
            "furnishingId": .string(furnishing.id), "furnishingVersion": .integer(1),
            "position": .vector((0..<3).map { matrix[$0][3] == 0 ? 0 : matrix[$0][3] }), "yaw": .number(yaw), "scale": .integer(1)])
        let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(furnishing))
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        guard case .object = candidate["extensions"] else { throw invalid() }
        let old = candidate["extensions"][ns]
        var descriptors: [DocumentValue] = [], placements: [DocumentValue] = [], frames: [DocumentValue] = [], openings: [DocumentValue] = [], ceilings: [DocumentValue] = []
        if old != .null {
            guard let version = old["version"].integer, (1...7).contains(version) || [9,10,11].contains(version),
                  let values = old["furnishings"].items, let rows = old["placements"].items,
                  version < 4 || old["frameAttachments"].items != nil,
                  version < 6 || old["openingAttachments"].items != nil,
                  version != 11 || old["ceilingAttachments"].items != nil else { throw invalid() }
            var expected: Set<String> = ["version", "furnishings", "placements"]
            if version >= 4 { expected.insert("frameAttachments") }; if version >= 6 { expected.insert("openingAttachments") }
            if version == 11 { expected.insert("ceilingAttachments"); try validate(candidate: candidate) }
            guard fields(old, expected) else { throw invalid() }
            descriptors = values; placements = rows; frames = old["frameAttachments"].items ?? []
            openings = old["openingAttachments"].items ?? []; ceilings = old["ceilingAttachments"].items ?? []
        }
        let matching = descriptors.filter { $0["id"] == .string(furnishing.id) }
        if matching == [descriptor], placements.filter({ $0["id"] == .string(a.placementId) }) == [placement],
           ceilings.filter({ $0["placementId"] == .string(a.placementId) }) == [encoded] { return nil }
        guard matching.isEmpty || matching == [descriptor], !placements.contains(where: { $0["id"] == .string(a.placementId) }),
              !ceilings.contains(where: { $0["placementId"] == .string(a.placementId) }),
              placements.count < 128, ceilings.count < 128, !matching.isEmpty || descriptors.count < 64 else { throw invalid() }
        if matching.isEmpty { descriptors.append(descriptor) }; placements.append(placement); ceilings.append(encoded)
        candidate["extensions"][ns] = .object(["version": .integer(11), "furnishings": .array(descriptors),
            "placements": .array(placements), "frameAttachments": .array(frames), "openingAttachments": .array(openings), "ceilingAttachments": .array(ceilings)])
        try validate(candidate: candidate)
        var result = draft; try result.replaceCandidate(candidate); return result
    }
}
