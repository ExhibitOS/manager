// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A saved-head light-object proposal. The shared producer admits geometry;
/// this consumer binds its reply to exactly one complete graph mutation.
public struct LightObjectRequest: Sendable {
    public let frame: ViewportFrame
    public let command: DocumentValue
    public let value: DocumentValue
    public let sourceLightId: String

    private static let details = NativeLightFixture.namespace
    private static let attachments = "org.exhibitos.studio/light-attachments"
    private static let visuals = FurnishingCatalog.namespace
    private static let template = "org.exhibitos.studio/template"
    private static let bindings: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
    private static var invalid: AuthoringFailure { AuthoringFailure("LIGHT_FIELDS_INVALID") }
    private static var changed: AuthoringFailure { AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
    private static func same(_ a: String?, _ b: String) -> Bool { a?.lowercased() == b.lowercased() }
    private static func uuid(_ value: String?) -> Bool {
        guard let value else { return false }; return UUID(uuidString: value) != nil
    }

    /// Bind an ordinary acknowledged rendering frame to the owned saved Native
    /// document. Ordinary Web ACKs omit workspace/ETag; the existing Native
    /// session and saved-head API supply and verify that separate authority.
    public static func nativeFrame(acknowledged frame: ViewportFrame, document: NativeDocumentState,
                                   ownerEpoch: String?, acceptedSequence: Int, readOnly: Bool?,
                                   boundedEdit: NativeBoundedEditCapability?) throws -> ViewportFrame {
        guard !document.dirty, document.pending == nil, let draft = document.draft, let etag = document.etag,
              UUID(uuidString: document.workspaceId) != nil,
              frame.epoch == ownerEpoch, frame.sequence == acceptedSequence, acceptedSequence > 0,
              readOnly == false, boundedEdit == nil, !frame.disabled, frame.draft == draft,
              frame.tenantId == document.tenantId, frame.principalId == document.principalId,
              (frame.workspaceId == nil) == (frame.baseETag == nil),
              frame.workspaceId == nil || frame.workspaceId == document.workspaceId,
              frame.baseETag == nil || frame.baseETag == etag else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return ViewportFrame(epoch: frame.epoch, sequence: frame.sequence, tenantId: frame.tenantId,
            principalId: frame.principalId, draft: frame.draft, selection: frame.selection,
            mode: frame.mode, view: frame.view, disabled: false, cameraSequence: frame.cameraSequence,
            transparent: frame.transparent, presentation: frame.presentation,
            workspaceId: document.workspaceId, baseETag: etag)
    }

    /// Construct the coupled command with the existing admitted ceiling-mount
    /// adapter's transform. Detach is explicit and does not move or retune light.
    public static func mountCommand(lightId: String, attachment: DocumentValue, frame: ViewportFrame) throws -> DocumentValue {
        guard !frame.disabled, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId, lightId].allSatisfy({ uuid($0) }),
              let lights = frame.draft.candidate["lights"].items,
              lights.filter({ same($0["id"].string, lightId) }).count == 1,
              let original = lights.first(where: { same($0["id"].string, lightId) }),
              let actualId = original["id"].string else { throw invalid }
        let light: DocumentValue
        if attachment == .null { light = original }
        else {
            let mount = try NativeAutomationFixture.AttachmentRequest(.object([
                "lightId": .string(actualId), "attachment": attachment
            ]), frame: frame)
            guard let mounted = mount.expected["lights"].items?.first(where: { same($0["id"].string, actualId) }) else { throw invalid }
            light = mounted
        }
        guard case .object(var fields) = light else { throw invalid }
        fields.removeValue(forKey: "id")
        let settings = try NativeLightFixture.settings(in: frame.draft, lightId: actualId)
        try checkLight(.object(fields), in: frame.draft)
        try checkSettings(settings)
        return .object(["type": .string("set-light-object"), "lightId": .string(actualId),
                        "light": .object(fields), "settings": settings, "attachment": attachment])
    }

