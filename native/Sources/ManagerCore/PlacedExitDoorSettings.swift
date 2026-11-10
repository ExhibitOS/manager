// SPDX-License-Identifier: Apache-2.0
import Foundation
import simd

public struct PlacedExitDoorSettings: Codable, Equatable, Sendable {
    public struct Destination: Codable, Equatable, Sendable {
        public let name: String
        public let url: String

        public init(name: String, url: String) throws {
            let label = name.trimmingCharacters(in: .whitespacesAndNewlines)
            let invalidName = CharacterSet.controlCharacters.union(CharacterSet(charactersIn: "\u{2028}\u{2029}"))
            let invalidURL = CharacterSet.whitespacesAndNewlines.union(.controlCharacters).union(CharacterSet(charactersIn: "\\"))
            guard !label.isEmpty, label.utf16.count <= 96, label.rangeOfCharacter(from: invalidName) == nil,
                  url.utf16.count <= 2048, url.rangeOfCharacter(from: invalidURL) == nil,
                  url.range(of: #"^https://[^/?#@]+(?:[/?#]|$)"#, options: [.regularExpression, .caseInsensitive]) != nil,
                  var components = URLComponents(string: url), components.scheme?.lowercased() == "https",
                  let host = components.host, !host.isEmpty, components.user == nil, components.password == nil,
                  components.port.map({ (0...65_535).contains($0) }) ?? true else { throw PlacedExitDoorSettings.invalid() }
            components.scheme = "https"
            components.host = host.lowercased()
            if components.port == 443 { components.port = nil }
            if components.path.isEmpty { components.path = "/" }
            guard let canonical = components.url?.standardized.absoluteString,
                  canonical.utf16.count <= 2048 else { throw PlacedExitDoorSettings.invalid() }
            self.name = label
            self.url = canonical
        }
    }

    public struct Target: Identifiable, Equatable, Sendable {
        public let id: String
        public let roomId: String
        public let furnishingId: String
        public let wallIds: [String]
    }

    public let placementId: String
    public let surfaceId: String
    public let primary: Destination
    public let additional: [Destination]
    static let modelSHA256 = "e4f9a701214e980dacf127a009847e4fd0c46ca98f53e880e03ce2b9d4c55e25"
    private static let keys: Set<String> = ["placementId", "surfaceId", "primary", "additional"]
    private static let requiredExperience = ["footsteps", "voices", "annotations", "rooms", "translations"]

    public init(placementId: String, surfaceId: String, primary: Destination, additional: [Destination]) throws {
        guard workspaceFurnishingID(placementId), workspaceFurnishingID(surfaceId), additional.count <= 4 else { throw Self.invalid() }
        let values = try ([primary] + additional).map { try Destination(name: $0.name, url: $0.url) }
        guard Set(values.map(\.url)).count == values.count else { throw Self.invalid() }
        self.placementId = placementId
        self.surfaceId = surfaceId
        self.primary = values[0]
        self.additional = Array(values.dropFirst())
    }

    init(value: DocumentValue) throws {
        guard case .object(let row) = value, Set(row.keys) == Self.keys,
              let placement = value["placementId"].string, let surface = value["surfaceId"].string,
              let rest = value["additional"].items else { throw Self.invalid() }
        func destination(_ value: DocumentValue) throws -> Destination {
            guard case .object(let fields) = value, Set(fields.keys) == ["name", "url"],
                  let name = value["name"].string, let url = value["url"].string else { throw Self.invalid() }
            return try Destination(name: name, url: url)
        }
        try self.init(placementId: placement, surfaceId: surface,
                      primary: destination(value["primary"]), additional: rest.map(destination))
    }

    public static func targets(in draft: NativeExhibitionDraft) -> [Target] {
        let workspace = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]
        let assets = workspace["furnishings"].items ?? []
        let walls = (draft.candidate["surfaces"].items ?? []).filter { $0["type"] == .string("wall") }
        return (workspace["placements"].items ?? []).compactMap { placement in
            guard let id = placement["id"].string, let room = placement["roomId"].string,
                  let furnishing = placement["furnishingId"].string,
                  [id, room, furnishing].allSatisfy(workspaceFurnishingID), placement["furnishingVersion"] == .integer(1),
                  draft.rooms.contains(where: { $0["id"] == .string(room) }) else { return nil }
            let matches = assets.filter { $0["id"] == .string(furnishing) && $0["version"] == .integer(1) }
            guard matches.count == 1, matches[0]["kind"] == .string("barrier"),
                  matches[0]["model"]["sha256"] == .string(modelSHA256), matches[0]["model"]["bytes"] == .integer(8684),
                  matches[0]["model"]["mime"] == .string("model/gltf-binary") else { return nil }
            let wallIds = walls.filter { $0["roomId"] == .string(room) && anchors(placement, to: $0) }
                .compactMap { $0["id"].string }.filter(workspaceFurnishingID)
            guard !wallIds.isEmpty else { return nil }
            return Target(id: id, roomId: room, furnishingId: furnishing, wallIds: wallIds)
        }
    }

