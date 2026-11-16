// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Wire constants and structural checks for the one articulated catalogue.
public enum NativeTrackSpotAim {
    public static let profile = "track-spot-aim-v1"
    public static let catalogue = "museum-track-spot-articulated"
    static let pose: DocumentValue = .object(["position": .vector([0, 0.15, 0.0755]), "yaw": .integer(0), "scale": .integer(1)])

    static func check(_ value: DocumentValue) throws {
        let invalid = AuthoringFailure("LIGHT_FIELDS_INVALID")
        func vector(_ value: DocumentValue) -> Bool {
            value.items?.count == 3 && value.items?.allSatisfy { $0.double?.isFinite == true } == true
        }
        func closed(_ value: DocumentValue, _ keys: Set<String>) -> Bool {
            guard case .object(let fields) = value else { return false }
            return Set(fields.keys) == keys
        }
        func id(_ value: DocumentValue) -> Bool {
            value.string.flatMap(UUID.init(uuidString:)) != nil
        }
        func angle(_ value: DocumentValue, in range: ClosedRange<Double>) -> Bool {
            value.double.map { $0.isFinite && range.contains($0) } == true
        }
        let mount = value["mount"], aim = value["aim"]
        guard closed(value, ["version", "profile", "mount", "aim"]),
              value["version"] == .integer(1), value["profile"] == .string(profile),
              closed(mount, ["railPlacementId", "position", "yaw"]),
              id(mount["railPlacementId"]), vector(mount["position"]), angle(mount["yaw"], in: -.pi ... .pi) else { throw invalid }
        switch aim["kind"].string {
        case "placement-center":
            guard closed(aim, ["kind", "placementId", "offset"]), id(aim["placementId"]), vector(aim["offset"]) else { throw invalid }
        case "point":
            guard closed(aim, ["kind", "position"]), vector(aim["position"]) else { throw invalid }
        case "manual":
            guard closed(aim, ["kind", "yaw", "pitch"]), angle(aim["yaw"], in: -.pi ... .pi),
                  angle(aim["pitch"], in: -.pi ... .pi) else { throw invalid }
        default: throw invalid
        }
    }

    public static func fixtureCommand(lightId: String, binding: DocumentValue) throws -> DocumentValue {
        try check(binding)
        return .object(["type": .string("set-light-fixture"), "lightId": .string(lightId),
            "visual": .object(["catalogId": .string(catalogue), "catalogVersion": .integer(1), "pose": pose, "trackAim": binding])])
    }

    public static func binding(in draft: NativeExhibitionDraft, lightId: String) -> DocumentValue {
        draft.candidate["extensions"][NativeLightFixture.namespace]["lights"].items?
            .first { $0["lightId"].string?.lowercased() == lightId.lowercased() }?["trackAim"] ?? .null
    }

    public static func updateCommand(in draft: NativeExhibitionDraft, lightId: String, binding: DocumentValue,
                                     shadowPriority: String, prefersShadow: Bool) throws -> DocumentValue {
        try check(binding)
        guard case .object(var fields) = draft.candidate["lights"].items?.first(where: { $0["id"].string == lightId }) ?? .null else {
            throw AuthoringFailure("LIGHT_FIELDS_INVALID")
        }
        fields.removeValue(forKey: "id")
        fields["targetPlacementId"] = shadowPriority.isEmpty ? nil : .string(shadowPriority)
        fields["castsShadow"] = .bool(prefersShadow)
        var settings = try NativeLightFixture.settings(in: draft, lightId: lightId)
        settings["trackAim"] = binding
        return .object(["type": .string("set-light-object"), "lightId": .string(lightId),
                        "light": .object(fields), "settings": settings])
    }

    public static func requiresCapability(_ command: DocumentValue) -> Bool {
        command["visual"]["catalogId"] == .string(catalogue) || command["settings"]["trackAim"] != .null
    }
}

/// Exact producer diagnostics, scoped by AuthoringAPI to a negotiated proposal.
public struct NativeTrackSpotAimDiagnostic: Sendable, Equatable {
    public let reason: String
    public let remedy: String