    public init(command: DocumentValue, frame: ViewportFrame) throws {
        guard !frame.disabled, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ Self.uuid($0) }),
              case .object(let fields) = command,
              let type = command["type"].string, LightObjectAuthoring.commands.contains(type),
              let id = command["lightId"].string, Self.uuid(id),
              let lights = frame.draft.candidate["lights"].items,
              lights.filter({ Self.same($0["id"].string, id) }).count == 1,
              let original = lights.first(where: { Self.same($0["id"].string, id) }),
              let actualId = original["id"].string else { throw Self.invalid }
        _ = try Self.rows(frame.draft.candidate, namespace: Self.details)
        _ = try Self.rows(frame.draft.candidate, namespace: Self.attachments)
        _ = try Self.furnishings(frame.draft.candidate)
        switch type {
        case "set-light-object":
            guard Set(fields.keys).subtracting(["attachment"]) == ["type", "lightId", "light", "settings"],
                  case .object(let light) = command["light"], light["id"] == nil,
                  case .object(let settings) = command["settings"], settings["lightId"] == nil,
                  command["light"]["roomId"] == original["roomId"],
                  command["light"]["castsShadow"] == original["castsShadow"],
                  command["light"]["transform"]["scale"] == original["transform"]["scale"] else { throw Self.invalid }
            try Self.checkLight(command["light"], in: frame.draft)
            try Self.checkSettings(command["settings"])
            let old = try Self.rows(frame.draft.candidate, namespace: Self.details).first { Self.same($0["lightId"].string, actualId) }
            if let purpose = settings["purpose"] { guard purpose == (old?["purpose"] ?? .null) else { throw Self.invalid } }
            if let placement = settings["basePlacement"] { guard placement == (old?["basePlacement"] ?? .null) else { throw Self.invalid } }
            if let attachment = fields["attachment"], attachment != .null { try Self.checkAttachment(attachment) }
        case "set-room-base-light-placement":
            guard Set(fields.keys) == ["type", "lightId", "basePlacement"],
                  let value = command["basePlacement"].string, let placement = RoomBaseLightPlacement(rawValue: value) else { throw Self.invalid }
            _ = try RoomBaseLightIntent.setPlacement(placement, lightId: actualId, in: frame.draft.candidate)
        case "set-light-fixture":
            guard Set(fields.keys) == ["type", "lightId", "visual"] else { throw Self.invalid }
            if command["visual"] != .null {
                guard case .object(let visual) = command["visual"], Set(visual.keys) == ["catalogId", "catalogVersion", "pose"],
                      let catalog = command["visual"]["catalogId"].string,
                      command["visual"]["catalogVersion"] == .integer(1), original["type"] == .string("spot"),
                      try SharedFurnishingCatalog.descriptor(catalog)["hostKinds"].items == [.string("light")] else { throw Self.invalid }
                _ = try FurnishingPose(command["visual"]["pose"])
            }
        case "duplicate-light-object", "remove-light-object":
            guard Set(fields.keys) == ["type", "lightId"] else { throw Self.invalid }
            if type == "duplicate-light-object" {
                guard !(try Self.rows(frame.draft.candidate, namespace: Self.details)).contains(where: {
                    Self.same($0["lightId"].string, actualId) && $0["purpose"] == .string("room-base")
                }) else { throw Self.invalid }
            } else { try Self.noForeignReference(frame.draft.candidate, id: actualId) }
        default: throw Self.invalid
        }
        var wire: [String: DocumentValue] = ["version": .integer(1), "epoch": .string(frame.epoch),
            "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId),
            "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased())]
        guard Set(fields.keys).isDisjoint(with: Self.bindings) else { throw Self.invalid }
        wire.merge(fields) { _, new in new }
        let value = DocumentValue.object(wire)
        guard try JSONEncoder().encode(value).count <= 65_536 else { throw Self.invalid }
        self.frame = frame; self.command = command; self.sourceLightId = actualId; self.value = value
    }

    public func checkedReply(_ reply: DocumentValue) throws -> LightObjectPreview {
        guard case .object(let fields) = reply, Set(fields.keys) == Self.bindings.union(["candidate", "lightId"]),
              try JSONEncoder().encode(reply).count <= 1_100_000,
              Self.bindings.allSatisfy({ reply[$0] == value[$0] }),
              let id = reply["lightId"].string, Self.uuid(id) else { throw Self.changed }
        if command["type"] == .string("duplicate-light-object") {
            guard !Self.references(frame.draft.candidate, id: id) else { throw Self.changed }
        } else { guard id == sourceLightId else { throw Self.changed } }
        let expected = try expectedCandidate(lightId: id, reply: reply["candidate"])
        guard reply["candidate"] == expected else { throw Self.changed }
        return LightObjectPreview(base: frame.draft, command: command, candidate: expected, lightId: id)
    }

    private func expectedCandidate(lightId: String, reply: DocumentValue) throws -> DocumentValue {
        let base = frame.draft.candidate
        var next = base
        guard var lights = base["lights"].items,
              let index = lights.firstIndex(where: { Self.same($0["id"].string, sourceLightId) }) else { throw Self.changed }
        let original = lights[index]
        switch command["type"].string {
        case "set-light-object":
            var row = command["light"]; row["id"] = original["id"]; lights[index] = row; next["lights"] = .array(lights)
            let mounted: Bool
            if case .object(let fields) = command { mounted = fields["attachment"].map { $0 != .null } ?? false } else { mounted = false }
            var settings = try RoomBaseLightIntent.settingsForEdit(command["settings"], light: row, original: original, base: base, mounted: mounted)
            settings["lightId"] = original["id"]
            try Self.upsert(settings, id: sourceLightId, namespace: Self.details, in: &next)
            if case .object(let fields) = command, let attachment = fields["attachment"] {
                if attachment == .null { try Self.removeRow(sourceLightId, namespace: Self.attachments, in: &next) }
                else { var row = attachment; row["lightId"] = original["id"]; try Self.upsert(row, id: sourceLightId, namespace: Self.attachments, in: &next) }
            }
        case "set-room-base-light-placement":
            guard let raw = command["basePlacement"].string, let placement = RoomBaseLightPlacement(rawValue: raw) else { throw Self.changed }
            next = try RoomBaseLightIntent.setPlacement(placement, lightId: sourceLightId, in: next)
        case "set-light-fixture":
            let visual = command["visual"]
            var settings = try NativeLightFixture.settings(in: frame.draft, lightId: sourceLightId)
            settings["fixture"] = .string(visual == .null ? "none" : "spot-can")
            settings["lightId"] = original["id"]
            try Self.upsert(settings, id: sourceLightId, namespace: Self.details, in: &next)
            var graph = try Self.furnishings(base)
            if visual == .null {
                graph.bindings.removeAll { $0["hostKind"] == .string("light") && Self.same($0["hostId"].string, sourceLightId) }
            } else {
                guard let catalog = visual["catalogId"].string else { throw Self.changed }
                let asset: DocumentValue
                if let old = graph.assets.first(where: { $0["catalogId"] == .string(catalog) }) { asset = old }
                else {
                    let candidates = reply["extensions"][Self.visuals]["assets"].items ?? []
                    let added = candidates.filter { $0["catalogId"] == .string(catalog) }
                    guard added.count == 1, let id = added[0]["id"].string, Self.uuid(id), !Self.references(base, id: id),
                          added[0] == (try SharedFurnishingCatalog.asset(catalogId: catalog, id: id)) else { throw Self.changed }
                    asset = added[0]; graph.assets.append(asset)
                }
                let binding: DocumentValue = .object(["hostKind": .string("light"), "hostId": original["id"], "assetId": asset["id"], "pose": visual["pose"]])
                if let at = graph.bindings.firstIndex(where: { $0["hostKind"] == .string("light") && Self.same($0["hostId"].string, sourceLightId) }) { graph.bindings[at] = binding }
                else { graph.bindings.append(binding) }
            }
            try Self.writeFurnishings(graph, base: base, in: &next)
        case "duplicate-light-object":
            var copied = original; copied["id"] = .string(lightId); lights.append(copied); next["lights"] = .array(lights)
            for namespace in [Self.details, Self.attachments] {
                if var row = try Self.rows(base, namespace: namespace).first(where: { Self.same($0["lightId"].string, sourceLightId) }) {
                    row["lightId"] = .string(lightId)
                    var rows = try Self.rows(base, namespace: namespace); rows.append(row); Self.writeRows(rows, namespace: namespace, in: &next)
                }
            }
            var graph = try Self.furnishings(base)
            if var row = graph.bindings.first(where: { $0["hostKind"] == .string("light") && Self.same($0["hostId"].string, sourceLightId) }) {
                row["hostId"] = .string(lightId); graph.bindings.append(row)
                try Self.writeFurnishings(graph, base: base, in: &next)
            }
        case "remove-light-object":
            for namespace in [Self.details, Self.attachments] { try Self.removeRow(sourceLightId, namespace: namespace, in: &next) }
            lights.remove(at: index); next["lights"] = .array(lights)
            var graph = try Self.furnishings(base)
            graph.bindings.removeAll { $0["hostKind"] == .string("light") && Self.same($0["hostId"].string, sourceLightId) }
            try Self.writeFurnishings(graph, base: base, in: &next)
        default: throw Self.changed
        }
        if next != base, case .object = next["extensions"][Self.template] { next["extensions"][Self.template]["modified"] = .bool(true) }
        return next
    }

    private static func rows(_ candidate: DocumentValue, namespace: String) throws -> [DocumentValue] {
        guard case .object(let extensions) = candidate["extensions"] else {
            if candidate["extensions"] == .null { return [] }; throw invalid
        }
        guard let value = extensions[namespace] else { return [] }
        guard case .object(let fields) = value, Set(fields.keys) == ["version", "lights"], value["version"] == .integer(1),
              let rows = value["lights"].items, rows.count <= 32 else { throw invalid }
        var seen = Set<String>()
        for row in rows {
            guard let id = row["lightId"].string, uuid(id), seen.insert(id.lowercased()).inserted,
                  candidate["lights"].items?.filter({ same($0["id"].string, id) }).count == 1,
                  case .object(var fields) = row else { throw invalid }
            fields.removeValue(forKey: "lightId")
            if namespace == details { try checkSettings(.object(fields)) }
            else { try checkAttachment(.object(fields)) }
        }
        return rows
    }
    private static func writeRows(_ rows: [DocumentValue], namespace: String, in candidate: inout DocumentValue) {
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        candidate["extensions"][namespace] = .object(["version": .integer(1), "lights": .array(rows)])
    }
    private static func upsert(_ row: DocumentValue, id: String, namespace: String, in candidate: inout DocumentValue) throws {
        var rows = try Self.rows(candidate, namespace: namespace)
        if let index = rows.firstIndex(where: { same($0["lightId"].string, id) }) { rows[index] = row }
        else { rows.append(row) }
        guard rows.count <= 32 else { throw invalid }; writeRows(rows, namespace: namespace, in: &candidate)
    }
    private static func removeRow(_ id: String, namespace: String, in candidate: inout DocumentValue) throws {
        let rows = try Self.rows(candidate, namespace: namespace).filter { !same($0["lightId"].string, id) }
        if case .object(let extensions) = candidate["extensions"], extensions[namespace] != nil { writeRows(rows, namespace: namespace, in: &candidate) }
    }
    private struct Furnishings { var assets: [DocumentValue]; var bindings: [DocumentValue] }
    private static func furnishings(_ candidate: DocumentValue) throws -> Furnishings {
        guard case .object(let extensions) = candidate["extensions"] else {
            if candidate["extensions"] == .null { return Furnishings(assets: [], bindings: []) }; throw invalid
        }
        guard let value = extensions[visuals] else { return Furnishings(assets: [], bindings: []) }
        guard case .object(let fields) = value, Set(fields.keys) == ["version", "assets", "bindings"], value["version"] == .integer(1),
              let assets = value["assets"].items, (1...8).contains(assets.count), let bindings = value["bindings"].items, (1...32).contains(bindings.count) else { throw invalid }
        var ids = Set<String>(), catalogs = Set<String>(), hosts = Set<String>(), used = Set<String>()
        for asset in assets {
            guard let id = asset["id"].string, let catalog = asset["catalogId"].string, ids.insert(id.lowercased()).inserted,
                  catalogs.insert(catalog).inserted, asset == (try SharedFurnishingCatalog.asset(catalogId: catalog, id: id)) else { throw invalid }
        }
        for binding in bindings {
            guard case .object(let row) = binding, Set(row.keys) == ["hostKind", "hostId", "assetId", "pose"],
                  let kind = binding["hostKind"].string, SharedFurnishingCommand.Host(rawValue: kind) != nil,
                  let host = binding["hostId"].string, uuid(host), hosts.insert(kind + ":" + host.lowercased()).inserted,
                  let id = binding["assetId"].string, let asset = assets.first(where: { same($0["id"].string, id) }),
                  let catalog = asset["catalogId"].string,
                  try SharedFurnishingCatalog.descriptor(catalog)["hostKinds"].items?.contains(.string(kind)) == true else { throw invalid }
            _ = try FurnishingPose(binding["pose"]); used.insert(id.lowercased())
        }
        guard used == ids else { throw invalid }; return Furnishings(assets: assets, bindings: bindings)
    }
    private static func writeFurnishings(_ graph: Furnishings, base: DocumentValue, in candidate: inout DocumentValue) throws {
        let assets = graph.assets.filter { asset in graph.bindings.contains { same($0["assetId"].string, asset["id"].string ?? "") } }
        for asset in graph.assets where !assets.contains(asset) { if let id = asset["id"].string { try noForeignReference(base, id: id) } }
        guard assets.count <= 8, graph.bindings.count <= 32 else { throw invalid }
        if graph.bindings.isEmpty {
            if case .object(var extensions) = candidate["extensions"] { extensions.removeValue(forKey: visuals); candidate["extensions"] = .object(extensions) }
        } else {
            if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
            candidate["extensions"][visuals] = .object(["version": .integer(1), "assets": .array(assets), "bindings": .array(graph.bindings)])
        }
    }
    private static func checkSettings(_ value: DocumentValue) throws {
        guard case .object(let fields) = value, fields["fixture"] != nil,
              Set(fields.keys).isSubset(of: ["fixture", "aim", "penumbra", "range", "temperatureKelvin", "purpose", "basePlacement"]),
              let fixture = value["fixture"].string, NativeLightFixture(rawValue: fixture) != nil,
              fields["purpose"].map({ $0 == .string("room-base") }) ?? true else { throw invalid }
        if let placement = fields["basePlacement"] {
            guard let raw = placement.string, RoomBaseLightPlacement(rawValue: raw) != nil else { throw invalid }
        }
        if let aim = fields["aim"] {
            guard case .object(let keys) = aim, let kind = aim["kind"].string,
                  ((kind == "rotation" || kind == "placement-center") && Set(keys.keys) == ["kind"])
                    || (kind == "point" && Set(keys.keys) == ["kind", "position"] && vector(aim["position"], count: 3)) else { throw invalid }
        }
        for key in ["penumbra", "range", "temperatureKelvin"] where fields[key] != nil {
            guard let number = value[key].double, number.isFinite else { throw invalid }
        }
    }
    private static func checkAttachment(_ value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys) == ["surfaceId", "anchor", "gap"], uuid(value["surfaceId"].string),
              vector(value["anchor"], count: 2), let gap = value["gap"].double, gap.isFinite, (0.01...1).contains(gap) else { throw invalid }
    }
    private static func vector(_ value: DocumentValue, count: Int) -> Bool {
        value.items?.count == count && value.items?.allSatisfy({ $0.double?.isFinite == true }) == true
    }
    private static func checkLight(_ value: DocumentValue, in draft: NativeExhibitionDraft) throws {
        guard case .object(let fields) = value,
              Set(fields.keys).isSuperset(of: ["roomId", "type", "transform", "color", "intensity", "unit", "castsShadow"]),
              Set(fields.keys).isSubset(of: ["roomId", "type", "transform", "color", "intensity", "unit", "castsShadow", "beamAngle", "dimensions", "targetPlacementId"]),
              let kind = value["type"].string, let type = NativeLightType(rawValue: kind), value["unit"] == .string(type.unit),
              let room = value["roomId"].string, draft.rooms.filter({ same($0["id"].string, room) }).count == 1,
              case .object(let pose) = value["transform"], Set(pose.keys) == ["position", "rotation", "scale"],
              vector(value["transform"]["position"], count: 3), vector(value["transform"]["rotation"], count: 4),
              value["transform"]["scale"] == .vector([1, 1, 1]), vector(value["color"], count: 3),
              value["color"].items?.allSatisfy({ (0...1).contains($0.double ?? -1) }) == true,
              let intensity = value["intensity"].double, intensity.isFinite, intensity >= 0,
              case .bool = value["castsShadow"], (type == .spot) == (fields["beamAngle"] != nil),
              (type == .area) == (fields["dimensions"] != nil) else { throw invalid }
        if type == .spot { guard let angle = value["beamAngle"].double, angle.isFinite, angle > 0, angle <= Double.pi / 2 else { throw invalid } }
        if type == .area {
            guard case .object(let dimensions) = value["dimensions"], Set(dimensions.keys) == ["width", "height"],
                  ["width", "height"].allSatisfy({ value["dimensions"][$0].double.map { $0.isFinite && $0 > 0 } == true }) else { throw invalid }
        }
        if fields["targetPlacementId"] != nil { guard uuid(value["targetPlacementId"].string) else { throw invalid } }
    }
    private static func references(_ value: DocumentValue, id: String) -> Bool {
        switch value {
        case .string(let text): return same(text, id)
        case .array(let rows): return rows.contains { references($0, id: id) }
        case .object(let fields): return fields.contains { same($0.key, id) || references($0.value, id: id) }
        default: return false
        }
    }
    private static func noForeignReference(_ candidate: DocumentValue, id: String) throws {
        guard case .object(var fields) = candidate else { throw invalid }
        fields.removeValue(forKey: "lights")
        if case .object(var extensions) = fields["extensions"] {
            for namespace in [details, attachments, visuals] { extensions.removeValue(forKey: namespace) }
            fields["extensions"] = .object(extensions)
        }
        guard !references(.object(fields), id: id) else { throw invalid }
    }
}

public struct LightObjectPreview: Sendable {
    public let base: NativeExhibitionDraft
    public let command: DocumentValue
    public let candidate: DocumentValue
    public let lightId: String

    fileprivate init(base: NativeExhibitionDraft, command: DocumentValue, candidate: DocumentValue, lightId: String) {
        self.base = base; self.command = command; self.candidate = candidate; self.lightId = lightId
    }
    public func apply(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
        guard draft == base else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        if candidate == base.candidate { return draft }
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}