    public static func read(in draft: NativeExhibitionDraft, placementId: String) throws -> Self? {
        let rows = draft.candidate["extensions"][ConcreteFootsteps.experienceNamespace]["exitDoors"].items ?? []
        let matches = rows.filter { $0["placementId"] == .string(placementId) }
        guard matches.count <= 1 else { throw invalid() }
        return try matches.first.map(Self.init(value:))
    }

    func candidate(in draft: NativeExhibitionDraft) throws -> DocumentValue {
        guard let target = Self.targets(in: draft).first(where: { $0.id == placementId }),
              target.wallIds.contains(surfaceId) else { throw Self.invalid() }
        var candidate = draft.candidate
        var extensions = candidate["extensions"]
        if extensions == .null { extensions = .object([:]) }
        guard case .object = extensions else { throw Self.invalid() }
        let namespace = ConcreteFootsteps.experienceNamespace
        var experience = extensions[namespace]
        if experience == .null {
            var fields = Dictionary(uniqueKeysWithValues: Self.requiredExperience.map { ($0, DocumentValue.array([])) })
            fields["version"] = .integer(1)
            experience = .object(fields)
        }
        guard case .object(let fields) = experience, experience["version"] == .integer(1),
              Self.requiredExperience.allSatisfy({ fields[$0]?.items != nil }),
              fields["exitDoors"] == nil || experience["exitDoors"].items != nil else { throw Self.invalid() }
        var rows = experience["exitDoors"].items ?? []
        let ids = rows.compactMap { $0["placementId"].string?.lowercased() }
        guard rows.count <= 32, ids.count == rows.count, Set(ids).count == ids.count else { throw Self.invalid() }
        let row = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(self))
        if let index = ids.firstIndex(of: placementId.lowercased()) { rows[index] = row }
        else {
            guard rows.count < 32 else { throw Self.invalid() }
            rows.append(row)
        }
        experience["exitDoors"] = .array(rows)
        extensions[namespace] = experience
        candidate["extensions"] = extensions
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }

    private static func vector(_ value: DocumentValue) -> SIMD3<Double>? {
        guard let items = value.items, items.count == 3, let x = items[0].double, let y = items[1].double,
              let z = items[2].double, [x, y, z].allSatisfy(\.isFinite) else { return nil }
        return SIMD3(x, y, z)
    }

    private static func anchors(_ placement: DocumentValue, to wall: DocumentValue) -> Bool {
        let transform = wall["transform"]
        guard let position = vector(placement["position"]), let yaw = placement["yaw"].double, yaw.isFinite,
              let origin = vector(transform["position"]), [origin.x, origin.y, origin.z].allSatisfy({ abs($0) <= 10_000 }),
              let scale = vector(transform["scale"]), [scale.x, scale.y, scale.z].allSatisfy({ (1e-6...100).contains($0) }),
              let raw = transform["rotation"].items, raw.count == 4,
              let x = raw[0].double, let y = raw[1].double, let z = raw[2].double, let w = raw[3].double,
              [x, y, z, w].allSatisfy(\.isFinite), let width = wall["dimensions"]["width"].double,
              let height = wall["dimensions"]["height"].double, width > 0, height > 0 else { return false }
        let rotation = simd_quatd(ix: x, iy: y, iz: z, r: w)
        guard abs(rotation.length - 1) <= 1e-6 else { return false }
        let local = rotation.inverse.act(position - origin)
        let normal = rotation.act(SIMD3(0, 0, 1))
        let facing = normal.x * sin(yaw) + normal.z * cos(yaw)
        return abs(local.z) <= 0.08 && facing >= 0.98 && abs(local.x) <= width * scale.x / 2
            && abs(local.y) <= height * scale.y / 2 + 0.01
    }

    private static func invalid() -> AuthoringFailure { AuthoringFailure("PLACED_EXIT_DOOR_INVALID") }
}
