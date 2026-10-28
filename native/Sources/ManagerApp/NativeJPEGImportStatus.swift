// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore

enum NativeJPEGImportStatus {
    static func message(for code: String) -> String? {
        switch code {
        case "JPEG_IMPORT_UNAVAILABLE": "이 실행 패키지의 JPEG 가져오기는 아직 준비되지 않았습니다. 원본을 유지하고 GLB·PNG 파일을 선택하세요."
        case "JPEG_PROFILE_UNSUPPORTED": "이 JPEG의 색상·회전 정보를 안전하게 처리할 수 없습니다. 원본은 유지됩니다. PNG로 내보낸 파일을 선택하세요."
        case "JPEG_LIMIT": "이 JPEG를 지원하는 크기의 감상용 PNG로 준비하지 못했습니다. 원본을 유지하고 크기를 조정해 내보낸 PNG를 선택하세요."
        case "JPEG_PREPARATION_FAILED": "JPEG 파일을 준비하지 못했습니다. 원본과 저장한 정보는 유지됩니다. 파일을 확인하거나 PNG로 내보낸 파일을 선택하세요."
        default: nil
        }
    }
}
