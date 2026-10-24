// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum InstallationCommandReason: String, CaseIterable, Sendable {
    case placementNotFound = "INSTALLATION_PLACEMENT_NOT_FOUND"
    case roomMismatch = "INSTALLATION_ROOM_MISMATCH"
    case sourceNotPlanar = "INSTALLATION_SOURCE_NOT_PLANAR"
    case singularTransform = "INSTALLATION_SINGULAR_TRANSFORM"
    case missingRoomFloor = "INSTALLATION_MISSING_ROOM_FLOOR"
    case ambiguousRoomFloor = "INSTALLATION_AMBIGUOUS_ROOM_FLOOR"
    case missingSelectedWall = "INSTALLATION_MISSING_SELECTED_WALL"
    case wallNotParallel = "INSTALLATION_WALL_NOT_PARALLEL"
    case limitsExceeded = "INSTALLATION_LIMITS_EXCEEDED"
    public static func fromJavaScriptMessage(_ message: String) -> Self? {
        guard message.utf8.count <= 180 else { return nil }
        let code = message.hasPrefix("Error: ") ? String(message.dropFirst(7)) : message
        return Self(rawValue: code)
    }
    public var explanation: String {
        switch self {
        case .placementNotFound: "작품 배치를 다시 선택하세요. 현재 입력은 유지됩니다."
        case .roomMismatch: "작품과 같은 방의 바닥이나 벽을 선택하세요."
        case .sourceNotPlanar: "이 작품 형상에는 이 설치 조작을 적용할 수 없습니다. 설정과 원본은 유지됩니다."
        case .singularTransform: "작품 변환 상태를 확인하세요. 현재 배치는 유지됩니다."
        case .missingRoomFloor: "작품 아래의 기준 바닥을 찾지 못했습니다. 바닥을 선택하세요."
        case .ambiguousRoomFloor: "작품 아래에 바닥이 여러 개 있습니다. 기준 바닥을 직접 선택하세요."
        case .missingSelectedWall: "간격을 맞출 기준 벽을 선택하세요."
        case .wallNotParallel: "작품이 선택한 벽과 평행해야 간격을 맞출 수 있습니다."
        case .limitsExceeded: "설치 치수나 위치가 지원 범위를 넘습니다. 입력과 작품 배치를 확인하세요."
        }
    }
}
