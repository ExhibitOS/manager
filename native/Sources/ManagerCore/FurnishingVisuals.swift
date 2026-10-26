// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Native adapter for the frozen furnishing-visuals-v1 contract. The shared
/// validator owns room/world geometry, collision, rights and asset admission.
public enum FurnishingCatalog {
    public static let namespace = "org.exhibitos.studio/furnishing-visuals"
    public static let id = "canted-display-pedestal"
    public static let sha256 = "7af772939927ea1a4015187ccadd4c6e378ae237c9b6650ecf83c4ef1e3cccc7"
    public static let title = "기울어진 전시 받침"
    public static let min = [-0.21938826143741608, 0, -0.22500000149011612]
    public static let max = [0.2590000033378601, 1.0479999464005232, 0.21965768933296204]
    public static let support = [0.052000001072883606, 1.0479999464005232, -0.017999999225139618]
    public static let rights: DocumentValue = .object([
        "licenseId": .string("CC0-1.0"),
        "creditLine": .string("Canted display pedestal — Innerscene 3D Library, CC0 1.0; creator not identified."),
        "source": .string("https://www.innerscene.com/tools/library/3d-parts/canted-display-pedestal-display-plinth-22ced290"),
        "licenseSource": .string("https://www.innerscene.com/tools/library/terms")])
    public static let descriptor: DocumentValue = .object([
        "id": .string(id), "version": .integer(1), "bytes": .integer(99_296), "sha256": .string(sha256), "triangles": .integer(788),
        "min": .vector(min), "max": .vector(max), "support": .vector(support), "supportSize": .vector([0.402, 0.402]), "rights": rights])
    public static func checkedCatalog(_ value: DocumentValue) throws -> Bool {
        guard case .object(let fields) = value, Set(fields.keys) == ["version", "items"], value["version"] == .integer(1),
              let items = value["items"].items, items.isEmpty || items == [descriptor] else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return !items.isEmpty
    }
    static func asset(_ id: String) throws -> DocumentValue {
        guard UUID(uuidString: id) != nil else { throw AuthoringFailure("FURNISHING_INVALID") }
        return .object(["id": .string(id), "catalogId": .string(Self.id), "catalogVersion": .integer(1), "mime": .string("model/gltf-binary"),
            "path": .string("furnishings/\(id)/model.glb"), "bytes": .integer(99_296), "sha256": .string(sha256), "rights": rights,
            "provenance": .object(["sourceSha256": .string(sha256), "recipe": .string("original-v1")])])
    }
    static func same(_ a: String?, _ b: String) -> Bool { a?.lowercased() == b.lowercased() }
    /// Reject unknown shapes/identities rather than normalizing persisted data.
    static func metadata(_ draft: NativeExhibitionDraft) throws -> (assets: [DocumentValue], bindings: [DocumentValue]) {
        let value = draft.candidate["extensions"][namespace]
        if value == .null {
            if case .object(let fields) = draft.candidate["extensions"], fields[namespace] != nil { throw AuthoringFailure("FURNISHING_INVALID") }
            return ([], [])
        }
        guard case .object(let fields) = value, Set(fields.keys) == ["version", "assets", "bindings"], value["version"] == .integer(1),
              let assets = value["assets"].items, assets.count == 1, let id = assets[0]["id"].string, assets[0] == (try asset(id)),
              let bindings = value["bindings"].items, (1...32).contains(bindings.count), try JSONEncoder().encode(value).count <= 32_768 else { throw AuthoringFailure("FURNISHING_INVALID") }
        var used = Set<String>()
        for binding in bindings {
            guard case .object(let row) = binding, Set(row.keys) == ["hostKind", "hostId", "assetId", "pose"], binding["hostKind"] == .string("pedestal"),
                  let host = binding["hostId"].string, UUID(uuidString: host) != nil, used.insert(host.lowercased()).inserted,
                  same(binding["assetId"].string, id), draft.placements.contains(where: { same($0["id"].string, host) }) else { throw AuthoringFailure("FURNISHING_INVALID") }
            _ = try FurnishingPose(binding["pose"])
        }
        try checkAssetIdentity(id, in: draft)
        return (assets, bindings)
    }
    static func checkAssetIdentity(_ id: String, in draft: NativeExhibitionDraft) throws {
        let c = draft.candidate
        let entities = ["rooms", "surfaces", "openings", "artworks", "placements", "lights", "mediaAssets"].flatMap { c[$0].items ?? [] }
        let assets = draft.artworks.flatMap { $0["assets"].items ?? [] }
        guard !(entities + assets).contains(where: { same($0["id"].string, id) || same($0["revisionId"].string, id) }) else { throw AuthoringFailure("FURNISHING_INVALID") }
    }
    public static func hasVisual(in draft: NativeExhibitionDraft, hostId: String) -> Bool {
        // Even unknown metadata locks the legacy editor; never overwrite it.
        guard draft.candidate["extensions"][namespace] != .null else { return false }
        guard let metadata = try? metadata(draft) else { return true }
        return metadata.bindings.contains(where: { same($0["hostId"].string, hostId) })
    }
}

