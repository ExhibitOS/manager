// SPDX-License-Identifier: Apache-2.0
import Foundation
import simd

/// Saved-head fixture proposals. Shared server geometry/authorization remains
/// authoritative; these checks bind the returned full candidate to the input.
enum NativeAutomationFixture {
    static let namespace = "org.exhibitos.studio/light-attachments"
    private static func same(_ a: String?, _ b: String?) -> Bool {
        guard let a, let b else { return false }; return a.lowercased() == b.lowercased()
    }
    private static func vector(_ value: DocumentValue, count: Int) throws -> [Double] {
        guard let values = value.items, values.count == count,
              values.allSatisfy({ $0.double?.isFinite == true }) else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        return values.compactMap(\.double)
    }
    static func fixtureRequest(_ input: DocumentValue, frame: ViewportFrame) throws -> SharedFurnishingViewportRequest {
        guard case .object(let fields) = input, Set(fields.keys) == ["lightId", "pose"],
              let id = input["lightId"].string, UUID(uuidString: id) != nil,
              let lights = frame.draft.candidate["lights"].items,
              lights.filter({ $0["id"].string == id && $0["type"] == .string("spot") }).count == 1 else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
        let pose = try FurnishingPose(input["pose"])
        let command = try SharedFurnishingCommand(host: .light, hostId: id, pose: pose)
        return try SharedFurnishingViewportRequest(frame: frame, command: command, selectedInspectorId: id)
    }
    enum FixturePreview: Sendable {
        case existing(NativeAutomationScene.ScenePreview)
        case changed(SharedFurnishingPreview)
        func apply(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
            switch self {
            case .existing(let preview): return try preview.apply(to: draft)
            case .changed(let preview): return try preview.apply(to: draft, command: preview.command)
            }
        }
    }
    static func fixtureReply(_ reply: DocumentValue, request: SharedFurnishingViewportRequest) throws -> FixturePreview {
        // The server explicitly returns unchanged input for an exact no-op;
        // generic GUI preview marks template modified even for such a reply.
        if reply["candidate"] == request.frame.draft.candidate {
            let keys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
            guard case .object(let fields) = reply, Set(fields.keys) == keys.union(["candidate"]),
                  try JSONEncoder().encode(reply).count <= 1_100_000,
                  keys.allSatisfy({ reply[$0] == request.value[$0] }),
                  try SharedFurnishingPreview.pose(in: request.frame.draft, host: .light, id: request.command.hostId) == request.command.pose else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            return .existing(.init(base: request.frame.draft, candidate: request.frame.draft.candidate))
        }
        return .changed(try request.checkedReply(reply))
    }

    struct AttachmentRequest: Sendable {
        let frame: ViewportFrame
        let value: DocumentValue
        let lightId: String
        let expected: DocumentValue
        let expectedPosition: [Double]
        let expectedRotation: [Double]
        let unchanged: Bool

