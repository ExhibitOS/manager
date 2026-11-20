// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A scoped request to the shared recorded floor policy. The producer owns the
/// mapping from an authored material to the approved recording.
public enum RecordedFloorCommand: Sendable {
    public static let profile = "saved-head-recorded-floor-default-v1"

    case select(FloorSettings.Target, FloorSettings.Preset)
    case reset(FloorSettings.Target)

    public var target: FloorSettings.Target {
        switch self {
        case .select(let target, _), .reset(let target): target
        }
    }

    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value, let kind = FloorSettings.Target.Kind(rawValue: value["target"]["kind"].string ?? "") else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        let targetKey = kind == .surface ? "surfaceId" : "roomId"
        guard let id = value["target"][targetKey].string, UUID(uuidString: id) != nil else {
            throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
        let target = FloorSettings.Target(kind: kind, id: id)
        guard value["target"] == target.value else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        switch value["type"].string {
        case "select-floor-material" where Set(fields.keys) == ["type", "target", "preset"]:
            guard let preset = FloorSettings.Preset(rawValue: value["preset"].string ?? "") else {
                throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
            }
            self = .select(target, preset)
        case "reset-floor-sound" where Set(fields.keys) == ["type", "target"]:
            self = .reset(target)
        default: throw BridgeFailure("AUTOMATION_COMMAND_INVALID")
        }
    }

    var fields: [String: DocumentValue] {
        switch self {
        case .select(let target, let preset):
            ["type": .string("select-floor-material"), "target": target.value, "preset": .string(preset.rawValue)]
        case .reset(let target):
            ["type": .string("reset-floor-sound"), "target": target.value]
        }
    }
}

public struct RecordedFloorStatus: Codable, Equatable, Sendable {
    public enum State: String, Codable, Sendable {
        case recorded, muted, unassigned, missing
    }
    public enum Source: String, Codable, Sendable {
        case stock, explicit
    }

    public let roomId: String
    public let surfaceId: String?
    public let material: String?
    public let state: State
    public let source: Source?
    public let issueCode: String?

    init(_ value: DocumentValue) throws {
        do {
            guard case .object(let fields) = value,
                  Set(fields.keys).isSubset(of: ["roomId", "surfaceId", "material", "state", "source", "issueCode"]) else {
                throw AuthoringFailure("FLOOR_RECORDING_RESPONSE_INVALID")
            }
            self = try JSONDecoder().decode(Self.self, from: JSONEncoder().encode(value))
        } catch {
            throw AuthoringFailure("FLOOR_RECORDING_RESPONSE_INVALID")
        }
    }
}

public struct RecordedFloorPreview: Sendable {
    public let command: RecordedFloorCommand
    public let base: NativeExhibitionDraft
    public let candidate: DocumentValue
    public let recording: RecordedFloorStatus

    public func applying(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
        guard draft == base else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        var next = draft
        if candidate != draft.candidate { try next.replaceCandidate(candidate) }
        return next
    }
}

struct RecordedFloorProposal {
    let value: DocumentValue
    let command: RecordedFloorCommand
    let base: NativeExhibitionDraft
    let roomId: String
    private let surfaceId: String?
    private let binding: [String: DocumentValue]

    init(command: RecordedFloorCommand, document: NativeDocumentState) throws {
        let target = command.target
        guard let draft = document.draft, !document.dirty, document.pending == nil,
              try FloorSettings.targets(in: draft.candidate).contains(where: { $0.key == target.key }),
              FloorSettings.isActive(target, in: draft.candidate) else {
            throw AuthoringFailure("FLOOR_TARGET_INVALID")
        }
        switch target.kind {
        case .roomBase:
            roomId = target.id
            surfaceId = nil
        case .surface:
            guard let surface = draft.candidate["surfaces"].items?.first(where: { $0["id"].string?.lowercased() == target.id.lowercased() }),
                  let roomId = surface["roomId"].string else { throw AuthoringFailure("FLOOR_TARGET_INVALID") }
            self.roomId = roomId
            surfaceId = target.id
        }
        binding = [
            "version": .integer(1), "epoch": .string(UUID().uuidString.lowercased()),
            "sequence": .integer(1), "tenantId": .string(document.tenantId),
            "principalId": .string(document.principalId), "requestId": .string(UUID().uuidString.lowercased()),
        ]
        self.command = command
        base = draft
        value = .object(binding.merging(command.fields) { existing, _ in existing })
    }