public struct FurnishingPose: Equatable, Sendable {
    public let position: [Double], yaw: Double, scale: Double
    public init(position: [Double], yaw: Double, scale: Double) throws {
        guard position.count == 3, position.allSatisfy({ $0.isFinite && (-10_000...10_000).contains($0) }), yaw.isFinite, (-Double.pi...Double.pi).contains(yaw), scale.isFinite, (0.1...4).contains(scale) else { throw AuthoringFailure("FURNISHING_FIELDS_INVALID") }
        self.position = position; self.yaw = yaw; self.scale = scale
    }
    init(_ value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == ["position", "yaw", "scale"], let p = value["position"].items,
              p.count == 3, p.allSatisfy({ $0.double != nil }), let yaw = value["yaw"].double, let scale = value["scale"].double else { throw AuthoringFailure("FURNISHING_INVALID") }
        try self.init(position: p.compactMap(\.double), yaw: yaw, scale: scale)
    }
    public var value: DocumentValue { .object(["position": .vector(position), "yaw": .number(yaw), "scale": .number(scale)]) }
    public var dimensions: [Double] { [(FurnishingCatalog.max[0]-FurnishingCatalog.min[0])*scale, FurnishingCatalog.max[1]*scale, (FurnishingCatalog.max[2]-FurnishingCatalog.min[2])*scale] }
    public func point(_ point: [Double]) -> [Double] {
        let x = point[0]*scale, y = point[1]*scale, z = point[2]*scale
        return [position[0]+cos(yaw)*x+sin(yaw)*z, position[1]+y, position[2]-sin(yaw)*x+cos(yaw)*z]
    }
    var pedestal: DocumentValue {
        let center = point([(FurnishingCatalog.min[0]+FurnishingCatalog.max[0])/2, 0, (FurnishingCatalog.min[2]+FurnishingCatalog.max[2])/2])
        return .object(["kind": .string("box"), "position": .vector(center), "yaw": .number(yaw),
            "dimensions": .object(["width": .number(dimensions[0]), "height": .number(dimensions[1]), "depth": .number(dimensions[2])]),
            "material": .object(["preset": .string("paint"), "color": .string("#141818"), "roughness": .number(0.61), "metalness": .integer(0)])])
    }
}

/// One model-owned raw buffer for both modes. Opening/browsing changes no draft.
public struct FurnishingFormBuffer: Equatable, Sendable {
    public var selected: Bool
    public var x: String, y: String, z: String, yawDegrees: String, scale: String
    private let initialPose: FurnishingPose?
    private let initialYaw: String
    public init(draft: NativeExhibitionDraft, hostId: String) throws {
        guard draft.placements.contains(where: { FurnishingCatalog.same($0["id"].string, hostId) }) else { throw AuthoringFailure("FURNISHING_INVALID") }
        let metadata = try FurnishingCatalog.metadata(draft)
        let binding = metadata.bindings.first(where: { FurnishingCatalog.same($0["hostId"].string, hostId) })
        let pose = try binding.map { try FurnishingPose($0["pose"]) }
        initialPose = pose; selected = pose != nil
        // New visual positions require explicit input; no guessed artwork anchor.
        x = pose.map { String($0.position[0]) } ?? ""; y = pose.map { String($0.position[1]) } ?? ""; z = pose.map { String($0.position[2]) } ?? ""
        yawDegrees = pose.map { String($0.yaw*180 / .pi) } ?? "0"; scale = pose.map { String($0.scale) } ?? "1"
        initialYaw = yawDegrees
    }
    public func command(hostId: String, locale: Locale = .current) throws -> FurnishingCommand {
        if !selected { return try FurnishingCommand(hostId: hostId, pose: nil) }
        func number(_ raw: String) throws -> Double {
            let text = raw.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: locale.decimalSeparator ?? ".", with: ".")
            guard let value = Double(text), value.isFinite else { throw AuthoringFailure("FURNISHING_FIELDS_INVALID") }
            return value
        }
        let yaw = yawDegrees == initialYaw ? (initialPose?.yaw ?? 0) : try number(yawDegrees)*Double.pi/180
        return try FurnishingCommand(hostId: hostId, pose: FurnishingPose(position: [number(x), number(y), number(z)], yaw: yaw, scale: number(scale)))
    }
}

