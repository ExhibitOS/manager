// SPDX-License-Identifier: Apache-2.0
import Foundation

/// The v2 wire command is separate from the legacy canted-pedestal adapter.
public struct SharedFurnishingCommand: Equatable, Sendable {
    public enum Host: String, Sendable { case pedestal, bench, light, opening }
    public let host: Host, hostId: String, pose: FurnishingPose?
    public var catalogId: String {
        switch host { case .pedestal: "rectangular-plinth"; case .bench: "backless-oak-bench"; case .light: "museum-track-spot"; case .opening: "gallery-open-portal" }
    }
    public init(host: Host, hostId: String, pose: FurnishingPose?) throws {
        guard UUID(uuidString: hostId) != nil else { throw AuthoringFailure("FURNISHING_INVALID") }
        self.host = host; self.hostId = hostId; self.pose = pose
    }
    public var value: DocumentValue {
        var fields: [String: DocumentValue] = ["type": .string(pose == nil ? "remove-furnishing-visual" : "set-furnishing-visual"), "hostKind": .string(host.rawValue), "hostId": .string(hostId)]
        if let pose { fields["catalogId"] = .string(catalogId); fields["catalogVersion"] = .integer(1); fields["pose"] = pose.value }
        return .object(fields)
    }
}

/// A shared candidate can be committed only if its entire delta corresponds to
/// the requested host and the exact known assets. Full admission is shared.
public struct SharedFurnishingPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft, command: SharedFurnishingCommand, candidate: DocumentValue, displayDraft: NativeExhibitionDraft
    private static let ns = FurnishingCatalog.namespace
    private static func same(_ a: String?, _ b: String?) -> Bool { a != nil && b != nil && a!.lowercased() == b!.lowercased() }
    private static func hostExists(_ kind: SharedFurnishingCommand.Host, id: String, draft: NativeExhibitionDraft) throws -> Bool {
        let rows: [DocumentValue]
        switch kind {
        case .pedestal: rows = draft.placements
        case .opening: rows = draft.candidate["openings"].items ?? []
        case .light: rows = draft.candidate["lights"].items ?? []
        case .bench: rows = try BenchSettings.list(in: draft).map(\.value)
        }
        return rows.filter { same($0["id"].string, id) }.count == 1
    }
    private static func metadata(_ draft: NativeExhibitionDraft) throws -> (assets: [DocumentValue], bindings: [DocumentValue]) {
        let raw = draft.candidate["extensions"][ns]
        if raw == .null {
            if case .object(let fields) = draft.candidate["extensions"], fields[ns] != nil { throw AuthoringFailure("FURNISHING_INVALID") }
            return ([], [])
        }
        guard case .object(let fields) = raw, Set(fields.keys) == ["version", "assets", "bindings"], raw["version"] == .integer(1),
              let assets = raw["assets"].items, (1...8).contains(assets.count), let bindings = raw["bindings"].items, (1...32).contains(bindings.count),
              try JSONEncoder().encode(raw).count <= 32_768 else { throw AuthoringFailure("FURNISHING_INVALID") }
        var ids = Set<String>(), recipes = Set<String>(), hosts = Set<String>(), used = Set<String>()
        for asset in assets {
            guard let id = asset["id"].string, let catalog = asset["catalogId"].string, asset == (try SharedFurnishingCatalog.asset(catalogId: catalog, id: id)),
                  ids.insert(id.lowercased()).inserted, recipes.insert(catalog).inserted else { throw AuthoringFailure("FURNISHING_INVALID") }
            try FurnishingCatalog.checkAssetIdentity(id, in: draft)
            guard !(try BenchSettings.list(in: draft)).contains(where: { same($0.id, id) }),
                  !same(draft.candidate["extensions"]["org.exhibitos.viewer/space-still"]["asset"]["id"].string, id) else { throw AuthoringFailure("FURNISHING_INVALID") }
        }
        for binding in bindings {
            guard case .object(let row) = binding, Set(row.keys) == ["hostKind", "hostId", "assetId", "pose"],
                  let rawKind = binding["hostKind"].string, let kind = SharedFurnishingCommand.Host(rawValue: rawKind),
                  let host = binding["hostId"].string, UUID(uuidString: host) != nil, let assetId = binding["assetId"].string,
                  let asset = assets.first(where: { same($0["id"].string, assetId) }),
                  try hostExists(kind, id: host, draft: draft), hosts.insert(rawKind + ":" + host.lowercased()).inserted else { throw AuthoringFailure("FURNISHING_INVALID") }
            let check = try SharedFurnishingCommand(host: kind, hostId: host, pose: nil)
            guard asset["catalogId"].string == check.catalogId else { throw AuthoringFailure("FURNISHING_INVALID") }
            _ = try FurnishingPose(binding["pose"]); used.insert(assetId.lowercased())
        }
        guard used == ids else { throw AuthoringFailure("FURNISHING_INVALID") }
        return (assets, bindings)
    }
    public static func pose(in draft: NativeExhibitionDraft, host: SharedFurnishingCommand.Host, id: String) throws -> FurnishingPose? {
        guard try hostExists(host, id: id, draft: draft) else { throw AuthoringFailure("FURNISHING_INVALID") }
        let meta = try metadata(draft)
        return try meta.bindings.first(where: { $0["hostKind"] == .string(host.rawValue) && same($0["hostId"].string, id) }).map { try FurnishingPose($0["pose"]) }
    }
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: SharedFurnishingCommand) throws {
        guard try JSONEncoder().encode(candidate).count <= 1_048_576,
              try Self.hostExists(command.host, id: command.hostId, draft: base) else { throw AuthoringFailure("FURNISHING_INVALID") }
        var meta = try Self.metadata(base), expected = base.candidate
        var extensions: [String: DocumentValue]
        if case .object(let fields) = expected["extensions"] { extensions = fields }
        else if expected["extensions"] == .null { extensions = [:] }
        else { throw AuthoringFailure("FURNISHING_INVALID") }
        let index = meta.bindings.firstIndex { $0["hostKind"] == .string(command.host.rawValue) && Self.same($0["hostId"].string, command.hostId) }
        if let pose = command.pose {
            let selected: DocumentValue
            if let old = meta.assets.first(where: { $0["catalogId"].string == command.catalogId }) { selected = old }
            else {
                guard let rows = candidate["extensions"][Self.ns]["assets"].items,
                      let id = rows.first(where: { $0["catalogId"].string == command.catalogId })?["id"].string else { throw AuthoringFailure("FURNISHING_INVALID") }
                try FurnishingCatalog.checkAssetIdentity(id, in: base)
                guard !(try BenchSettings.list(in: base)).contains(where: { Self.same($0.id, id) }),
                      !Self.same(base.candidate["extensions"]["org.exhibitos.viewer/space-still"]["asset"]["id"].string, id),
                      !meta.assets.contains(where: { Self.same($0["id"].string, id) }) else { throw AuthoringFailure("FURNISHING_INVALID") }
                selected = try SharedFurnishingCatalog.asset(catalogId: command.catalogId, id: id); meta.assets.append(selected)
            }
            let binding: DocumentValue = .object(["hostKind": .string(command.host.rawValue), "hostId": .string(command.hostId), "assetId": selected["id"], "pose": pose.value])
            if let index { meta.bindings[index] = binding } else { meta.bindings.append(binding) }
            let descriptor = try SharedFurnishingCatalog.descriptor(command.catalogId)
            let min = descriptor["bounds"]["min"].items!.compactMap(\.double), max = descriptor["bounds"]["max"].items!.compactMap(\.double)
            guard min.count == 3, max.count == 3 else { throw AuthoringFailure("FURNISHING_INVALID") }
            let width = (max[0]-min[0])*pose.scale, height = max[1]*pose.scale, depth = (max[2]-min[2])*pose.scale
            switch command.host {
            case .pedestal:
                _ = try base.installationSettings(for: command.hostId)
                var rows = extensions[InstallationSettings.namespace]?["placements"].items ?? []
                let center = pose.point([(min[0]+max[0])/2, 0, (min[2]+max[2])/2])
                let pedestal: DocumentValue = .object(["kind": .string("box"), "position": .vector(center), "yaw": .number(pose.yaw), "dimensions": .object(["width": .number(width), "height": .number(height), "depth": .number(depth)]), "material": .object(["preset": .string("paint"), "color": .string("#deded7"), "roughness": .number(0.65), "metalness": .integer(0)])])
                if let i = rows.firstIndex(where: { Self.same($0["placementId"].string, command.hostId) }) { rows[i]["pedestal"] = pedestal }
                else { rows.append(.object(["placementId": .string(command.hostId), "pedestal": pedestal])) }
                extensions[InstallationSettings.namespace] = .object(["version": .integer(1), "placements": .array(rows)])
            case .bench:
                guard var rows = extensions[BenchSettings.namespace]?["benches"].items,
                      let i = rows.firstIndex(where: { Self.same($0["id"].string, command.hostId) }),
                      let seatHeight = descriptor["functional"]["seatEnvelopeY"].double else { throw AuthoringFailure("FURNISHING_INVALID") }
                rows[i]["position"] = .vector(pose.position); rows[i]["yaw"] = .number(pose.yaw); rows[i]["kind"] = .string("slab")
                rows[i]["dimensions"] = .object(["width": .number(width), "height": .number(seatHeight*pose.scale), "depth": .number(depth)])
                extensions[BenchSettings.namespace]?["benches"] = .array(rows)
            case .opening:
                guard var rows = expected["openings"].items, let i = rows.firstIndex(where: { Self.same($0["id"].string, command.hostId) }),
                      let w = descriptor["functional"]["clearOpening"]["width"].double, let h = descriptor["functional"]["clearOpening"]["height"].double else { throw AuthoringFailure("FURNISHING_INVALID") }
                rows[i]["dimensions"] = .object(["width": .number(w*pose.scale), "height": .number(h*pose.scale)]); expected["openings"] = .array(rows)
            case .light: break
            }
        } else {
            guard let index else { throw AuthoringFailure("FURNISHING_INVALID") }
            meta.bindings.remove(at: index)
        }
        meta.assets = meta.assets.filter { asset in meta.bindings.contains { Self.same($0["assetId"].string, asset["id"].string) } }
        guard meta.assets.count <= 8, meta.bindings.count <= 32 else { throw AuthoringFailure("FURNISHING_INVALID") }
        if meta.bindings.isEmpty { extensions.removeValue(forKey: Self.ns) }
        else {
            let value: DocumentValue = .object(["version": .integer(1), "assets": .array(meta.assets), "bindings": .array(meta.bindings)])
            guard try JSONEncoder().encode(value).count <= 32_768 else { throw AuthoringFailure("FURNISHING_INVALID") }
            extensions[Self.ns] = value
        }
        if case .object(var fields) = extensions["org.exhibitos.studio/template"] { fields["modified"] = .bool(true); extensions["org.exhibitos.studio/template"] = .object(fields) }
        expected["extensions"] = .object(extensions)
        guard candidate == expected else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        var display = base; try display.replaceCandidate(candidate)
        self.base = base; self.command = command; self.candidate = candidate; self.displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, command: SharedFurnishingCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        _ = try Self(candidate: candidate, base: draft, command: command)
        var result = draft; if candidate != draft.candidate { try result.replaceCandidate(candidate) }; return result
    }
}