    func checkedReply(_ reply: DocumentValue) throws -> RecordedFloorPreview {
        var keys = Set(binding.keys).union(["candidate", "floorRecording"])
        if surfaceId != nil { keys.insert("surfaceId") }
        guard case .object(let fields) = reply, Set(fields.keys) == keys,
              binding.allSatisfy({ fields[$0.key] == $0.value }),
              reply["surfaceId"].string?.lowercased() == surfaceId?.lowercased(),
              try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        let recording = try RecordedFloorStatus(reply["floorRecording"])
        guard recording.roomId.lowercased() == roomId.lowercased(),
              recording.surfaceId?.lowercased() == surfaceId?.lowercased() else {
            throw AuthoringFailure("FLOOR_RECORDING_RESPONSE_INVALID")
        }
        let candidate = reply["candidate"]
        try checkCandidate(candidate, recording: recording)
        return .init(command: command, base: base, candidate: candidate, recording: recording)
    }

    private func checkCandidate(_ candidate: DocumentValue, recording: RecordedFloorStatus) throws {
        let namespace = FloorSettings.namespace
        let resources = "org.exhibitos.viewer/footstep-resources"
        let template = "org.exhibitos.studio/template"
        let target = command.target.key
        let prior = try FloorSettings.all(in: base.candidate)
        let replacement = try FloorSettings.all(in: candidate)
        let left = prior.filter { $0.target.key != target }
        let right = replacement.filter { $0.target.key != target }
        var expected = base.candidate
        if expected["extensions"] == .null && candidate["extensions"] != .null {
            expected["extensions"] = .object([:])
        }
        if candidate["extensions"][namespace] != .null {
            expected["extensions"][namespace] = candidate["extensions"][namespace]
        }
        let oldFloor = prior.first { $0.target.key == target }
        let newFloor = replacement.first { $0.target.key == target }
        switch command {
        case .select(_, let preset):
            guard let newFloor, newFloor.preset == preset else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            if let oldFloor, oldFloor.value["sound"] != .null {
                guard newFloor.value["sound"] == oldFloor.value["sound"] else {
                    throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED")
                }
            }
        case .reset:
            let oldAppearance = try oldFloor?.effectiveAppearance(candidate: base.candidate)
            let newAppearance = try newFloor?.effectiveAppearance(candidate: candidate)
            let soundMode = newFloor?.value["sound"]["mode"].string
            guard oldAppearance == newAppearance,
                  recording.state == .unassigned ? soundMode == nil : soundMode == "material-default" else {
                throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED")
            }
            let experience = "org.exhibitos.viewer/experience"
            if let surfaceId, let footsteps = expected["extensions"][experience]["footsteps"].items {
                let reset = footsteps.compactMap { row -> DocumentValue? in
                    guard row["surfaceId"].string?.lowercased() == surfaceId.lowercased(),
                          case .object(var fields) = row else { return row }
                    if recording.state == .unassigned { return nil }
                    fields.removeValue(forKey: "assetId")
                    fields.removeValue(forKey: "bank")
                    return .object(fields)
                }
                expected["extensions"][experience]["footsteps"] = .array(reset)
            }
        }
        if candidate["extensions"][resources] != base.candidate["extensions"][resources] {
            let old = base.candidate["extensions"][resources]["families"].items ?? []
            let new = candidate["extensions"][resources]["families"].items ?? []
            let names = new.compactMap(\.string)
            let allowed = Set(["wood", "marble", "tile", "carpet", "concrete"])
            guard !names.isEmpty, names.count == new.count, names == Array(Set(names)).sorted(),
                  Set(names).isSubset(of: allowed), old.allSatisfy(new.contains), new.count <= old.count + 1,
                  candidate["extensions"][resources] == .object(["version": .integer(1), "families": .array(new)])
            else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            expected["extensions"][resources] = candidate["extensions"][resources]
        }
        if expected["extensions"][template] != .null {
            expected["extensions"][template]["modified"] = candidate["extensions"][template]["modified"] == .bool(true)
                ? .bool(true) : expected["extensions"][template]["modified"]
        }
        guard left == right, expected == candidate else {
            throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED")
        }
    }
}
