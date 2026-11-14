// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Existing cameraInteractionV1 mapping. Object rotation remains a different editor mode.
public enum ViewportCameraInteractionMode: String, Codable, Sendable {
    case object
    case cameraRotate = "camera-rotate"
    case cameraPan = "camera-pan"
}

public struct ViewportCameraInteractionRequest: Encodable, Sendable {
    public static let capability = "cameraInteractionV1"
    public let epoch: String
    public let sequence: Int
    public let requestId: String
    public let mode: ViewportCameraInteractionMode

    public init(frame: ViewportFrame, mode: ViewportCameraInteractionMode,
                requestId: String = UUID().uuidString.lowercased()) throws {
        guard frame.presentation == .threeD, !frame.disabled,
              UUID(uuidString: frame.epoch)?.uuidString.lowercased() == frame.epoch,
              frame.sequence > 0, frame.sequence <= 9_007_199_254_740_991,
              UUID(uuidString: requestId)?.uuidString.lowercased() == requestId else {
            throw AuthoringFailure("CAMERA_INTERACTION_INVALID")
        }
        epoch = frame.epoch; sequence = frame.sequence; self.requestId = requestId; self.mode = mode
    }

    /// This ACK confirms only the input mapping, never focus, motion or appearance.
    public func matches(_ value: DocumentValue) -> Bool {
        guard case .object(let fields) = value,
              Set(fields.keys) == ["epoch", "sequence", "requestId", "mode", "accepted"] else { return false }
        return value["epoch"] == .string(epoch) && value["sequence"] == .integer(Int64(sequence))
            && value["requestId"] == .string(requestId) && value["mode"] == .string(mode.rawValue)
            && value["accepted"] == .bool(true)
    }
}

public enum ViewportCameraInteractionReset {
    /// Envelope owner/sequence is checked by the host before this body is used.
    public static func valid(_ value: DocumentValue) -> Bool {
        guard case .object(let fields) = value, Set(fields.keys) == ["mode", "reason"],
              value["mode"] == .string("object"), let reason = value["reason"].string else { return false }
        return reason == "hidden" || reason == "unavailable"
    }
}
