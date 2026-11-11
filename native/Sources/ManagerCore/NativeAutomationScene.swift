// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Closed, local adapters for the saved-head scene proposal protocol. These
/// construct previews only; session ownership and authenticated save stay with
/// the caller. A reply must correspond to the entire typed candidate.
enum NativeAutomationScene {
    static func lightUpdate(_ value: DocumentValue, draft: NativeExhibitionDraft) throws -> LightCommand {
        guard case .object(let fields) = value,
              Set(fields.keys).isSubset(of: ["lightId", "type", "color", "intensity", "position", "beamAngle", "areaSize", "colorIntent"]),
              Set(fields.keys).isSuperset(of: ["lightId", "type", "color", "intensity", "position"]),
              let id = value["lightId"].string, UUID(uuidString: id) != nil,
              let name = value["type"].string, let type = NativeLightType(rawValue: name),
              let color = value["color"].items, let position = value["position"].items,
              color.allSatisfy({ $0.double != nil }), position.allSatisfy({ $0.double != nil }),
              let intensity = value["intensity"].double else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        let beam = fields["beamAngle"]?.double
        let area = fields["areaSize"]?.items?.compactMap(\.double)
        if fields["beamAngle"] != nil && beam == nil { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        if let raw = fields["areaSize"], (raw.items == nil || area?.count != raw.items?.count) { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        let intent: LightColorIntent
        if let raw = fields["colorIntent"] {
            guard case .object(let options) = raw, let kind = raw["kind"].string else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            switch kind {
            case "preserve" where Set(options.keys) == ["kind"]: intent = .preserve
            case "direct" where Set(options.keys) == ["kind"]: intent = .direct
            case "temperature" where Set(options.keys) == ["kind", "kelvin"]:
                guard let kelvin = raw["kelvin"].double else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                intent = .temperature(kelvin)
            default: throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
        } else { intent = .preserve }
        let current = try NativeLightFixture.settings(in: draft, lightId: id)
        guard let fixtureName = current["fixture"].string, let fixture = NativeLightFixture(rawValue: fixtureName) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        let command = LightCommand(lightId: id, type: type, color: color.compactMap(\.double), intensity: intensity,
                                   position: position.compactMap(\.double), beamAngle: beam, areaSize: area,
                                   fixture: fixture, colorIntent: intent)
        // Run the existing full validator now, before any producer request.
        _ = try command.proposedCandidate(in: draft, selectedLightId: id)
        return command
    }

    static func lightRequest(_ value: DocumentValue, frame: ViewportFrame) throws -> LightViewportRequest {
        let command = try lightUpdate(value, draft: frame.draft)
        return try LightViewportRequest(frame: frame, command: command, selectedLightId: command.lightId)
    }
    static func pedestal(_ value: DocumentValue, draft: NativeExhibitionDraft) throws -> (String, DocumentValue) {
        guard case .object(let f) = value, Set(f.keys) == ["placementId", "pedestal"],
              let id = value["placementId"].string, UUID(uuidString: id) != nil,
              draft.placements.filter({ $0["id"].string == id }).count == 1,
              case .object(let p) = value["pedestal"], let k = value["pedestal"]["kind"].string,
              let kind = InstallationSettings.Pedestal.Kind(rawValue: k) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        var result = InstallationSettings.Pedestal()
        result.kind = kind
        if kind == .none {
            guard Set(p.keys) == ["kind"] else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        } else {
            let ped = value["pedestal"], dimensions = ped["dimensions"], material = ped["material"]
            guard Set(p.keys) == ["kind", "position", "yaw", "dimensions", "material"],
                  case .object(let d) = dimensions,
                  Set(d.keys) == (kind == .box ? ["width", "depth", "height"] : ["diameter", "height"]),
                  let position = ped["position"].items, position.count == 3, position.allSatisfy({ $0.double != nil }),
                  let yaw = ped["yaw"].double, let height = dimensions["height"].double else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            result.position = position.compactMap(\.double); result.yaw = yaw; result.height = height
            if kind == .box {
                guard let width = dimensions["width"].double, let depth = dimensions["depth"].double else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                result.width = width; result.depth = depth
            } else {
                guard let diameter = dimensions["diameter"].double else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
                result.diameter = diameter
            }
            result.material = try InstallationSettings.readMaterial(material, frame: false)
        }
        var settings = try draft.installationSettings(for: id) ?? InstallationSettings()
        settings.pedestal = result
        var expected = draft; try expected.setInstallation(settings, for: id)
        return (id, expected.candidate)
    }

    /// The caller supplies a stable full light object. Shared producer validates
    /// OES fields; the client binds the reply to exactly this one addition.
    static func addition(_ value: DocumentValue, draft: NativeExhibitionDraft) throws -> (String, DocumentValue) {
        guard case .object(let f) = value, Set(f.keys).isSubset(of: ["light", "settings"]), f["light"] != nil,
              case .object = value["light"], let id = value["light"]["id"].string, UUID(uuidString: id) != nil,
              let room = value["light"]["roomId"].string, draft.rooms.filter({ $0["id"].string == room }).count == 1,
              var rows = draft.candidate["lights"].items,
              !rows.contains(where: { $0["id"].string?.lowercased() == id.lowercased() }) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        var expected = draft.candidate; rows.append(value["light"]); expected["lights"] = .array(rows)
        if let settings = f["settings"] {
            guard case .object(let fields) = settings, fields["lightId"] == nil, fields["purpose"] == nil else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            expected = try NativeLightFixture.applying(settings: settings, to: expected, base: draft, lightId: id)
        }
        let template = "org.exhibitos.studio/template"
        if expected["extensions"][template] != .null {
            guard case .bool = expected["extensions"][template]["modified"] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            expected["extensions"][template]["modified"] = .bool(true)
        }
        return (id, expected)
    }

    struct Proposal: Sendable {
        let frame: ViewportFrame
        let value: DocumentValue
        let targetKey: String
        let targetId: String
        let expected: DocumentValue
        init(frame: ViewportFrame, command: DocumentValue, targetKey: String, targetId: String, expected: DocumentValue) throws {
            guard !frame.disabled, frame.sequence > 0,
                  [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
                  case .object(let fields) = command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            var binding: [String: DocumentValue] = ["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased())]
            guard Set(binding.keys).isDisjoint(with: fields.keys) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            binding.merge(fields) { _, new in new }
            self.frame = frame; self.value = .object(binding); self.targetKey = targetKey; self.targetId = targetId; self.expected = expected
        }
        func checkedReply(_ reply: DocumentValue) throws -> ScenePreview {
            let keys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
            guard case .object(let fields) = reply, Set(fields.keys) == keys.union([targetKey, "candidate"]),
                  try JSONEncoder().encode(reply).count <= 1_100_000,
                  keys.allSatisfy({ reply[$0] == value[$0] }), reply[targetKey] == .string(targetId), reply["candidate"] == expected else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            return ScenePreview(base: frame.draft, candidate: expected)
        }
    }
    struct ScenePreview: Sendable {
        let base: NativeExhibitionDraft
        let candidate: DocumentValue
        func apply(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
            guard draft == base else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            var next = draft; if candidate != draft.candidate { try next.replaceCandidate(candidate) }; return next
        }
    }
    static func pedestalRequest(_ input: DocumentValue, frame: ViewportFrame) throws -> Proposal {
        let (id, expected) = try pedestal(input, draft: frame.draft)
        return try Proposal(frame: frame, command: .object(["type": .string("set-pedestal"), "placementId": .string(id), "pedestal": input["pedestal"]]), targetKey: "placementId", targetId: id, expected: expected)
    }
    static func additionRequest(_ input: DocumentValue, frame: ViewportFrame) throws -> Proposal {
        let (id, expected) = try addition(input, draft: frame.draft)
        guard case .object(var fields) = input else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        fields["type"] = .string("add-light")
        return try Proposal(frame: frame, command: .object(fields), targetKey: "lightId", targetId: id, expected: expected)
    }

    enum RoomBasePreparation: Sendable {
        case existing(ScenePreview)
        case addition(Proposal)
    }

    /// The existing typed room-base action defines the canonical fixture and
    /// validates the complete candidate. Caller ID is used only if absent.
    static func roomBaseRequest(_ input: DocumentValue, frame: ViewportFrame) throws -> RoomBasePreparation {
        guard case .object(let fields) = input, Set(fields.keys) == ["type", "roomId", "lightId"],
              input["type"] == .string("ensure-room-base-light"),
              let roomId = input["roomId"].string, let lightId = input["lightId"].string,
              UUID(uuidString: roomId) != nil, UUID(uuidString: lightId) != nil,
              frame.draft.rooms.filter({ $0["id"].string == roomId }).count == 1,
              !frame.disabled, frame.sequence > 0,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
              let rows = frame.draft.candidate["lights"].items else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        let selected = ViewportSelection(kind: "room", id: roomId)
        let action = RoomBaseLightAction.ensure(roomId: roomId)
        _ = try action.encodedValue(in: frame.draft, selected: selected)
        let markers = try NativeLightFixture.bindings(in: frame.draft.candidate).filter { $0["purpose"] == .string("room-base") }
        let matches = rows.filter { light in
            light["roomId"].string?.lowercased() == roomId.lowercased() && markers.contains { $0["lightId"].string?.lowercased() == light["id"].string?.lowercased() }
        }
        if !matches.isEmpty {
            var checked = frame.draft
            guard matches.count == 1, try !action.apply(frame.draft.candidate, to: &checked, selected: selected) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            return .existing(ScenePreview(base: frame.draft, candidate: frame.draft.candidate))
        }
        guard !rows.contains(where: { $0["id"].string?.lowercased() == lightId.lowercased() }),
              !frame.draft.rooms.contains(where: { $0["id"].string?.lowercased() == lightId.lowercased() }),
              let room = frame.draft.rooms.first(where: { $0["id"].string == roomId }) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        var expected = try RoomBaseLightAction.appendingLight(to: frame.draft.candidate, room: room, lightId: lightId)
        let template = "org.exhibitos.studio/template"
        if expected["extensions"][template] != .null {
            guard case .object = expected["extensions"][template], case .bool = expected["extensions"][template]["modified"] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            expected["extensions"][template]["modified"] = .bool(true)
        }
        var checked = frame.draft
        guard try action.apply(expected, to: &checked, selected: selected), checked.candidate == expected,
              let light = expected["lights"].items?.last,
              let binding = try NativeLightFixture.bindings(in: expected).first(where: { $0["lightId"].string == lightId }),
              case .object(var settings) = binding else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        settings.removeValue(forKey: "lightId")
        let command: DocumentValue = .object(["type": .string("add-light"), "light": light, "settings": .object(settings)])
        return .addition(try Proposal(frame: frame, command: command, targetKey: "lightId", targetId: lightId, expected: expected))
    }

}