    static func parse(_ message: String?) -> Self? {
        guard let message, messages.contains(message),
              let separator = message.range(of: ": ") else { return nil }
        return Self(reason: String(message[..<separator.lowerBound]), remedy: String(message[separator.upperBound...]))
    }

    private static let messages: Set<String> = [
        "TRACK_SPOT_AIM_INVALID_BINDING: Choose the known articulated fixture and an explicit rail mount and aiming intent.",
        "TRACK_SPOT_AIM_INVALID_BINDING: Restore the explicit mount and target references and finite rigid transforms.",
        "TRACK_SPOT_AIM_INVALID_BINDING: The articulated mount replaces the legacy emitter attachment",
        "TRACK_SPOT_AIM_INVALID_BINDING: Use the known articulated fixture with one explicit mount/aim and no legacy aim or base-light intent",
        "TRACK_SPOT_AIM_INVALID_BINDING: Use the source optical datum; edit the explicit mount instead of the visual offset.",
        "TRACK_SPOT_AIM_INVALID_ENVELOPE: Supply finite ordered verified yaw and pitch intervals.",
        "TRACK_SPOT_AIM_INVALID_INPUT: Supply a finite target or manual angles and finite previous angles.",
        "TRACK_SPOT_AIM_INVALID_INPUT: Supply finite source geometry datums.",
        "TRACK_SPOT_AIM_INVALID_MOUNT: Supply a finite unit-scale rigid mount pose.",
        "TRACK_SPOT_AIM_INVALID_MOUNT: Supply a unit XYZW mount quaternion.",
        "TRACK_SPOT_AIM_INVALID_RAIL: Choose an admitted passive ceiling rail.",
        "TRACK_SPOT_AIM_INVALID_RAIL: Choose an admitted unit-scale ceiling rail in the light room.",
        "TRACK_SPOT_AIM_INVALID_RAIL: Restore the admitted rail descriptor and its ceiling attachment.",
        "TRACK_SPOT_AIM_INVALID_SOURCE: Restore the exact supported articulated catalogue identity.",
        "TRACK_SPOT_AIM_INVALID_SOURCE: Restore the exact supported articulated source identity.",
        "TRACK_SPOT_AIM_INVALID_SOURCE: Select one exact articulated visual for this light.",
        "TRACK_SPOT_AIM_INVALID_SOURCE: Select the exact supported articulated catalogue source.",
        "TRACK_SPOT_AIM_INVALID_TARGET: Choose an existing artwork placement or point in the same room.",
        "TRACK_SPOT_AIM_NUMERICAL_FAILURE: Use coordinates with finite rigid-transform arithmetic.",
        "TRACK_SPOT_AIM_NUMERICAL_FAILURE: Use source datums and coordinates that resolve the requested target within numerical tolerance.",
        "TRACK_SPOT_AIM_OUTSIDE_ENVELOPE: Choose joint angles inside the verified source envelope.",
        "TRACK_SPOT_AIM_OUTSIDE_ENVELOPE: Move the target or mount so the requested direction fits the verified source envelope.",
        "TRACK_SPOT_AIM_OUTSIDE_ROOM: Move the mount or target so every articulated part stays inside the room.",
        "TRACK_SPOT_AIM_RAIL_CONTACT: Align the adapter to the selected rail and keep its complete footprint in contact.",
        "TRACK_SPOT_AIM_RAIL_CONTACT: Move the adapter onto a verified contact region of the selected rail.",
        "TRACK_SPOT_AIM_TARGET_AT_OR_BEHIND_APERTURE: Move the target beyond the optical aperture.",
        "TRACK_SPOT_AIM_UNSUPPORTED_GEOMETRY: Provide certified coaxial Y-swivel/X-pitch geometry with the aperture along the head -Z axis.",
        "TRACK_SPOT_AIM_ZERO_TARGET_DIRECTION: Move the target away from the head pivot.",
    ]
}