public struct FurnishingCommand: Equatable, Sendable {
    public let hostId: String
    public let pose: FurnishingPose?
    public init(hostId: String, pose: FurnishingPose?) throws {
        guard UUID(uuidString: hostId) != nil else { throw AuthoringFailure("FURNISHING_INVALID") }
        self.hostId = hostId; self.pose = pose
    }
    public var value: DocumentValue {
        var fields: [String: DocumentValue] = ["type": .string(pose == nil ? "remove-furnishing-visual" : "set-furnishing-visual"), "hostKind": .string("pedestal"), "hostId": .string(hostId)]
        if let pose { fields["catalogId"] = .string(FurnishingCatalog.id); fields["catalogVersion"] = .integer(1); fields["pose"] = pose.value }
        return .object(fields)
    }
    /// Predict authorized fields using ONLY the shared minted/reused asset ID.
    /// This is reply correspondence, never an alternative full admission path.
    func expected(in draft: NativeExhibitionDraft, newAssetId: String?) throws -> DocumentValue {
        var metadata = try FurnishingCatalog.metadata(draft), candidate = draft.candidate
        guard draft.placements.contains(where: { FurnishingCatalog.same($0["id"].string, hostId) }) else { throw AuthoringFailure("FURNISHING_INVALID") }
        var extensions: [String: DocumentValue]
        if case .object(let fields) = candidate["extensions"] { extensions = fields }
        else if candidate["extensions"] == .null { extensions = [:] }
        else { throw AuthoringFailure("FURNISHING_INVALID") }
        let index = metadata.bindings.firstIndex(where: { FurnishingCatalog.same($0["hostId"].string, hostId) })
        if let pose {
            if metadata.assets.isEmpty {
                guard let newAssetId else { throw AuthoringFailure("FURNISHING_INVALID") }
                try FurnishingCatalog.checkAssetIdentity(newAssetId, in: draft)
                metadata.assets = [try FurnishingCatalog.asset(newAssetId)]
            }
            let binding: DocumentValue = .object(["hostKind": .string("pedestal"), "hostId": .string(hostId), "assetId": metadata.assets[0]["id"], "pose": pose.value])
            if let index { metadata.bindings[index] = binding } else { metadata.bindings.append(binding) }
            let installation = InstallationSettings.namespace
            var rows = extensions[installation]?["placements"].items ?? []
            // Existing closed installation reader must accept the untouched base.
            _ = try draft.installationSettings(for: hostId)
            if let at = rows.firstIndex(where: { FurnishingCatalog.same($0["placementId"].string, hostId) }) { rows[at]["pedestal"] = pose.pedestal }
            else { rows.append(.object(["placementId": .string(hostId), "pedestal": pose.pedestal])) }
            extensions[installation] = .object(["version": .integer(1), "placements": .array(rows)])
        } else {
            guard let index else { return candidate }
            metadata.bindings.remove(at: index)
        }
        guard metadata.bindings.count <= 32 else { throw AuthoringFailure("FURNISHING_INVALID") }
        if metadata.bindings.isEmpty { extensions.removeValue(forKey: FurnishingCatalog.namespace) }
        else {
            let value: DocumentValue = .object(["version": .integer(1), "assets": .array(metadata.assets), "bindings": .array(metadata.bindings)])
            guard try JSONEncoder().encode(value).count <= 32_768 else { throw AuthoringFailure("FURNISHING_INVALID") }
            extensions[FurnishingCatalog.namespace] = value
        }
        let template = "org.exhibitos.studio/template"
        if case .object(var fields) = extensions[template] { fields["modified"] = .bool(true); extensions[template] = .object(fields) }
        candidate["extensions"] = .object(extensions)
        return candidate
    }
}

public struct FurnishingPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft, command: FurnishingCommand, candidate: DocumentValue, displayDraft: NativeExhibitionDraft
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: FurnishingCommand) throws {
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        let assetId = candidate["extensions"][FurnishingCatalog.namespace]["assets"].items?.first?["id"].string
        var expected = try command.expected(in: base, newAssetId: assetId)
        // Shared quaternion affine evaluation may differ by a few ulps from the
        // native display formula. Tolerate only this selected fallback center.
        if command.pose != nil, let actualRows = candidate["extensions"][InstallationSettings.namespace]["placements"].items,
           let index = actualRows.firstIndex(where: { FurnishingCatalog.same($0["placementId"].string, command.hostId) }),
           let point = actualRows[index]["pedestal"]["position"].items,
           var expectedRows = expected["extensions"][InstallationSettings.namespace]["placements"].items,
           index < expectedRows.count, let predicted = expectedRows[index]["pedestal"]["position"].items, point.count == 3, predicted.count == 3 {
            for axis in 0..<3 { guard let a = point[axis].double, let b = predicted[axis].double, a.isFinite, abs(a-b) <= 1e-9 else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") } }
            expectedRows[index]["pedestal"]["position"] = .array(point)
            expected["extensions"][InstallationSettings.namespace]["placements"] = .array(expectedRows)
        }
        guard candidate == expected else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, command: FurnishingCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        _ = try Self(candidate: candidate, base: draft, command: command)
        var next = draft; if candidate != draft.candidate { try next.replaceCandidate(candidate) }; return next
    }
}