/// A single raw input buffer; invalid text remains visible until corrected or discarded.
public struct SharedFurnishingFormBuffer: Equatable, Sendable {
    public let host: SharedFurnishingCommand.Host, hostId: String
    public var selected: Bool
    public var x: String, y: String, z: String, yawDegrees: String, scale: String
    private let initialPose: FurnishingPose?, initialYaw: String
    public init(draft: NativeExhibitionDraft, host: SharedFurnishingCommand.Host, hostId: String) throws {
        let pose = try SharedFurnishingPreview.pose(in: draft, host: host, id: hostId)
        self.host = host; self.hostId = hostId; selected = pose != nil; initialPose = pose
        x = pose.map { String($0.position[0]) } ?? ""; y = pose.map { String($0.position[1]) } ?? ""; z = pose.map { String($0.position[2]) } ?? ""
        yawDegrees = pose.map { String($0.yaw * 180 / .pi) } ?? "0"; scale = pose.map { String($0.scale) } ?? "1"
        if pose == nil {
            switch host {
            case .bench:
                if let row = try BenchSettings.list(in: draft).first(where: { $0.id == hostId }) {
                    x = String(row.position[0]); y = String(row.position[1]); z = String(row.position[2]); yawDegrees = String(row.yaw * 180 / .pi)
                }
            case .light: x = "0"; y = "0.15"; z = "0.0755"
            case .opening:
                if let row = draft.candidate["openings"].items?.first(where: { $0["id"].string == hostId }),
                   let offsets = row["offset"].items, offsets.count == 2, let ox = offsets[0].double, let oy = offsets[1].double {
                    x = String(ox); y = String(oy - 1.1); z = "0"
                }
            case .pedestal: break // No guessed artwork or placement anchor.
            }
        }
        initialYaw = yawDegrees
    }
    public func command(locale: Locale = .current) throws -> SharedFurnishingCommand {
        if !selected { return try .init(host: host, hostId: hostId, pose: nil) }
        func number(_ raw: String) throws -> Double {
            let text = raw.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: locale.decimalSeparator ?? ".", with: ".")
            guard let value = Double(text), value.isFinite else { throw AuthoringFailure("FURNISHING_FIELDS_INVALID") }
            return value
        }
        let yaw = yawDegrees == initialYaw && initialPose != nil ? initialPose!.yaw : try number(yawDegrees) * .pi / 180
        return try .init(host: host, hostId: hostId, pose: FurnishingPose(position: [number(x), number(y), number(z)], yaw: yaw, scale: number(scale)))
    }
}