        init(_ input: DocumentValue, frame: ViewportFrame) throws {
            guard !frame.disabled, frame.sequence > 0,
                  [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
                  case .object(let fields) = input, Set(fields.keys) == ["lightId", "attachment"],
                  let id = input["lightId"].string, UUID(uuidString: id) != nil,
                  case .object(let attachment) = input["attachment"], Set(attachment.keys) == ["surfaceId", "anchor", "gap"],
                  let surfaceId = input["attachment"]["surfaceId"].string, UUID(uuidString: surfaceId) != nil,
                  let gap = input["attachment"]["gap"].double, gap.isFinite, (0.01...1).contains(gap),
                  var lights = frame.draft.candidate["lights"].items else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let anchor = try NativeAutomationFixture.vector(input["attachment"]["anchor"], count: 2)
            let matches = lights.indices.filter { NativeAutomationFixture.same(lights[$0]["id"].string, id) }
            guard matches.count == 1, let index = matches.first, let actualId = lights[index]["id"].string,
                  ["point", "spot", "area"].contains(lights[index]["type"].string ?? ""),
                  let surfaces = frame.draft.candidate["surfaces"].items else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let found = surfaces.filter { NativeAutomationFixture.same($0["id"].string, surfaceId) }
            guard found.count == 1, let surface = found.first, surface["type"] == .string("ceiling"),
                  NativeAutomationFixture.same(surface["roomId"].string, lights[index]["roomId"].string),
                  let actualSurface = surface["id"].string,
                  let width = surface["dimensions"]["width"].double, width.isFinite, width > 0,
                  let height = surface["dimensions"]["height"].double, height.isFinite, height > 0,
                  abs(anchor[0]) <= width / 2, abs(anchor[1]) <= height / 2 else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let origin = try NativeAutomationFixture.vector(surface["transform"]["position"], count: 3)
            let q = try NativeAutomationFixture.vector(surface["transform"]["rotation"], count: 4)
            let scale = try NativeAutomationFixture.vector(surface["transform"]["scale"], count: 3)
            guard scale.allSatisfy({ abs($0 - 1) <= 1e-5 }), abs(q.reduce(0) { $0 + $1 * $1 } - 1) <= 1e-5 else { throw BridgeFailure("AUTOMATION_COMMAND_INVALID") }
            let rotation = simd_quatd(ix: q[0], iy: q[1], iz: q[2], r: q[3])
            let point = rotation.act(SIMD3(anchor[0] * scale[0], anchor[1] * scale[1], gap * scale[2])) + SIMD3(origin[0], origin[1], origin[2])
            let targetPosition = [point.x, point.y, point.z], targetRotation = [-q[2], q[3], q[0], -q[1]]
            var expected = frame.draft.candidate
            var extensions: [String: DocumentValue]
            if expected["extensions"] == .null { extensions = [:] }
            else if case .object(let value) = expected["extensions"] { extensions = value }
            else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            var bindings: [DocumentValue] = []
            if let previous = extensions[NativeAutomationFixture.namespace] {
                guard case .object(let ext) = previous, Set(ext.keys) == ["version", "lights"], previous["version"] == .integer(1),
                      let rows = previous["lights"].items, rows.count <= 32 else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
                bindings = rows
            }
            let old = bindings.filter { NativeAutomationFixture.same($0["lightId"].string, actualId) }
            guard old.count <= 1 else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            let binding: DocumentValue = .object(["lightId": .string(actualId), "surfaceId": .string(actualSurface), "anchor": .vector(anchor), "gap": .number(gap)])
            unchanged = old.first == binding
            if !unchanged {
                bindings.removeAll { NativeAutomationFixture.same($0["lightId"].string, actualId) }
                bindings.append(binding)
                guard bindings.count <= 32 else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
                extensions[NativeAutomationFixture.namespace] = .object(["version": .integer(1), "lights": .array(bindings)])
                expected["extensions"] = .object(extensions)
                lights[index]["transform"] = .object(["position": .vector(targetPosition), "rotation": .vector(targetRotation), "scale": .vector([1, 1, 1])])
                expected["lights"] = .array(lights)
            }
            expected = try RoomBaseLightIntent.manualize(expected, lightId: actualId)
            self.frame = frame; lightId = actualId; self.expected = expected
            expectedPosition = targetPosition; expectedRotation = targetRotation
            value = .object(["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased()),
                "type": .string("set-light-attachment"), "lightId": .string(actualId),
                "attachment": .object(["surfaceId": .string(actualSurface), "anchor": .vector(anchor), "gap": .number(gap)])])
        }
        func checkedReply(_ reply: DocumentValue) throws -> NativeAutomationScene.ScenePreview {
            let keys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
            guard case .object(let fields) = reply, Set(fields.keys) == keys.union(["lightId", "candidate"]),
                  try JSONEncoder().encode(reply).count <= 1_100_000,
                  keys.allSatisfy({ reply[$0] == value[$0] }), reply["lightId"] == .string(lightId) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            var comparison = expected
            if !unchanged {
                guard var rows = comparison["lights"].items, let index = rows.firstIndex(where: { $0["id"].string == lightId }),
                      let actual = reply["candidate"]["lights"].items?.first(where: { $0["id"].string == lightId }) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
                let position = try NativeAutomationFixture.vector(actual["transform"]["position"], count: 3)
                let rotation = try NativeAutomationFixture.vector(actual["transform"]["rotation"], count: 4)
                guard zip(position, expectedPosition).allSatisfy({ abs($0.0 - $0.1) <= 1e-9 }),
                      zip(rotation, expectedRotation).allSatisfy({ abs($0.0 - $0.1) <= 1e-9 }) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
                rows[index]["transform"]["position"] = actual["transform"]["position"]
                rows[index]["transform"]["rotation"] = actual["transform"]["rotation"]
                comparison["lights"] = .array(rows)
            }
            guard reply["candidate"] == comparison else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            return .init(base: frame.draft, candidate: reply["candidate"])
        }
    }
}
