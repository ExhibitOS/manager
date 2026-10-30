// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum InstallationCommand: Sendable {
    case centerArtwork(placementId: String)
    case onTop(placementId: String)
    case alignOnVerifiedPlinth(placementId: String)
    case installHeight(placementId: String, height: Double, floorSurfaceId: String?)
    case wallGap(placementId: String, gap: Double, wallSurfaceId: String)
    public var placementId: String {
        switch self {
        case .centerArtwork(let id), .onTop(let id), .alignOnVerifiedPlinth(let id), .installHeight(let id, _, _), .wallGap(let id, _, _): id
        }
    }
    public var translationAxes: Set<Int> {
        switch self { case .centerArtwork: [0, 2]; case .onTop, .installHeight: [1]; case .wallGap, .alignOnVerifiedPlinth: [0, 1, 2] }
    }
    public func encodedValue() throws -> DocumentValue {
        guard UUID(uuidString: placementId) != nil else { throw AuthoringFailure("INSTALLATION_SETTINGS_INVALID") }
        var result: DocumentValue = .object(["type": .string("installation-command"), "placementId": .string(placementId)])
        switch self {
        case .centerArtwork: result["command"] = .string("center-artwork")
        case .onTop: result["command"] = .string("on-top")
        case .alignOnVerifiedPlinth: result["command"] = .string("align-on-verified-plinth")
        case .installHeight(_, let height, let floor):
            guard height.isFinite, (0...10).contains(height), floor == nil || UUID(uuidString: floor!) != nil else { throw AuthoringFailure("INSTALLATION_SETTINGS_INVALID") }
            result["command"] = .string("install-height"); result["height"] = .number(height)
            if let floor { result["floorSurfaceId"] = .string(floor) }
        case .wallGap(_, let gap, let wall):
            guard gap.isFinite, (0...1).contains(gap), UUID(uuidString: wall) != nil else { throw AuthoringFailure("INSTALLATION_SETTINGS_INVALID") }
            result["command"] = .string("wall-gap"); result["gap"] = .number(gap); result["wallSurfaceId"] = .string(wall)
        }
        return result
    }
}
