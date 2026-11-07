// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit

public struct NativeBoundedOEXJob: Decodable, Sendable {
    public let id: String
    public let state: String
}

public struct AuthoringSession: Decodable, Sendable {
    public let userId: String
    public let tenantId: String
    public let role: String
    public let csrfToken: String
    public let expiresAt: String
    public var nativeOexExportProfile: String? = nil
    public var nativePublicationProfile: String? = nil
    public var workspaceMaterialProfile: String? = nil
    public var workspaceFurnishingProfile: String? = nil
    public var workspaceFurnishingStoolProfile: String? = nil
    public var workspaceFurnishingRoomVisualProfile: String? = nil
    public var workspaceArtworkFrameProfile: String? = nil

    public var advertisesWorkspaceFurnishingStool: Bool {
        workspaceFurnishingProfile == "workspace-furnishing-intake-v1"
            && workspaceFurnishingStoolProfile == "workspace-furnishing-stool-v1"
    }

    public var advertisesWorkspaceFurnishingRoomVisual: Bool {
        advertisesWorkspaceFurnishingStool
            && workspaceFurnishingRoomVisualProfile == "workspace-furnishing-room-visual-v1"
    }

    public var advertisesWorkspaceArtworkFrame: Bool {
        advertisesWorkspaceFurnishingRoomVisual && workspaceArtworkFrameProfile == "workspace-artwork-frame-v1"
    }
}
public struct AuthoringPage<Item: Decodable & Sendable>: Decodable, Sendable {
    public let items: [Item]
    public let nextCursor: String?
}
public struct NativeArtist: Codable, Sendable, Identifiable {
    public let id: String
    public let userId: String?
    public let revision: Int
    public let name: String
    public let bio: String
    public let archived: Bool
}
public struct ArtworkSummary: Codable, Sendable, Identifiable {
    public let id: String
    public let artistId: String
    public let revision: Int
    public let title: String
    public let archived: Bool
    public let approvedRevision: Int?
}
public struct ArtworkMetadata: Codable, Equatable, Sendable {
    public struct Dimensions: Codable, Equatable, Sendable {
        public var width = 1.0, height = 1.0
        public var depth: Double?
        public var unit = "m"
    }
    public struct Rights: Codable, Equatable, Sendable {
        public struct Permissions: Codable, Equatable, Sendable {
            public var display = true, download = false, export = false, commercial = false
        }
        public var holder = ""
        public var ownership = "owner"
        public var licenseId: String?
        public var licenseText: String? = "전시만 허용. 원본 다운로드와 내보내기 허용 없음."
        public var permissions = Permissions()
        public var creditLine = ""
        public var validFrom: String?
        public var expiresAt: String?
    }
    public struct Provenance: Codable, Equatable, Sendable {
        public var source = "human-authored"
        public var sourceUnits = "m"
        public var scaleApplied = false
        public var notes = ""
    }
    public struct DisplayReference: Codable, Equatable, Sendable {
        public struct Bounds: Codable, Equatable, Sendable {
            public let min: [Double], max: [Double]
        }
        public struct Pose: Codable, Equatable, Sendable {
            public let position: [Double], rotation: [Double], scale: [Double]
        }
        public let version: Int
        public let status: String
        public let assetSha256: String
        public let rawBounds: Bounds
        public let referenceHeightMeters: Double
        public let pose: Pose
    }
    public var title = "", description = ""
    public var medium: String?
    public var creationYear: Int?
    public var dimensions = Dimensions()
    public var rights = Rights()
    public var provenance = Provenance()
    public var displayReference: DisplayReference?
    public init() {}
}
public struct NativeArtwork: Codable, Sendable, Identifiable {
    public struct Asset: Codable, Sendable, Identifiable {
        public let id: String, mime: String, state: String, sha256: String
        public let bytes: Int64
    }
    public let id: String, artistId: String
    public let revision: Int
    public let approvedRevision: Int?
    public let approvedAssetId: String?
    public let metadata: ArtworkMetadata?
    public let assets: [Asset]
    public let archived: Bool
}
public struct AuthoringFailure: Error, LocalizedError, Sendable {
    public let status: Int
    public let code: String
    public let role: String?
    public let field: String?
    public init(_ code: String, status: Int = 0, role: String? = nil, field: String? = nil) {
        self.code = code; self.status = status
        self.role = role.flatMap(WorkspaceMaterialRole.init(rawValue:))?.rawValue
        self.field = Self.safeField(field) ? field : nil
    }
    static func safeCode(_ code: String) -> Bool {
        code.range(of: "^[A-Z0-9_]{1,80}$", options: .regularExpression) != nil
    }
    static func safeField(_ field: String?) -> Bool {
        field.map { $0.range(of: "^[A-Za-z][A-Za-z0-9_.-]{0,63}$", options: .regularExpression) != nil } ?? true
    }
    public var errorDescription: String? {
        switch code {
        case "ARTWORK_DISPLAY_REFERENCE_INVALID": return "원본 단위 미확인 작품의 임시 표시 기준과 파일을 확인하세요. 배치 위치는 숫자로 입력할 수 있습니다."
        case "EXHIBITION_COMPARISON_STALE": return "저장본이 비교 후 바뀌었습니다. 현재 입력은 유지됩니다. 저장본을 다시 비교하세요."
        case "ARCHITECTURE_FIELDS_INVALID": return "공간 이름·크기·위치를 확인하세요. 크기는 0보다 큰 값, 위치는 숫자로 입력하세요."
        case "FURNISHING_FIELDS_INVALID": return "받침의 위치·회전·비례 크기를 확인하세요. 위치는 미터, 회전은 −180–180도, 비율은 0.1–4입니다. 입력과 기존 전시는 유지됩니다."
        case "FURNISHING_INVALID", "FURNISHING_CATALOG_INVALID": return "현재 지원하는 받침 정보를 확인하지 못했습니다. 기존 받침과 입력은 유지됩니다."
        case "FURNISHING_PREVIEW_FAILED": return "받침을 미리 보지 못했습니다. 받침 전체가 공간 안에 들어가는 위치와 크기를 확인하세요. 기존 전시와 입력은 유지됩니다."
        case "LIGHT_FIELDS_INVALID": return "조명 입력을 확인하세요. 밝기는 0–1000000, 선형 RGB는 0–1, 빔 전체각은 0보다 크고 90도 이하, 면광원 크기는 0.01–100m입니다. 입력과 기존 설정은 유지됩니다."
        case "LIGHT_SELECTION_CHANGED": return "선택한 조명을 다시 확인하세요. 현재 입력과 저장된 설정은 유지됩니다."
        case "LIGHT_PREVIEW_FAILED": return "조명 미리보기를 확인하지 못했습니다. 조명이 속한 공간 안의 위치와 입력, 로컬 제작 연결을 확인하세요. 입력과 저장된 설정은 유지됩니다."
        case "SURFACE_MATERIAL_INVALID": return "색상과 재질 값을 확인하세요. 색상 코드는 #rrggbb, 거칠기와 금속성은 0부터 1까지 입력하세요. 입력과 기존 설정은 유지됩니다."
        case "SURFACE_MATERIAL_TEXTURE_INVALID": return "무늬 반복은 0.05–20 m, 회전은 −180–180도, 요철 강도는 0–2로 입력하세요. 입력과 기존 재질은 유지됩니다."
        case "MATERIAL_CATALOG_INVALID": return "재질 목록을 확인하지 못했습니다. 기존 재질과 입력은 유지됩니다."
        case "MATERIAL_DECLARATION": return "재질 이름, 출처와 권리 입력을 확인하세요."
        case "WORKSPACE_MATERIAL_BINDING_INVALID": return "바닥과 반복 크기, 회전 값을 확인하세요."
        case "WORKSPACE_MATERIAL_BINDING_UNSUPPORTED": return "이 표면이나 재질 맵 조합은 현재 런타임에서 연결할 수 없습니다."
        case "WORKSPACE_MATERIAL_BINDING_SAMPLING_UNSUPPORTED": return "이 바닥에 기존 요철 또는 주변광 차폐 설정이 있어 다른 재질로 교체할 수 없습니다."
        case "WORKSPACE_MATERIAL_SAVED_DRAFT_REQUIRED": return "현재 전시를 저장한 뒤 재질을 연결하세요."
        case "NATIVE_SURFACE_CAPABILITY_UNAVAILABLE": return "현재 저장본의 편집 정책에서는 바닥 재질을 연결할 수 없습니다."
        case "NATIVE_DOCUMENT_STATE_CHANGED": return "저장 문서가 바뀌었습니다. 전시를 다시 확인하세요."
        case "MATERIAL_INTAKE_LIMIT": return "진행 중인 재질 등록이 한도에 도달했습니다. 기존 등록을 마친 뒤 다시 시도하세요."
        case "MATERIAL_MAP_DESCRIPTOR", "MATERIAL_MAP_DIMENSIONS", "PNG_PIXEL_LIMIT_EXCEEDED", "FORMAT_REJECTED":
            return "크기가 8 MiB 이하이고 픽셀이 419만 개 이하인 8비트 RGB 또는 RGBA PNG를 선택하세요."
        case "MATERIAL_MAP_COLOR_SPACE": return "색상 맵은 sRGB, 데이터 맵은 선형 색 공간의 PNG를 선택하세요."
        case "DISPLAY_PERMISSION_REQUIRED": return "이 재질의 전시 권한을 확인하세요."
        case "WORKSPACE_MATERIAL_UNAVAILABLE": return "이 작업공간에서 재질을 등록할 수 없습니다. 연결 상태를 확인하세요."
        case "ROLE_CONFLICT", "MODEL_CONFLICT", "UPLOAD_INTEGRITY": return "선언한 파일과 받은 파일이 다릅니다. 원래 파일로 등록을 이어가거나 새로 등록하세요."
        case "FURNISHING_DECLARATION": return "비품 이름, 출처, 권리와 32 MiB 이하 GLB 파일을 확인하세요."
        case "WORKSPACE_FURNISHING_UNAVAILABLE": return "현재 작업공간에서 이 비품을 등록하거나 배치할 수 없습니다."
        case "FURNISHING_VALIDATION_UNAVAILABLE": return "비품 검증이 중단됐습니다. 등록 기록에서 상태를 확인하고 검증을 다시 시도하세요."
        case "WORKSPACE_FURNISHING_INVALID": return "선택한 공간과 비품의 위치, 방향, 크기를 확인하세요."
        case "FRAME_SAVED_DRAFT_REQUIRED": return "전시를 저장한 뒤 액자를 연결하거나 해제하세요."
        case "FRAME_TARGET_STALE": return "저장된 작품 배치가 바뀌었습니다. 전시를 다시 열고 작품을 선택하세요."
        case "FRAME_ASSET_UNSUPPORTED": return "등록된 얇은 황동 액자와 승인된 평면 이미지를 선택하세요."
        case "FRAME_SIZE_UNSUPPORTED": return "폭 60 cm, 높이 80 cm로 승인된 작품을 선택하세요."
        case "FRAME_DEPTH_UNSUPPORTED": return "깊이 정보가 없는 평면 작품을 선택하세요."
        case "FRAME_TRANSFORM_UNSUPPORTED": return "작품 배치를 원래 크기로 돌린 뒤 전시를 저장하세요."
        case "FRAME_PROCEDURAL_CONFLICT": return "기존 액자 설정을 해제하고 전시를 저장한 뒤 다시 시도하세요."
        case "ARCHITECTURE_SELECTION_CHANGED": return "선택한 대상이 바뀌었습니다. 입력을 취소한 뒤 대상을 다시 선택하세요."
        case "PLACEMENT_APPLY_REQUIRED": return "배치와 위치 입력을 먼저 적용하거나 취소하세요. 입력과 기존 전시는 그대로 유지됩니다."
        case "ARCHITECTURE_APPLY_REQUIRED": return "공간·재질 변경을 적용하거나 입력을 취소한 뒤 전시를 저장하세요."
        case "FILE_SIZE_INVALID": return "파일은 1바이트에서 32 MiB까지 지원합니다."
        case "FILE_FORMAT_UNSUPPORTED": return "GLB 또는 PNG 파일을 선택하세요."
        case "FILE_CHANGED": return "읽는 동안 파일이 바뀌었습니다. 저장을 마친 뒤 다시 선택하세요."
        case "FILE_READ_FAILED", "FILE_REGULAR_REQUIRED": return "파일을 읽을 수 없습니다. 파일 위치와 접근 권한을 확인하세요."
        case "FLOOR_APPLY_REQUIRED": return "바닥 설정을 공간에서 확인한 뒤 적용하거나 입력을 되돌려 주세요."
        case "FLOOR_SETTINGS_INVALID", "FLOOR_PRESET_REQUIRED": return "바닥 재질과 변경할 항목을 선택하고 입력 범위를 확인하세요."
        case "WALL_FINISH_INVALID", "WALL_FINISH_PREVIEW_FAILED": return "선택한 벽과 마감 수치를 확인하세요. 문·작품과의 여유 공간이 필요하며 기존 설정과 입력은 유지됩니다."
        case "FLOOR_TARGET_INVALID", "CONCRETE_FOOTSTEPS_INVALID": return "실제 바닥 표면을 선택하고 기존 발소리 설정을 확인하세요. 바닥 외관과 다른 소리는 유지됩니다."
        case "FLOOR_SOUND_CONFLICT": return "선택한 바닥에 재질 소리 설정이 있습니다. 기존 설정을 바꾸지 않고 콘크리트 발소리 선택을 중단했습니다."
        case "EXHIBITION_EXPORT_SAVE_REQUIRED": return "현재 입력을 전시 저장본에 반영한 뒤 내보내세요."
        case "VIEWER_ENTRANCE_INVALID": return "시작 화면의 작가·소개·이미지 설정을 확인하세요. 작가는 512자, 소개는 4,096자, 입장권 제목은 160자, 안내는 512자, 이미지 설명은 2,048자까지 지원하며, 표지는 전시에 포함한 8 MiB 이하 PNG와 설명이 필요합니다."
        case "PUBLICATION_REVIEW_REQUIRED": return "현재 저장본을 확인하고 작품의 표시 권리를 확인하세요."
        case "PUBLICATION_PLAN_CHANGED": return "공개 준비를 시작한 실행본 또는 설정과 현재 상태가 다릅니다. 보관한 요청·식별자와 이전 공개본은 유지했습니다. 같은 준비 상태에서 다시 확인하세요."
        case "WATERMARK_FIELDS_INVALID": return "워터마크 문구·위치·크기·진하기를 확인하세요. 크기는 8–24 px, 불투명도는 0.1–0.6입니다."
        case "WATERMARK_TEXT_UNSUPPORTED": return "사본 삽입은 지원하는 영문·숫자·기호 80자까지 사용합니다. 다른 언어는 화면 표시를 선택하세요."
        case "WATERMARK_RUNTIME_UNAVAILABLE": return "이 실행본은 새 워터마크 설정을 지원하지 않습니다. 입력과 저장 요청을 보존했습니다. 지원 실행본에서 다시 확인하세요."
        case "PUBLICATION_RESPONSE_INVALID": return "공개 준비 상태를 확인하지 못했습니다. 같은 요청으로 다시 확인하세요."
        case "PUBLICATION_SCOPE_CHANGED", "PUBLICATION_STATUS_UNCONFIRMED": return "이 Mac의 준비 상태를 다시 확인하세요. 제작 중인 전시와 이전 기록은 유지됩니다."
        case "PUBLICATION_STAGE_UNAVAILABLE": return "현재 실행본에서 이 공개 준비 방식을 사용할 수 없습니다. 보관한 요청과 전시는 유지됩니다."
        case "PUBLICATION_NOT_READY": return "공개 준비에 필요한 항목이 남아 있습니다. 작품의 승인·표시 권리와 전시 정보를 확인하세요."
        case "PUBLISHED_DRAFT_IMMUTABLE": return "이 공개용 저장본은 변경할 수 없습니다. 원래 제작 전시에서 편집하세요."
        case "NATIVE_OEX_EXPORT_LIMIT": return "이 실행본의 대용량 OEX는 최대 512 MiB를 지원합니다. 원본과 기존 파일은 유지됩니다."
        case "NATIVE_OEX_EXPORT_BUSY": return "이전 내보내기 작업이 진행 중입니다. 종료를 확인한 뒤 다시 내보내세요."
        case "NATIVE_OEX_EXPORT_UNCERTAIN", "NATIVE_OEX_EXPORT_STAGE_UNCERTAIN", "NATIVE_OEX_EXPORT_FILE_UNCERTAIN": return "이전 내보내기의 종료를 아직 확인하지 못했습니다. 입력과 기존 파일은 유지됩니다. 다시 내보내면 같은 작업의 상태부터 확인합니다."
        case "NATIVE_OEX_EXPORT_UNAVAILABLE": return "현재 실행본에서 대용량 내보내기를 사용할 수 없습니다. 전시와 기존 파일은 보존됩니다."
        case "NATIVE_OEX_EXPORT_CAPACITY": return "내보내기에 필요한 저장 공간이 부족합니다. 전시와 기존 파일은 보존됩니다."
        case "OEX_LIMIT": return "내보내기 파일은 최대 64 MiB를 지원합니다. 전시의 작품 파일 크기를 확인하세요."
        case "RIGHTS_DENIED": return "내보내기를 허용하지 않은 작품이 있습니다. 작품의 권리와 내보내기 허용을 확인하세요."
        case "OEX_SOURCE_CHANGED", "ASSET_INTEGRITY": return "저장한 전시와 현재 작품 파일 또는 승인 상태가 다릅니다. 작품을 확인한 뒤 전시를 다시 저장하세요."
        case "OEX_PROFILE_INVALID", "OEX_EXTENSION_UNSUPPORTED", "OEX_LEGAL_NOTICE_INVALID", "OEX_INVALID": return "현재 전시에 OEX로 내보낼 수 없는 정보가 있습니다. 전시와 작품 정보를 확인하세요."
        case "OEX_RESPONSE_INVALID": return "내보내기 파일을 확인하지 못했습니다. 저장하지 않았습니다. 다시 시도하세요."
        case "OEX_FILE_REQUIRED": return "이 Mac의 OEX 파일을 선택하세요."
        case "OEX_FORMAT_INVALID": return "OEX 파일의 형식을 확인하지 못했습니다. 원본을 보관하고 지원되는 전시 파일인지 확인하세요."
        case "OEX_IMPORT_PROFILE_UNAVAILABLE": return "이 실행본은 해당 OEX 전송 방식을 아직 지원하지 않습니다. 원본과 작업 기록을 보관하세요."
        case "OEX_IMPORT_FILE_CHANGED": return "처음 선택한 OEX와 다른 파일입니다. 원래 파일을 다시 선택하세요."
        case "OEX_IMPORT_SCOPE_CHANGED": return "작업 공간이 바뀌어 가져오기를 멈췄습니다. 원래 작업 공간에서 다시 확인하세요."
        case "OEX_IMPORT_INVALID", "OEX_IMPORT_RESPONSE_INVALID": return "가져오기 상태를 확인하지 못했습니다. 원본 파일을 보관하고 다시 확인하세요."
        case "EXPORT_WRITE_FAILED": return "파일을 저장하지 못했습니다. 저장 위치와 여유 공간을 확인하고 다시 시도하세요."
        case "IMPORT_DERIVED_REQUIRED": return "같은 JPEG 원본을 다시 선택해 변환 파일을 준비하세요. 저장한 요청은 유지됩니다."
        case "IMPORT_CONTEXT_CHANGED": return "작업 공간이나 파일 검사 요청이 바뀌었습니다. 저장한 요청을 다시 확인하세요."
        case "IMPORT_DRAIN_PENDING": return "작품 파일 처리의 종료를 아직 확인하지 못했습니다. 저장한 요청과 원본을 유지한 채 상태를 확인하거나 로컬 제작을 정상 정지하세요."
        case "IMPORT_MODEL_BINDING_INVALID": return "감상용 파일 요청의 파일·설정·작업 식별 정보가 일치하지 않습니다. 저장한 원본과 요청을 유지했습니다."
        case "IMPORT_MODEL_UNAVAILABLE": return "이 실행본은 저장한 감상용 파일 요청을 지원하지 않습니다. 원본과 요청을 보존했으니 지원 실행본에서 다시 확인하세요."
        case "IMPORT_ICC_UNAVAILABLE": return "이 실행본은 저장한 ICC 변환 요청을 지원하지 않습니다. 원본과 요청을 보존했으니 지원 실행본에서 다시 확인하세요."
        case "IMPORT_INTENT_CHANGED": return "이전 검사 요청과 파일 또는 권리 입력이 다릅니다. 이전 요청을 완료하거나 취소한 뒤 새 검사를 시작하세요."
        case "IMPORT_FILE_REQUIRED": return "이전 요청과 같은 파일을 다시 선택하세요. 저장한 요청은 유지됩니다."
        default: break
        }
        return switch status {
        case 401: "작업 공간 연결이 만료되었습니다. 다시 연결하세요. 편집 중인 내용은 유지됩니다."
        case 403: "현재 작업 공간에서 이 작업을 수행할 권한이 없습니다. 입력은 유지됩니다."
        case 409, 412: "다른 곳에서 내용이 변경됐습니다. 입력을 유지하고 서버 내용과 비교하세요."
        case 400: "입력 형식과 권리 정보를 확인하세요."
        default: "제작 서비스에 연결하지 못했습니다. 입력을 유지하고 다시 시도하세요."
        }
    }
}
private struct APIErrorEnvelope: Decodable { let code: String; let role: String?; let field: String? }
private final class LocalRedirectBlocker: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

/// A private in-memory cookie jar per selected native installation. Callers get
/// typed CMS operations, never an arbitrary authenticated URL request API.
/// An exact streamed snapshot released only after the selected producer confirms
/// its response drain and known-stage cleanup for the same authenticated operation.
public struct NativeBoundedOEXExport: Sendable {
    private let archive: NativeOEXArchive
    public var count: Int { archive.count }
    public var sha256: String { archive.sha256 }
    fileprivate init(_ archive: NativeOEXArchive) { self.archive = archive }
    public func write(to url: URL) async throws { try await archive.writeBoundedVerified(to: url) }
}
private struct NativeOEXExportResponse: Sendable {
    let archive: NativeOEXArchive
    let operationId: String?
}
private struct NativeOEXExportCompletion: Decodable, Sendable {
    let operationId: String
    let state: String
    let etag: String?
    let bytes: Int?
    let sha256: String?
}

public enum NativeOEXExportProgress: Equatable, Sendable {
    case packing(completed: Int?, total: Int?)
    case checkingPackage
    case receiving(bytes: Int, total: Int?)
    case verifying
}

private struct NativeOEXPreparationStatus: Decodable {
    struct Progress: Decodable {
        let phase: String
        let completedItems: Int?
        let totalItems: Int?
    }
    let operationId: String
    let state: String
    let progress: Progress

    var update: NativeOEXExportProgress? {
        guard state == "pending" else { return nil }
        switch (progress.phase, progress.completedItems, progress.totalItems) {
        case ("packing", nil, nil): return .packing(completed: nil, total: nil)
        case ("packing", let completed?, let total?) where (1...256).contains(total) && (0...total).contains(completed):
            return .packing(completed: completed, total: total)
        case ("verifying", nil, nil): return .checkingPackage
        default: return nil
        }
    }
}

public actor AuthoringAPI {
    private let base: URL
    private let tenant: String
    private let transport: URLSession
    public nonisolated let admitsICCJPEG: Bool
    // These reviewed v5 tuples are local-validation payloads. Release admission
    // requires separate package qualification.
    #if DEBUG
    private static let workspaceMaterialManifestPins: Set<String> = [
        "40ca49e65e769c4c034eea69300ff65ce8af46ef9b081a9a2b4b1ade12ed9415",
        "cfe1faba63dc6d41e2451ad8bf8870d197a48e1b6d234850bdbad16c23083b59"
    ]
    // A floor-only renderer rejects a draft containing any wall material.
    private static let workspaceWallMaterialManifestPins: Set<String> = [
        "6be2d5969462e4961e9ecbd3b6417936f2f075de1fbc3626d1b4f19cde7e65ec"
    ]
    private static let workspaceMaterialMultimapManifestPins: Set<String> = [
        "72a13c526fb422c106f8eefaa7e277e5ab003f3c2085ebd09b2814c0a19ef904"
    ]
    private static let workspaceCatalogReadManifest = "aad7ddf920dc64b02b916166e829d9638fca11b585cd0cb9157bc49a0f09e0d9"
    #else
    private static let workspaceMaterialManifestPins: Set<String> = []
    private static let workspaceWallMaterialManifestPins: Set<String> = []
    #endif
    public nonisolated let workspaceMaterialRuntimeQualified: Bool
    public nonisolated let workspaceWallMaterialRuntimeQualified: Bool
    public nonisolated let workspaceMaterialMultimapRuntimeQualified: Bool
    private let workspaceCatalogReadRuntimeQualified: Bool
    // Add the paired manifest only after the native 036 owner path is qualified.
    private static let workspaceFurnishingManifestPins: Set<String> = []
    public nonisolated let workspaceFurnishingRuntimeQualified: Bool
    private var nativeSession = false
    private var session: AuthoringSession?
    private var generation: UInt64 = 0
    private var authenticationBusy = false
    private var activeRequests = 0
    private struct BoundedExportIntent: Sendable {
        let requestId: String
        let exhibitionId: String
        let etag: String
        let generation: UInt64
    }
    private var uncertainBoundedExport: BoundedExportIntent?
    private var boundedExportBusy = false

    public init(port: UInt16, tenantId: String, nativeRuntimeManifestSHA256: String? = nil) throws {
        guard port >= 1024, let id = UUID(uuidString: tenantId) else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        base = URL(string: "http://127.0.0.1:\(port)")!
        tenant = id.uuidString.lowercased()
        admitsICCJPEG = NativeJPEGImportRuntime.supportsICC(manifestSHA256: nativeRuntimeManifestSHA256, ready: true)
        let manifest = nativeRuntimeManifestSHA256 ?? ""
        #if DEBUG
        let multimapQualified = Self.workspaceMaterialMultimapManifestPins.contains(manifest)
        #else
        let multimapQualified = false
        #endif
        let wallQualified = multimapQualified || Self.workspaceWallMaterialManifestPins.contains(manifest)
        workspaceMaterialMultimapRuntimeQualified = multimapQualified
        workspaceWallMaterialRuntimeQualified = wallQualified
        workspaceMaterialRuntimeQualified = wallQualified || Self.workspaceMaterialManifestPins.contains(manifest)
        workspaceFurnishingRuntimeQualified = Self.workspaceFurnishingManifestPins.contains(manifest)
        #if DEBUG
        workspaceCatalogReadRuntimeQualified = manifest == Self.workspaceCatalogReadManifest
        #else
        workspaceCatalogReadRuntimeQualified = false
        #endif
        let configuration = URLSessionConfiguration.ephemeral
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 15
        configuration.timeoutIntervalForResource = 120 // OEX validates approved asset bytes before returning. Per-request JSON timeout stays15s.
        configuration.connectionProxyDictionary = [:]
        transport = URLSession(configuration: configuration, delegate: LocalRedirectBlocker(), delegateQueue: nil)
    }
    deinit { transport.invalidateAndCancel() }
    private func clear() {
        session = nil
        nativeSession = false
        for cookie in transport.configuration.httpCookieStorage?.cookies ?? [] { transport.configuration.httpCookieStorage?.deleteCookie(cookie) }
    }
    private func identifier(_ value: String) throws -> String {
        guard let id = UUID(uuidString: value) else { throw AuthoringFailure("IDENTIFIER_INVALID") }
        return id.uuidString.lowercased()
    }
    private func encoded<T: Encodable>(_ body: T) throws -> Data { try JSONEncoder().encode(body) }
    private func request<T: Decodable & Sendable>(_ path: String, method: String = "GET", body: Data? = nil, bodyFile: URL? = nil, bodyLength: Int? = nil, authenticated: Bool = true, contentType: String = "application/json", ifMatch: String? = nil, chunkSHA256: String? = nil, publicationProfile: String? = nil, timeout: TimeInterval = 15) async throws -> T {
        guard !(authenticated && authenticationBusy && path != "/api/v1/auth/logout") else { throw AuthoringFailure("AUTH_BUSY") }
        activeRequests += 1
        defer { activeRequests -= 1 }
        let current = generation
        if authenticated && session == nil { throw AuthoringFailure("AUTH_REQUIRED", status: 401) }
        guard let url = URL(string: path, relativeTo: base)?.absoluteURL, url.host == "127.0.0.1", url.port == base.port, url.scheme == "http", url.user == nil, url.password == nil else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        var request = URLRequest(url: url, timeoutInterval: timeout)
        request.httpMethod = method; request.httpBody = body
        if let publicationProfile {
            guard publicationProfile == "bounded-local-v1", session?.nativePublicationProfile == publicationProfile else { throw AuthoringFailure("PUBLICATION_STAGE_UNAVAILABLE") }
            request.setValue(publicationProfile, forHTTPHeaderField: "X-ExhibitOS-Publication-Profile")
        }
        if let bodyFile {
            guard body == nil, let bodyLength, bodyLength > 0, let stream = InputStream(url: bodyFile) else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
            request.httpBodyStream = stream
            request.setValue(String(bodyLength), forHTTPHeaderField: "Content-Length")
        }
        if let ifMatch {
            guard ifMatch.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil else { throw AuthoringFailure("ETAG_INVALID") }
            request.setValue(ifMatch, forHTTPHeaderField: "If-Match")
        }
        if let chunkSHA256 {
            guard method == "PUT", contentType == "application/octet-stream", body != nil,
                  chunkSHA256.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
            request.setValue(chunkSHA256, forHTTPHeaderField: "x-exhibitos-chunk-sha256")
        }
        request.setValue(base.absoluteString, forHTTPHeaderField: "Origin")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if body != nil || bodyFile != nil { request.setValue(contentType, forHTTPHeaderField: "Content-Type") }
        if method != "GET", authenticated, let session { request.setValue(session.csrfToken, forHTTPHeaderField: "x-csrf-token") }
        let (bytes, response) = try await transport.bytes(for: request)
        defer { bytes.task.cancel() }
        guard let http = response as? HTTPURLResponse, http.url == url else { throw AuthoringFailure("RESPONSE_INVALID") }
        var data = Data()
        for try await byte in bytes {
            try Task.checkCancellation()
            guard data.count < 2 * 1024 * 1024 else { throw AuthoringFailure("RESPONSE_LIMIT") }
            data.append(byte)
        }
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        guard (200..<300).contains(http.statusCode) else {
            let envelope = try? JSONDecoder().decode(APIErrorEnvelope.self, from: data)
            let code = envelope?.code ?? "REQUEST_FAILED"
            if http.statusCode == 401 { generation &+= 1; clear() }
            throw AuthoringFailure(AuthoringFailure.safeCode(code) ? code : "REQUEST_FAILED", status: http.statusCode,
                                   role: envelope?.role, field: envelope?.field)
        }
        let result = try JSONDecoder().decode(T.self, from: data)
        if let studio = result as? NativeStudioResponse {
            guard studio.revision > 0, studio.revision <= 2_147_483_647,
                  studio.etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil,
                  studio.etag.hasPrefix("\"studio-r\(studio.revision)-"), http.value(forHTTPHeaderField: "ETag") == studio.etag else { throw AuthoringFailure("ETAG_INVALID") }
        }
        if let editing = result as? NativeReusableEditingDraft {
            guard http.value(forHTTPHeaderField: "ETag") == editing.etag else { throw AuthoringFailure("ETAG_INVALID") }
        }
        return result
    }
    public func furnishings() async throws -> Bool {
        let value: DocumentValue = try await request("/api/v1/tenants/\(tenant)/studio/furnishings")
        return try FurnishingCatalog.checkedCatalog(value)
    }
    public func sharedFurnishings() async throws -> [DocumentValue] {
        let value: DocumentValue = try await request("/api/v1/tenants/\(tenant)/studio/furnishings")
        return try SharedFurnishingCatalog.checkedCatalog(value)
    }
    public func surfaceMaterials() async throws -> NativeSurfaceMaterialCatalog {
        let value: DocumentValue = try await request("/api/v1/tenants/\(tenant)/studio/materials")
        return try NativeSurfaceMaterialCatalog(value: value, tenantId: tenant)
    }
    public func privateFloorCatalog() async throws -> NativePrivateFloorCatalog {
        let value: DocumentValue = try await request(studioPrefix + "/materials/private-floor")
        return try NativePrivateFloorCatalog(value: value, tenantId: tenant)
    }
    public func connect(_ credentials: LocalAuthoringSession) async throws -> AuthoringSession {
        guard !authenticationBusy, activeRequests == 0 else { throw AuthoringFailure("AUTH_BUSY") }
        guard credentials.tenantId.lowercased() == tenant, credentials.origin == base.absoluteString,
              UUID(uuidString: credentials.principalId) != nil,
              credentials.token.range(of: "^[A-Za-z0-9_-]{43}$", options: .regularExpression) != nil else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        authenticationBusy = true
        generation &+= 1; clear()
        defer { authenticationBusy = false; if session == nil { clear() } }
        guard let cookie = HTTPCookie(properties: [.domain: "127.0.0.1", .path: "/", .name: "exhibitos_local_session", .value: credentials.token, .discard: "TRUE"]) else { throw AuthoringFailure("LOCAL_SESSION_INVALID") }
        transport.configuration.httpCookieStorage?.setCookies([cookie], for: base, mainDocumentURL: base)
        let value: AuthoringSession = try await request("/api/v1/auth/session", authenticated: false)
        guard value.tenantId.lowercased() == tenant, value.userId.lowercased() == credentials.principalId.lowercased(), !value.csrfToken.isEmpty else { clear(); throw AuthoringFailure("ACCOUNT_BINDING_MISMATCH") }
        session = value
        nativeSession = true
        return value
    }
    /// Recheck the existing cookie-bound session without minting credentials
    /// or clearing a live selected import's authority.
    public func verifyRetainedSession(_ expected: AuthoringSession) async throws -> AuthoringSession {
        guard let original = session, original.userId == expected.userId, original.tenantId == expected.tenantId,
              original.role == expected.role, original.csrfToken == expected.csrfToken, original.expiresAt == expected.expiresAt else {
            throw AuthoringFailure("ACCOUNT_BINDING_MISMATCH")
        }
        let current: AuthoringSession = try await request("/api/v1/auth/session")
        guard let held = session, held.userId == original.userId, held.tenantId == original.tenantId,
              held.role == original.role, held.csrfToken == original.csrfToken, held.expiresAt == original.expiresAt,
              current.userId == original.userId, current.tenantId == original.tenantId,
              current.role == original.role, current.csrfToken == original.csrfToken, current.expiresAt == original.expiresAt else {
            throw AuthoringFailure("ACCOUNT_BINDING_MISMATCH")
        }
        return current
    }
    public func login(subject: String, password: String) async throws -> AuthoringSession {
        guard !authenticationBusy, activeRequests == 0 else { throw AuthoringFailure("AUTH_BUSY") }
        authenticationBusy = true
        defer { authenticationBusy = false; if session == nil { clear() } }
        generation &+= 1; clear()
        struct Login: Encodable { let subject: String; let password: String; let tenantId: String }
        struct Accepted: Decodable, Sendable { let authenticated: Bool }
        let accepted: Accepted = try await request("/api/v1/auth/login", method: "POST", body: encoded(Login(subject: subject, password: password, tenantId: tenant)), authenticated: false)
        guard accepted.authenticated else { throw AuthoringFailure("AUTH_REQUIRED", status: 401) }
        let value: AuthoringSession = try await request("/api/v1/auth/session", authenticated: false)
        guard value.tenantId.lowercased() == tenant, UUID(uuidString: value.userId) != nil, !value.csrfToken.isEmpty else { clear(); throw AuthoringFailure("ACCOUNT_BINDING_MISMATCH") }
        session = value
        return value
    }
    public func logout() async throws {
        guard !authenticationBusy, activeRequests == 0 else { throw AuthoringFailure("AUTH_BUSY") }
        authenticationBusy = true
        defer { authenticationBusy = false }
        struct Done: Decodable, Sendable { let loggedOut: Bool }
        defer { generation &+= 1; clear() }
        let _: Done = try await request("/api/v1/auth/logout", method: "POST", body: Data("{}".utf8))
    }
    private var prefix: String { "/api/v1/tenants/\(tenant)/cms" }
    public func artworks(query: String = "", cursor: String? = nil) async throws -> AuthoringPage<ArtworkSummary> {
        var components = URLComponents()
        components.path = prefix + "/artworks"
        components.queryItems = [URLQueryItem(name: "limit", value: "20"), URLQueryItem(name: "archived", value: "false"), URLQueryItem(name: "q", value: String(query.prefix(100)))]
        if let cursor { components.queryItems?.append(URLQueryItem(name: "cursor", value: try identifier(cursor))) }
        return try await request(components.string!)
    }
    public func artists() async throws -> AuthoringPage<NativeArtist> { try await request(prefix + "/artists?limit=50&archived=false") }
    public func artist(_ id: String) async throws -> NativeArtist { try await request(prefix + "/artists/" + identifier(id)) }
    public func artwork(_ id: String) async throws -> NativeArtwork { try await request(prefix + "/artworks/" + identifier(id)) }
    public func createArtist(name: String, bio: String) async throws -> NativeArtist {
        struct Input: Encodable { let name: String; let bio: String }
        return try await request(prefix + "/artists", method: "POST", body: encoded(Input(name: name, bio: bio)))
    }
    public func saveArtwork(id: String?, revision: Int?, artistId: String, metadata: ArtworkMetadata) async throws -> NativeArtwork {
        guard var fields = try JSONSerialization.jsonObject(with: encoded(metadata)) as? [String: Any] else { throw AuthoringFailure("METADATA_INVALID") }
        let path: String
        if let id, let revision {
            path = prefix + "/artworks/" + (try identifier(id)); fields["revision"] = revision
        } else if id == nil && revision == nil {
            path = prefix + "/artworks"; fields["artistId"] = try identifier(artistId)
        } else { throw AuthoringFailure("REVISION_REQUIRED") }
        return try await request(path, method: id == nil ? "POST" : "PATCH", body: JSONSerialization.data(withJSONObject: fields))
    }
    private var studioPrefix: String { "/api/v1/tenants/\(tenant)/studio" }
    public func exhibitions(query: String = "", cursor: String? = nil) async throws -> AuthoringPage<NativeExhibitionSummary> {
        var components = URLComponents(); components.path = studioPrefix + "/exhibitions"
        components.queryItems = [URLQueryItem(name: "limit", value: "20"), URLQueryItem(name: "q", value: String(query.prefix(100)))]
        if let cursor { components.queryItems?.append(URLQueryItem(name: "cursor", value: try identifier(cursor))) }
        return try await request(components.string!)
    }
    public func exhibition(_ id: String) async throws -> NativeStudioResponse {
        let value: NativeStudioResponse = try await request(studioPrefix + "/exhibitions/" + identifier(id))
        guard value.draft.id.lowercased() == id.lowercased() else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
        return value
    }
    /// A scoped policy observation; it does not authorize a write.
    public func nativeViewportPolicy(_ id: String) async throws -> NativeViewportPolicy {
        let value: NativeViewportPolicy = try await request(studioPrefix + "/exhibitions/" + identifier(id) + "/native-viewport-policy")
        guard value.exhibitionId.lowercased() == id.lowercased(), value.revision > 0, !value.etag.isEmpty else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
        return value
    }
    /// A read-only lookup; a 404 from an older runtime remains unsupported and
    /// never falls back to silently creating a copy.
    public func reusableEditingDraft(publicationId: String) async throws -> NativeReusableEditingDraft {
        let id = try identifier(publicationId)
        let value: NativeReusableEditingDraft = try await request(studioPrefix + "/publications/" + id + "/editing-draft")
        return try value.validated(expectedPublicationId: id)
    }
    /// Keep the same draft/requestId/ETag after uncertain replies; a conflict
    /// requires comparison. This client never obtains a fresh ETag then retries.
    public func saveExhibition(_ draft: NativeExhibitionDraft, requestId: String, etag: String?) async throws -> NativeStudioResponse {
        struct Input: Encodable { let draft: NativeExhibitionDraft; let requestId: String }
        let body = try encoded(Input(draft: draft, requestId: identifier(requestId)))
        guard body.count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        let path = studioPrefix + "/exhibitions" + (etag == nil ? "" : "/" + (try identifier(draft.id)))
        let response: NativeStudioResponse = try await request(path, method: etag == nil ? "POST" : "PUT", body: body, ifMatch: etag)
        guard response.draft.id == draft.id, response.draft == draft else { throw AuthoringFailure("EXHIBITION_SCOPE_CHANGED") }
        return response
    }
    /// One selected-session save. Persist and revalidate the exact pending
    /// request before the single PUT; uncertainty is never retried here.
    public func saveAndReopenSelectedOEX(exhibitionId: String, expectedETag: String, requestId: String,
        persistAndAuthorize: @Sendable (NativeStudioResponse, String) async throws -> Void) async throws -> NativeStudioResponse {
        let current = generation
        let original = try await exhibition(exhibitionId)
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        guard original.etag == expectedETag else { throw AuthoringFailure("EXHIBITION_COMPARISON_STALE", status: 409) }
        try Task.checkCancellation()
        try await persistAndAuthorize(original, requestId)
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        try Task.checkCancellation()
        let saved = try await saveExhibition(original.draft, requestId: requestId, etag: original.etag)
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        guard saved.revision == original.revision + 1, saved.etag != original.etag else { throw AuthoringFailure("EXHIBITION_COMPARISON_STALE", status: 409) }
        try Task.checkCancellation()
        let reopened = try await exhibition(exhibitionId)
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        try saved.requireSameComparison(reopened)
        return reopened
    }
    public func publicationReady(exhibitionId: String, etag: String? = nil, publicationProfile: String? = nil) async throws -> NativePublicationReady {
        if publicationProfile != nil, etag == nil { throw AuthoringFailure("ETAG_INVALID") }
        let result: NativePublicationReady = try await request(studioPrefix + "/exhibitions/" + identifier(exhibitionId) + "/ready", ifMatch: publicationProfile == nil ? nil : etag, publicationProfile: publicationProfile, timeout: 120)
        return try result.validated()
    }
    public func preparePublication(exhibitionId: String, etag: String, requestId: String, publicationProfile: String? = nil) async throws -> NativePublication {
        struct Input: Encodable { let requestId: String }
        let result: NativePublication = try await request(studioPrefix + "/exhibitions/" + identifier(exhibitionId) + "/publications", method: "POST", body: encoded(Input(requestId: identifier(requestId))), ifMatch: etag, publicationProfile: publicationProfile, timeout: 120)
        return try result.validated()
    }
    public func publications(exhibitionId: String) async throws -> [NativePublication] {
        struct Page: Decodable, Sendable { let items: [NativePublication] }
        let result: Page = try await request(studioPrefix + "/exhibitions/" + identifier(exhibitionId) + "/publications")
        guard result.items.count <= 100 else { throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID") }
        return try result.items.map { try $0.validated() }
    }
    public func unpublish(_ expected: NativePublication) async throws -> NativePublication {
        _ = try expected.validated()
        guard expected.status == "published" else { throw AuthoringFailure("PUBLICATION_STATUS_UNCONFIRMED") }
        let result: NativePublication = try await request(studioPrefix + "/publications/" + identifier(expected.publicationId) + "/unpublish", method: "POST", body: Data("{}".utf8))
        return try expected.acceptingStatus(result, requiredStatus: "unpublished")
    }
    /// Export only a saved exact revision through the existing rights/integrity gate.
    /// Bounded streaming prevents a malformed local response from allocating an unbounded archive.
    public func floorPreview(_ preset: FloorSettings.Preset) async throws -> Data {
        try await floorPreviewFile(.forPreset(preset))
    }

    public func privateFloorPreview(_ catalog: NativePrivateFloorCatalog) async throws -> Data {
        try await floorPreviewFile(.init(path: catalog.previewPath, bytes: NativePrivateFloorCatalog.previewBytes,
                                         sha256: NativePrivateFloorCatalog.previewSHA256))
    }

    public func surfaceMaterialPreview(_ preview: NativeSurfaceMaterialCatalog.Preview) async throws -> Data {
        guard (1...2_000_000).contains(preview.bytes), preview.width <= 256, preview.height <= 256 else {
            throw AuthoringFailure("MATERIAL_PREVIEW_INVALID")
        }
        return try await floorPreviewFile(.init(path: preview.url, bytes: Int(preview.bytes), sha256: preview.sha256))
    }

    private func floorPreviewFile(_ file: NativeFloorPreview, requireDigestHeader: Bool = false) async throws -> Data {
        try await readPreviewBytes(path: file.path, expectedBytes: file.bytes, sha256: file.sha256,
            mime: "image/png", invalidCode: "FLOOR_PREVIEW_INVALID", requireDigestHeader: requireDigestHeader)
    }

    private func readPreviewBytes(path: String, expectedBytes: Int, sha256: String, mime: String,
                                  invalidCode: String, requireDigestHeader: Bool) async throws -> Data {
        guard !authenticationBusy, session != nil else { throw AuthoringFailure("AUTH_REQUIRED", status: 401) }
        let current = generation
        let url = base.appendingPathComponent(path)
        activeRequests += 1
        defer { activeRequests -= 1 }
        var request = URLRequest(url: url, timeoutInterval: 30)
        request.setValue(mime, forHTTPHeaderField: "Accept")
        request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        let (bytes, response) = try await transport.bytes(for: request)
        defer { bytes.task.cancel() }
        if requireDigestHeader, let http = response as? HTTPURLResponse, http.url == url, http.statusCode == 403 {
            throw AuthoringFailure("DISPLAY_PERMISSION_REQUIRED", status: 403)
        }
        guard let http = response as? HTTPURLResponse, http.url == url, http.statusCode == 200,
              http.mimeType?.lowercased() == mime,
              !requireDigestHeader || http.value(forHTTPHeaderField: "x-exhibitos-sha256") == sha256,
              http.value(forHTTPHeaderField: "Content-Encoding").map({ $0.lowercased() == "identity" }) ?? true,
              http.expectedContentLength < 0 || http.expectedContentLength == Int64(expectedBytes) else { throw AuthoringFailure(invalidCode) }
        var data = Data(); data.reserveCapacity(expectedBytes)
        try await withTaskCancellationHandler {
            for try await byte in bytes {
                try Task.checkCancellation()
                guard data.count < expectedBytes else { throw AuthoringFailure(invalidCode) }
                data.append(byte)
            }
        } onCancel: { bytes.task.cancel() }
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        guard data.count == expectedBytes, nativeImportSHA256(data) == sha256 else { throw AuthoringFailure(invalidCode) }
        return data
    }
    public func exportOEX(exhibitionId: String, etag: String,
                          progress: (@MainActor @Sendable (NativeOEXExportProgress) -> Void)? = nil) async throws -> NativeOEXArchive {
        try await receiveOEXExport(exhibitionId: exhibitionId, etag: etag, bounded: false, progress: progress).archive
    }

    /// Uses only the selected native producer. Legacy export remains limited to64MiB.
    public func exportBoundedOEX(exhibitionId: String, etag: String,
                                 progress: (@MainActor @Sendable (NativeOEXExportProgress) -> Void)? = nil) async throws -> NativeBoundedOEXExport {
        guard !authenticationBusy, session != nil else { throw AuthoringFailure("AUTH_REQUIRED", status: 401) }
        guard session?.nativeOexExportProfile == "bounded-file-v1" else { throw AuthoringFailure("NATIVE_OEX_EXPORT_UNAVAILABLE") }
        guard !boundedExportBusy else { throw AuthoringFailure("NATIVE_OEX_EXPORT_BUSY") }
        let id = try identifier(exhibitionId), current = generation
        guard etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil else { throw AuthoringFailure("ETAG_INVALID") }
        boundedExportBusy = true; activeRequests += 1
        defer { boundedExportBusy = false; activeRequests -= 1 }
        if let previous = uncertainBoundedExport {
            // An explicit retry first correlates the original operation; it never
            // starts another producer while the previous owner is unresolved.
            _ = try await boundedExportCompletion(previous, allowFailed: true)
            uncertainBoundedExport = nil
        }
        let intent = BoundedExportIntent(requestId: UUID().uuidString.lowercased(), exhibitionId: id, etag: etag, generation: current)
        uncertainBoundedExport = intent
        do {
            let received = try await receiveOEXExport(exhibitionId: id, etag: etag, bounded: true,
                                                      requestId: intent.requestId, progress: progress)
            guard received.operationId == intent.requestId else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
            let result = try await boundedExportCompletion(intent)
            guard result.bytes == received.archive.count, result.sha256 == received.archive.sha256 else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
            try Task.checkCancellation()
            uncertainBoundedExport = nil
            return NativeBoundedOEXExport(received.archive)
        } catch {
            // Keep the request identity even when the operation header/response
            // was lost. No error or cancellation is an owner-release assertion.
            throw error
        }
    }

    private func boundedExportCompletion(_ intent: BoundedExportIntent, allowFailed: Bool = false) async throws -> NativeOEXExportCompletion {
        let path = studioPrefix + "/exhibitions/" + intent.exhibitionId + "/native-oex/exports/" + intent.requestId
        let clock = ContinuousClock(), deadline = ContinuousClock.now.advanced(by: .seconds(30))
        while clock.now < deadline {
            try Task.checkCancellation()
            guard generation == intent.generation, session != nil else { throw AuthoringFailure("NATIVE_OEX_EXPORT_UNCERTAIN") }
            let result: NativeOEXExportCompletion = try await request(path, ifMatch: intent.etag, timeout: 5)
            guard generation == intent.generation, session != nil else { throw AuthoringFailure("NATIVE_OEX_EXPORT_UNCERTAIN") }
            guard result.operationId == intent.requestId else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
            if result.state == "complete" {
                guard result.etag == intent.etag, let count = result.bytes, count > 0,
                      count <= NativeOEXImportFile.boundedMaximumBytes,
                      result.sha256?.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
                return result
            }
            if allowFailed, result.state == "failed", result.etag == nil, result.bytes == nil, result.sha256 == nil { return result }
            guard result.state == "pending", result.etag == nil, result.bytes == nil, result.sha256 == nil else {
                throw AuthoringFailure("NATIVE_OEX_EXPORT_UNCERTAIN")
            }
            try await Task.sleep(for: .milliseconds(250))
        }
        throw AuthoringFailure("NATIVE_OEX_EXPORT_UNCERTAIN")
    }

    /// Optional observation uses its own read-only endpoint. Polling must not
    /// acknowledge the terminal receipt that authorizes saving the archive.
    private func observeBoundedExport(exhibitionId: String, operationId: String, etag: String, generation: UInt64,
                                      progress: @MainActor @Sendable (NativeOEXExportProgress) -> Void) async {
        let path = studioPrefix + "/exhibitions/" + exhibitionId + "/native-oex/exports/" + operationId + "/progress"
        let url = base.appendingPathComponent(path)
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 3)
        request.setValue(etag, forHTTPHeaderField: "If-Match")
        request.setValue(base.absoluteString, forHTTPHeaderField: "Origin")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        while !Task.isCancelled, self.generation == generation, session != nil {
            do {
                let (bytes, response) = try await transport.bytes(for: request)
                defer { bytes.task.cancel() }
                guard let http = response as? HTTPURLResponse, http.url == url else { return }
                if http.statusCode == 200, http.mimeType == "application/json", http.expectedContentLength <= 4096 {
                    var data = Data()
                    for try await byte in bytes {
                        guard data.count < 4096 else { return }
                        data.append(byte)
                    }
                    let status = try JSONDecoder().decode(NativeOEXPreparationStatus.self, from: data)
                    if !Task.isCancelled, self.generation == generation, status.operationId == operationId,
                       let update = status.update { await progress(update) }
                } else if http.statusCode != 404 {
                    return
                }
                try await Task.sleep(for: .milliseconds(500))
            } catch {
                return
            }
        }
    }

    private func receiveOEXExport(exhibitionId: String, etag: String, bounded: Bool, requestId: String? = nil,
                                  progress: (@MainActor @Sendable (NativeOEXExportProgress) -> Void)?) async throws -> NativeOEXExportResponse {
        guard !authenticationBusy, let session else { throw AuthoringFailure("AUTH_REQUIRED", status: 401) }
        guard etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil else { throw AuthoringFailure("ETAG_INVALID") }
        let id = try identifier(exhibitionId)
        let url = base.appendingPathComponent(studioPrefix + "/exhibitions/" + id + (bounded ? "/native-oex/export" : "/oex/export"))
        activeRequests += 1
        defer { activeRequests -= 1 }
        let current = generation
        var request = URLRequest(url: url, timeoutInterval: 120)
        request.httpMethod = "POST"
        if bounded {
            struct Input: Encodable { let requestId: String }
            request.httpBody = try encoded(Input(requestId: identifier(requestId ?? "")))
        } else { request.httpBody = Data("{}".utf8) }
        request.setValue(base.absoluteString, forHTTPHeaderField: "Origin")
        request.setValue(session.csrfToken, forHTTPHeaderField: "x-csrf-token")
        request.setValue(etag, forHTTPHeaderField: "If-Match")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/vnd.exhibitos.oex+zip", forHTTPHeaderField: "Accept")
        request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        let observer: Task<Void, Never>?
        if bounded, let requestId, let progress {
            observer = Task { await observeBoundedExport(exhibitionId: id, operationId: requestId, etag: etag,
                                                         generation: current, progress: progress) }
        } else {
            observer = nil
        }
        defer { observer?.cancel() }
        let (bytes, response) = try await transport.bytes(for: request)
        observer?.cancel()
        defer { bytes.task.cancel() } // also cancels early header/size rejection
        guard let http = response as? HTTPURLResponse, http.url == url else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        let success = http.statusCode == 200
        let limit = success ? (bounded ? NativeOEXImportFile.boundedMaximumBytes : NativeOEXArchive.maximumBytes) : 64 * 1024
        guard http.expectedContentLength <= Int64(limit) else { throw AuthoringFailure(success ? (bounded ? "NATIVE_OEX_EXPORT_LIMIT" : "OEX_LIMIT") : "RESPONSE_LIMIT") }
        var expectedSHA256: String?
        var operationId: String?
        if success {
            if bounded {
                let digest = http.value(forHTTPHeaderField: "X-ExhibitOS-OEX-SHA256") ?? ""
                let rawLength = http.value(forHTTPHeaderField: "Content-Length") ?? ""
                let cache = (http.value(forHTTPHeaderField: "Cache-Control") ?? "").lowercased().split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
                guard let length = Int64(rawLength), length > 0, String(length) == rawLength,
                      length == http.expectedContentLength, length <= Int64(limit),
                      http.value(forHTTPHeaderField: "Transfer-Encoding") == nil, cache.contains("no-store"),
                      http.value(forHTTPHeaderField: "X-ExhibitOS-OEX-Profile") == "bounded-file-v1",
                      digest.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
                let rawOperation = http.value(forHTTPHeaderField: "X-ExhibitOS-OEX-Operation") ?? ""
                guard let uuid = UUID(uuidString: rawOperation), uuid.uuidString.lowercased() == rawOperation,
                      rawOperation == requestId else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
                expectedSHA256 = digest; operationId = rawOperation
            }
            guard http.mimeType?.lowercased() == NativeOEXArchive.mime,
                  http.value(forHTTPHeaderField: "Content-Encoding").map({ $0.lowercased() == "identity" }) ?? true else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        }
        if !success {
            var data = Data()
            for try await byte in bytes {
                try Task.checkCancellation()
                guard data.count < limit else { throw AuthoringFailure("RESPONSE_LIMIT") }
                data.append(byte)
            }
            guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
            let code = (try? JSONDecoder().decode(APIErrorEnvelope.self, from: data).code) ?? "REQUEST_FAILED"
            if bounded, (400..<500).contains(http.statusCode),
               let requestId, uncertainBoundedExport?.requestId == requestId,
               http.value(forHTTPHeaderField: "X-ExhibitOS-OEX-Operation") == requestId,
               http.value(forHTTPHeaderField: "X-ExhibitOS-OEX-Admission") == "attempt-not-admitted" {
                uncertainBoundedExport = nil
            }
            if http.statusCode == 401 { generation &+= 1; clear() }
            throw AuthoringFailure(code.range(of: "^[A-Z0-9_]{1,80}$", options: .regularExpression) == nil ? "REQUEST_FAILED" : code, status: http.statusCode)
        }
        let writer = try OEXArchiveWriter(limit: limit)
        let total = http.expectedContentLength > 0 ? Int(http.expectedContentLength) : nil
        let clock = ContinuousClock()
        var lastUpdate = clock.now
        await progress?(.receiving(bytes: 0, total: total))
        var chunk = Data(); chunk.reserveCapacity(256 * 1024)
        try await withTaskCancellationHandler {
            for try await byte in bytes {
                try Task.checkCancellation()
                guard writer.count + chunk.count < limit else { throw AuthoringFailure(bounded ? "NATIVE_OEX_EXPORT_LIMIT" : "OEX_LIMIT") }
                chunk.append(byte)
                if chunk.count == 256 * 1024 {
                    try writer.append(chunk); chunk.removeAll(keepingCapacity: true)
                    if progress != nil, total.map({ writer.count <= $0 }) ?? true,
                       lastUpdate.duration(to: clock.now) >= .milliseconds(125) {
                        lastUpdate = clock.now
                        await progress?(.receiving(bytes: writer.count, total: total))
                    }
                }
            }
            if !chunk.isEmpty { try writer.append(chunk) }
        } onCancel: { bytes.task.cancel() }
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        guard http.expectedContentLength < 0 || Int64(writer.count) == http.expectedContentLength else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        await progress?(.receiving(bytes: writer.count, total: total))
        await progress?(.verifying)
        let staged = try writer.finish()
        if bounded, staged.sha256 != expectedSHA256 { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        return NativeOEXExportResponse(archive: staged, operationId: operationId)
    }

    /// Reuses the server's scoped OEX import and idempotent request ID. No
    /// credentials, local file paths or archive bytes enter the native helper.
    public func createOEXImport(requestId: String, bytes: Int, sha256: String) async throws -> NativeOEXImportJob {
        struct Body: Encodable { let requestId: String; let bytes: Int; let sha256: String }
        guard bytes > 0, bytes <= NativeOEXArchive.maximumBytes,
              sha256.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
        let body = Body(requestId: try identifier(requestId), bytes: bytes, sha256: sha256)
        let job: NativeOEXImportJob = try await request("/api/v1/tenants/\(tenant)/oex/imports", method: "POST", body: encoded(body))
        return try job.checked()
    }
    public func oexImportJob(_ id: String) async throws -> NativeOEXImportJob {
        let job: NativeOEXImportJob = try await request("/api/v1/tenants/\(tenant)/oex/imports/\(identifier(id))")
        return try job.checked()
    }
    /// Preserve the supported importer's full receipt/idMap without inventing
    /// fields absent from older runtimes or exposing local session credentials.
    public func oexImportReceipt(_ id: String) async throws -> DocumentValue {
        try await request("/api/v1/tenants/\(tenant)/oex/imports/\(identifier(id))")
    }
    public func uploadOEXImport(_ id: String, archive: NativeOEXArchive) async throws {
        struct Receipt: Decodable { let received: Int }
        let receipt: Receipt = try await request("/api/v1/tenants/\(tenant)/oex/imports/\(identifier(id))/bytes", method: "PUT", bodyFile: archive.fileURL, bodyLength: archive.count, contentType: "application/octet-stream", timeout: 120)
        guard receipt.received == archive.count else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
    }
    /// Inactive selected-profile client. A live Session capability and server
    /// writer exclusion must be established before a caller may use this.
    public func createBoundedOEXImport(requestId: String, archive: NativeOEXArchive) async throws -> NativeBoundedOEXJob {
        struct Body: Encodable { let requestId: String; let bytes: Int; let sha256: String }
        guard archive.count > 0, archive.count <= NativeOEXImportFile.boundedMaximumBytes,
              archive.sha256.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
        let body = Body(requestId: try identifier(requestId), bytes: archive.count, sha256: archive.sha256)
        let job: NativeBoundedOEXJob = try await request("/api/v1/tenants/\(tenant)/native-oex/imports", method: "POST", body: encoded(body))
        guard try identifier(job.id) == job.id, job.state == "uploading" else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
        return job
    }
    /// Each immutable retry resends the exact same snapshot segment. A verified
    /// upload is not an import, commit, or permission to release its resources.
    public func uploadBoundedOEXImport(_ id: String, archive: NativeOEXArchive) async throws {
        try await uploadBoundedOEXImport(id, archive: archive, observe: nil)
    }
    public func uploadBoundedOEXImport(_ id: String, archive: NativeOEXArchive, observe: NativeOEXUploadObservation?) async throws {
        struct ChunkReceipt: Decodable { let index: Int; let received: Int }
        let job = try identifier(id), current = generation, chunkSize = 16 * 1024 * 1024
        guard archive.count > 0, archive.count <= NativeOEXImportFile.boundedMaximumBytes, session != nil else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
        let chunks = (archive.count + chunkSize - 1) / chunkSize
        var verifiedBytes = 0
        for index in 0..<chunks {
            try Task.checkCancellation()
            guard generation == current, session != nil else { throw AuthoringFailure("OEX_IMPORT_SCOPE_CHANGED") }
            let bytes = try archive.boundedChunk(index: index)
            let hash = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
            let receipt: ChunkReceipt = try await request("/api/v1/tenants/\(tenant)/native-oex/imports/\(job)/chunks/\(index)", method: "PUT", body: bytes, contentType: "application/octet-stream", chunkSHA256: hash, timeout: 120)
            guard generation == current, receipt.index == index, receipt.received == bytes.count else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
            verifiedBytes += receipt.received
            await observe?(verifiedBytes, archive.count)
        }
    }
    public func queueBoundedOEXImport(_ id: String) async throws -> NativeBoundedOEXJob {
        let job = try identifier(id), current = generation
        let result: NativeBoundedOEXJob = try await request("/api/v1/tenants/\(tenant)/native-oex/imports/\(job)/queue", method: "POST", body: Data("{}".utf8), timeout: 120)
        guard generation == current, (try identifier(result.id)) == job, result.state == "queued" else { throw AuthoringFailure("OEX_IMPORT_RESPONSE_INVALID") }
        return result
    }
    public func oexImportAction(_ id: String, action: String) async throws -> NativeOEXImportJob {
        guard ["complete", "retry", "cancel"].contains(action) else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
        let job: NativeOEXImportJob = try await request("/api/v1/tenants/\(tenant)/oex/imports/\(identifier(id))/\(action)", method: "POST", body: Data("{}".utf8))
        return try job.checked()
    }
    public func studioArtwork(_ id: String) async throws -> NativeStudioArtwork {
        let id = try identifier(id)
        let response: NativeStudioArtwork = try await request(studioPrefix + "/artworks/" + id)
        guard response.artwork["id"].string?.lowercased() == id,
              response.artwork["extensions"]["org.exhibitos.studio/cms"]["tenantId"].string?.lowercased() == tenant,
              response.previewUrl == prefix + "/artworks/" + id + "/preview" else { throw AuthoringFailure("ARTWORK_SCOPE_CHANGED") }
        return response
    }
    public func importJob(_ id: String) async throws -> NativeImportJob {
        try await request("/api/v1/tenants/\(tenant)/imports/" + identifier(id))
    }
    public func createImport(_ input: NativeImportIntent) async throws -> NativeImportJob {
        guard input.derived?.recipe.colourConversion == nil || admitsICCJPEG else { throw AuthoringFailure("IMPORT_ICC_UNAVAILABLE") }
        guard input.bytes > 0, input.bytes <= 32 * 1024 * 1024,
              input.scaleMeters.isFinite, input.scaleMeters > 0, input.scaleMeters <= 1_000_000,
              ((["model/gltf-binary", "image/png"].contains(input.mime) && input.derived == nil)
                || (input.mime == "image/jpeg" && input.derived?.valid == true)),
              input.modelDerivation == nil || (input.mime == "model/gltf-binary" && input.derived == nil
                && input.modelDerivation?.valid == true && input.modelDerivation?.operationID == input.idempotencyKey),
              input.sha256.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil,
              UUID(uuidString: input.idempotencyKey) != nil else { throw AuthoringFailure("IMPORT_INPUT_INVALID") }
        _ = try identifier(input.artworkId)
        return try await request("/api/v1/tenants/\(tenant)/imports", method: "POST", body: encoded(input))
    }
    public func uploadImport(_ id: String, bytes: Data) async throws {
        guard !bytes.isEmpty, bytes.count <= 32 * 1024 * 1024 else { throw AuthoringFailure("IMPORT_SIZE_INVALID") }
        struct Received: Decodable, Sendable { let received: Int }
        let value: Received = try await request("/api/v1/tenants/\(tenant)/imports/" + identifier(id) + "/bytes", method: "PUT", body: bytes, contentType: "application/octet-stream")
        guard value.received == bytes.count else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
    }
    public func uploadDerivedImport(_ id: String, bytes: Data) async throws {
        guard !bytes.isEmpty, bytes.count <= 8_388_608 else { throw AuthoringFailure("IMPORT_SIZE_INVALID") }
        struct Received: Decodable, Sendable { let received: Int }
        let value: Received = try await request("/api/v1/tenants/\(tenant)/imports/" + identifier(id) + "/derived-bytes", method: "PUT", body: bytes, contentType: "application/octet-stream")
        guard value.received == bytes.count else { throw AuthoringFailure("IMPORT_RECEIPT_INVALID") }
    }
    public func importAction(_ id: String, action: NativeImportAction) async throws -> NativeImportJob {
        try await request("/api/v1/tenants/\(tenant)/imports/" + identifier(id) + "/" + action.rawValue, method: "POST", body: Data("{}".utf8))
    }

    private var hasWorkspaceMaterialSession: Bool {
        nativeSession && !authenticationBusy && session?.workspaceMaterialProfile == "workspace-material-intake-v1"
    }

    public var admitsWorkspaceMaterials: Bool {
        workspaceMaterialRuntimeQualified && hasWorkspaceMaterialSession
    }

    var admitsWorkspaceMaterialCatalog: Bool {
        (workspaceMaterialRuntimeQualified || workspaceCatalogReadRuntimeQualified) && hasWorkspaceMaterialSession
    }

    private func workspaceMaterialPath(_ suffix: String, catalogRead: Bool = false) throws -> String {
        guard catalogRead ? admitsWorkspaceMaterialCatalog : admitsWorkspaceMaterials else {
            throw AuthoringFailure("WORKSPACE_MATERIAL_UNAVAILABLE")
        }
        return "/api/v1/tenants/\(tenant)/workspace/" + suffix
    }

    public func createWorkspaceMaterial(_ declaration: WorkspaceMaterialDeclaration) async throws -> WorkspaceMaterialIntake {
        try declaration.check()
        let status: WorkspaceMaterialIntake = try await request(workspaceMaterialPath("material-intakes"),
            method: "POST", body: encoded(declaration))
        try status.check(declaration: declaration)
        return status
    }

    public func workspaceMaterialIntake(_ id: String) async throws -> WorkspaceMaterialIntake {
        let id = try identifier(id)
        let status: WorkspaceMaterialIntake = try await request(workspaceMaterialPath("material-intakes/" + id))
        try status.check(id: id)
        return status
    }

    public func uploadWorkspaceMaterial(_ id: String, role: WorkspaceMaterialRole, snapshot: ImportFileSnapshot) async throws -> WorkspaceMaterialIntake {
        let id = try identifier(id)
        let map = try WorkspaceMaterialMap(snapshot: snapshot, role: role)
        let path = try workspaceMaterialPath("material-intakes/" + id + "/maps/" + role.rawValue)
        let status: WorkspaceMaterialIntake = try await request(path, method: "PUT", body: snapshot.data,
            contentType: "application/octet-stream")
        try status.check(id: id)
        guard status.received[role.rawValue]?.bytes == map.bytes, status.received[role.rawValue]?.sha256 == map.sha256 else {
            throw AuthoringFailure("MATERIAL_RESPONSE_INVALID")
        }
        return status
    }

    public func workspaceMaterialAction(_ id: String, _ action: WorkspaceMaterialIntakeAction) async throws -> WorkspaceMaterialIntake {
        let id = try identifier(id)
        let path = try workspaceMaterialPath("material-intakes/" + id + "/" + action.rawValue)
        let status: WorkspaceMaterialIntake = try await request(path, method: "POST", body: Data("{}".utf8))
        try status.check(id: id)
        return status
    }

    public func workspaceMaterials(cursor: String? = nil) async throws -> AuthoringPage<WorkspaceMaterialSummary> {
        func validCursor(_ value: String) -> Bool {
            value.range(of: "^[A-Za-z0-9_-]{1,128}$", options: .regularExpression) != nil
        }
        guard cursor.map(validCursor) ?? true else { throw AuthoringFailure("INVALID_CURSOR") }
        let path = try workspaceMaterialPath("materials" + (cursor.map { "?cursor=" + $0 } ?? ""), catalogRead: true)
        let page: AuthoringPage<WorkspaceMaterialSummary> = try await request(path)
        guard page.items.count <= 50, page.nextCursor.map(validCursor) ?? true,
              page.items.allSatisfy({ UUID(uuidString: $0.id)?.uuidString.lowercased() == $0.id
                  && (1...2_147_483_647).contains($0.version) && (1...256).contains($0.name.unicodeScalars.count) }) else {
            throw AuthoringFailure("MATERIAL_RESPONSE_INVALID")
        }
        return page
    }

    public func workspaceMaterial(_ id: String, version: Int) async throws -> WorkspaceMaterial {
        let id = try identifier(id)
        let path = try workspaceMaterialVersionPath(id, version: version, catalogRead: true)
        let material: WorkspaceMaterial = try await request(path)
        try material.check()
        guard material.id == id, material.version == version else { throw AuthoringFailure("MATERIAL_RESPONSE_INVALID") }
        return material
    }

    public func workspaceMaterialPreview(_ material: WorkspaceMaterial, role: WorkspaceMaterialRole) async throws -> Data {
        try material.check()
        guard let map = material.maps[role.rawValue] else { throw AuthoringFailure("MATERIAL_MAP_NOT_FOUND") }
        let path = try workspaceMaterialVersionPath(material.id, version: material.version) + "/maps/" + role.rawValue
        return try await floorPreviewFile(.init(path: path, bytes: map.bytes, sha256: map.sha256), requireDigestHeader: true)
    }

    public func prepareWorkspaceFloorMaterial(_ binding: WorkspaceFloorMaterialBinding, in document: NativeDocumentState) async throws -> NativeExhibitionDraft? {
        try await prepareWorkspaceSurfaceMaterial(binding.surface, in: document)
    }

    public func prepareWorkspaceSurfaceMaterial(_ binding: WorkspaceSurfaceMaterialBinding, in document: NativeDocumentState) async throws -> NativeExhibitionDraft? {
        guard binding.surfaceKind == .floor || workspaceWallMaterialRuntimeQualified else {
            throw AuthoringFailure("WORKSPACE_MATERIAL_BINDING_UNSUPPORTED")
        }
        guard admitsWorkspaceMaterials, document.tenantId == tenant, document.principalId == session?.userId else {
            throw AuthoringFailure("WORKSPACE_MATERIAL_UNAVAILABLE")
        }
        guard !document.dirty, let draft = document.draft, let etag = document.etag else {
            throw AuthoringFailure("WORKSPACE_MATERIAL_SAVED_DRAFT_REQUIRED")
        }
        let heldGeneration = generation
        let policy = try await nativeViewportPolicy(draft.id)
        try SurfaceMaterialTextureRequest.requireEditingPolicy(policy, document: document)
        let saved = try await exhibition(draft.id)
        guard saved.etag == etag, saved.draft == draft else { throw AuthoringFailure("EXHIBITION_COMPARISON_STALE") }
        let material = try await workspaceMaterial(binding.materialId, version: binding.materialVersion)
        guard generation == heldGeneration else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        let multimap = workspaceMaterialMultimapRuntimeQualified
        let next = try binding.applying(material, to: draft, multimap: multimap)
        if let next, policy.boundedProfile == true {
            try await validateNativeViewportEdit(next, etag: etag)
        }
        if multimap || next == nil || policy.boundedProfile != true {
            // The descriptor alone may carry a future or expired display grant.
            // Recheck each 040 map's bytes even when the Studio check verified the descriptor.
            for role in WorkspaceSurfaceMaterialBinding.supportedMapRoles(multimap: multimap)
                where material.maps[role.rawValue] != nil {
                guard generation == heldGeneration else { throw AuthoringFailure("ACCOUNT_CHANGED") }
                _ = try await workspaceMaterialPreview(material, role: role)
            }
        }
        guard generation == heldGeneration else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        return next
    }

    private func workspaceMaterialVersionPath(_ id: String, version: Int, catalogRead: Bool = false) throws -> String {
        guard (1...2_147_483_647).contains(version) else { throw AuthoringFailure("MATERIAL_DECLARATION") }
        return try workspaceMaterialPath("materials/" + identifier(id) + "/versions/" + String(version), catalogRead: catalogRead)
    }
    public var admitsWorkspaceFurnishings: Bool {
        workspaceFurnishingRuntimeQualified && nativeSession && !authenticationBusy
            && session?.workspaceFurnishingProfile == "workspace-furnishing-intake-v1"
            && ["admin", "artist", "curator"].contains(session?.role ?? "")
    }

    public var admitsWorkspaceArtworkFrames: Bool {
        admitsWorkspaceFurnishings && session?.advertisesWorkspaceArtworkFrame == true
    }

    private var supportedFurnishingExtensionVersion: Int64 {
        guard admitsWorkspaceFurnishings, let session else { return 0 }
        if session.advertisesWorkspaceArtworkFrame { return 4 }
        if session.advertisesWorkspaceFurnishingRoomVisual { return 3 }
        return session.advertisesWorkspaceFurnishingStool ? 2 : 1
    }

    private func workspaceFurnishingPath(_ suffix: String) throws -> String {
        guard admitsWorkspaceFurnishings else { throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE") }
        return "/api/v1/tenants/\(tenant)/workspace/" + suffix
    }

    private func furnishingIntakePath(_ id: String) throws -> String {
        guard workspaceFurnishingID(id) else { throw AuthoringFailure("IDENTIFIER_INVALID") }
        return try workspaceFurnishingPath("furnishing-intakes/" + id)
    }

    public func createWorkspaceFurnishing(requestId: String, details: WorkspaceFurnishingDetails) async throws -> WorkspaceFurnishingIntake {
        guard workspaceFurnishingID(requestId), details.isValid else { throw AuthoringFailure("FURNISHING_DECLARATION") }
        guard details.kind.minimumExtensionVersion <= supportedFurnishingExtensionVersion else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        if details.kind == .artworkFrame {
            guard details.model.bytes == WorkspaceArtworkFrameAttachment.thinBrassBytes,
                  details.model.sha256 == WorkspaceArtworkFrameAttachment.thinBrassSHA256 else {
                throw AuthoringFailure("FRAME_ASSET_UNSUPPORTED")
            }
        }
        let body = try encoded(WorkspaceFurnishingRequest(requestId: requestId, details: details))
        let intake: WorkspaceFurnishingIntake = try await request(workspaceFurnishingPath("furnishing-intakes"), method: "POST", body: body)
        try intake.check(requestId: requestId, details: details, supportedExtensionVersion: supportedFurnishingExtensionVersion)
        return intake
    }

    public func workspaceFurnishingIntake(_ id: String) async throws -> WorkspaceFurnishingIntake {
        let intake: WorkspaceFurnishingIntake = try await request(furnishingIntakePath(id))
        try intake.check(id: id, supportedExtensionVersion: supportedFurnishingExtensionVersion)
        return intake
    }

    public func uploadWorkspaceFurnishing(_ previous: WorkspaceFurnishingIntake,
                                          snapshot: ImportFileSnapshot) async throws -> WorkspaceFurnishingIntake {
        try previous.check(supportedExtensionVersion: supportedFurnishingExtensionVersion)
        let model = try WorkspaceFurnishingModel(snapshot: snapshot)
        guard model == previous.declaration.model else { throw AuthoringFailure("MODEL_CONFLICT") }
        let intake: WorkspaceFurnishingIntake = try await request(furnishingIntakePath(previous.intakeId) + "/model",
            method: "PUT", body: snapshot.data, contentType: "application/octet-stream", timeout: 120)
        try intake.check(id: previous.intakeId, requestId: previous.requestId, details: previous.declaration,
                         supportedExtensionVersion: supportedFurnishingExtensionVersion)
        guard intake.received != nil else { throw AuthoringFailure("FURNISHING_RESPONSE_INVALID") }
        return intake
    }

    public func workspaceFurnishingAction(_ previous: WorkspaceFurnishingIntake,
                                          _ action: WorkspaceFurnishingIntakeAction) async throws -> WorkspaceFurnishingIntake {
        try previous.check(supportedExtensionVersion: supportedFurnishingExtensionVersion)
        let intake: WorkspaceFurnishingIntake = try await request(furnishingIntakePath(previous.intakeId) + "/" + action.rawValue,
            method: "POST", body: Data("{}".utf8))
        try intake.check(id: previous.intakeId, requestId: previous.requestId, details: previous.declaration,
                         supportedExtensionVersion: supportedFurnishingExtensionVersion)
        return intake
    }

    private func furnishingPage<Item: Decodable & Sendable>(_ suffix: String, cursor: String?) async throws -> AuthoringPage<Item> {
        func valid(_ cursor: String) -> Bool {
            cursor.range(of: "^[A-Za-z0-9_-]{1,128}$", options: .regularExpression) != nil
        }
        guard cursor.map(valid) ?? true else { throw AuthoringFailure("INVALID_CURSOR") }
        let page: AuthoringPage<Item> = try await request(workspaceFurnishingPath(suffix + (cursor.map { "?cursor=" + $0 } ?? "")))
        guard page.items.count <= 50, page.nextCursor.map(valid) ?? true,
              page.nextCursor == nil || (!page.items.isEmpty && page.nextCursor != cursor) else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        return page
    }

    public func workspaceFurnishingIntakes(cursor: String? = nil) async throws -> AuthoringPage<WorkspaceFurnishingIntake> {
        let page: AuthoringPage<WorkspaceFurnishingIntake> = try await furnishingPage("furnishing-intakes", cursor: cursor)
        for intake in page.items { try intake.check(supportedExtensionVersion: supportedFurnishingExtensionVersion) }
        guard Set(page.items.map(\.intakeId)).count == page.items.count else { throw AuthoringFailure("FURNISHING_RESPONSE_INVALID") }
        return page
    }

    public func workspaceFurnishings(cursor: String? = nil) async throws -> AuthoringPage<WorkspaceFurnishingSummary> {
        let page: AuthoringPage<WorkspaceFurnishingSummary> = try await furnishingPage("furnishings", cursor: cursor)
        let version = supportedFurnishingExtensionVersion
        guard page.items.allSatisfy({ $0.isValid && $0.kind.minimumExtensionVersion <= version }),
              Set(page.items.map(\.id)).count == page.items.count else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        return page
    }

    private func workspaceFurnishingVersionPath(_ id: String, version: Int) throws -> String {
        guard workspaceFurnishingID(id), version == 1 else { throw AuthoringFailure("FURNISHING_DECLARATION") }
        return try workspaceFurnishingPath("furnishings/" + id + "/versions/1")
    }

    public func workspaceFurnishing(_ id: String, version: Int) async throws -> WorkspaceFurnishing {
        let furnishing: WorkspaceFurnishing = try await request(workspaceFurnishingVersionPath(id, version: version))
        guard furnishing.id == id, furnishing.details.kind.minimumExtensionVersion <= supportedFurnishingExtensionVersion else {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        return furnishing
    }

    public func workspaceFurnishingModel(_ furnishing: WorkspaceFurnishing) async throws -> Data {
        guard furnishing.details.kind.minimumExtensionVersion <= supportedFurnishingExtensionVersion else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        let path = try workspaceFurnishingVersionPath(furnishing.id, version: furnishing.version) + "/model"
        let model = furnishing.details.model
        return try await readPreviewBytes(path: path, expectedBytes: model.bytes, sha256: model.sha256,
            mime: model.mime, invalidCode: "FURNISHING_RESPONSE_INVALID", requireDigestHeader: true)
    }

    public func prepareWorkspaceFurnishingPlacement(_ placement: WorkspaceFurnishingPlacement,
                                                   in document: NativeDocumentState) async throws -> NativeExhibitionDraft? {
        guard document.pending == nil, document.tenantId == tenant, session?.userId == document.principalId,
              let draft = document.draft else { throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE") }
        let heldGeneration = generation
        let furnishing = try await workspaceFurnishing(placement.furnishingId, version: placement.furnishingVersion)
        let next = try placement.applying(furnishing, to: draft, supportedExtensionVersion: supportedFurnishingExtensionVersion)
        _ = try await workspaceFurnishingModel(furnishing)
        guard generation == heldGeneration else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        return next
    }

    private func validateNativeViewportEdit(_ draft: NativeExhibitionDraft, etag: String) async throws {
        struct Input: Encodable { let requestId: String; let draft: NativeExhibitionDraft }
        struct Validation: Decodable { let valid: Bool }
        let body = try encoded(Input(requestId: UUID().uuidString.lowercased(), draft: draft))
        guard body.count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        let path = studioPrefix + "/exhibitions/" + (try identifier(draft.id)) + "/native-viewport-check"
        let response: Validation = try await request(path, method: "POST", body: body, ifMatch: etag)
        guard response.valid else { throw AuthoringFailure("DRAFT_INVALID") }
    }

    public func prepareWorkspaceArtworkFrame(_ command: WorkspaceArtworkFrameCommand,
                                             in document: NativeDocumentState) async throws -> NativeExhibitionDraft? {
        guard admitsWorkspaceArtworkFrames, document.tenantId == tenant, document.principalId == session?.userId else {
            throw AuthoringFailure("WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        guard !document.dirty, let draft = document.draft, let etag = document.etag else {
            throw AuthoringFailure("FRAME_SAVED_DRAFT_REQUIRED")
        }
        let heldGeneration = generation
        let saved = try await exhibition(draft.id)
        guard saved.etag == etag, saved.draft == draft else { throw AuthoringFailure("REMOTE_CONFLICT", status: 412) }
        guard generation == heldGeneration else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        let furnishing: WorkspaceFurnishing?
        switch command {
        case .attach(let attachment):
            try attachment.checkTarget(in: saved.draft)
            furnishing = try await workspaceFurnishing(attachment.furnishingId, version: attachment.furnishingVersion)
        case .detach:
            furnishing = nil
        }
        guard generation == heldGeneration else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        let next = try command.applying(to: saved.draft, furnishing: furnishing)
        if let next { try await validateNativeViewportEdit(next, etag: etag) }
        guard generation == heldGeneration else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        return next
    }

    public func approveArtwork(_ id: String, revision: Int, assetId: String) async throws -> NativeArtwork {
        struct Approval: Encodable { let revision: Int; let assetId: String }
        return try await request(prefix + "/artworks/" + identifier(id) + "/approve", method: "POST", body: encoded(Approval(revision: revision, assetId: try identifier(assetId))))
    }

}

public enum NativeImportAction: String, Sendable { case complete, cancel, retry }
public struct NativeImagePreparationStatus: Decodable, Sendable {
    public let provenance: String
    public let originalSha256: String
    public let derivedSha256: String
    public let recipe: NativeImportDerivedRecipe
    private struct WireKey: CodingKey {
        let stringValue: String
        let intValue: Int? = nil
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { return nil }
    }
    private enum CodingKeys: String, CodingKey { case provenance, originalSha256, derivedSha256, recipe }
    public init(from decoder: Decoder) throws {
        func checkKeys(_ decoder: Decoder, expected: Set<String>) throws -> KeyedDecodingContainer<WireKey> {
            let values = try decoder.container(keyedBy: WireKey.self)
            guard Set(values.allKeys.map(\.stringValue)) == expected else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unexpected ICC readback shape"))
            }
            return values
        }
        _ = try checkKeys(decoder, expected: ["provenance", "originalSha256", "derivedSha256", "recipe"])
        let values = try decoder.container(keyedBy: CodingKeys.self)
        let recipeDecoder = try values.superDecoder(forKey: .recipe)
        let fields = try checkKeys(recipeDecoder, expected: ["version", "role", "sampling", "colourMeaning", "recipeSha256", "settingsSha256", "sourcePixelsSha256", "outputPixelsSha256", "toolVersion", "colourConversion"])
        let conversionDecoder = try fields.superDecoder(forKey: WireKey(stringValue: "colourConversion")!)
        _ = try checkKeys(conversionDecoder, expected: ["version", "algorithm", "sourceIccSha256"])
        provenance = try values.decode(String.self, forKey: .provenance)
        originalSha256 = try values.decode(String.self, forKey: .originalSha256)
        derivedSha256 = try values.decode(String.self, forKey: .derivedSha256)
        recipe = try values.decode(NativeImportDerivedRecipe.self, forKey: .recipe)
    }
    func matches(_ intent: NativeImportIntent) -> Bool {
        guard let derived = intent.derived, derived.recipe.colourConversion != nil else { return false }
        return provenance == "client-declared" && originalSha256 == intent.sha256
            && derivedSha256 == derived.sha256 && recipe == derived.recipe
    }
}
public struct NativeImportJob: Decodable, Sendable {
    public let id: String
    public let state: String
    public let progress: Int
    public let attempts: Int
    public let errorCode: String?
    public let assetId: String?
    public let retryAt: String?
    public let originalAssetId: String?
    public let modelDerivation: NativeModelDerivationStatus?
    public let imagePreparation: NativeImagePreparationStatus?
    public let workerDrainVersion: Int?
    public let workerDrained: Bool?
    private enum CodingKeys: String, CodingKey {
        case id, state, progress, attempts, errorCode, assetId, retryAt, originalAssetId, modelDerivation, imagePreparation, workerDrainVersion, workerDrained
    }
    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        id = try values.decode(String.self, forKey: .id)
        state = try values.decode(String.self, forKey: .state)
        progress = try values.decode(Int.self, forKey: .progress)
        attempts = try values.decode(Int.self, forKey: .attempts)
        errorCode = try values.decodeIfPresent(String.self, forKey: .errorCode)
        assetId = try values.decodeIfPresent(String.self, forKey: .assetId)
        retryAt = try values.decodeIfPresent(String.self, forKey: .retryAt)
        originalAssetId = try values.decodeIfPresent(String.self, forKey: .originalAssetId)
        modelDerivation = try values.decodeIfPresent(NativeModelDerivationStatus.self, forKey: .modelDerivation)
        // Legacy clients ignored this field. A malformed future field remains
        // absent here; selected ICC validation then rejects that absence.
        imagePreparation = try? values.decode(NativeImagePreparationStatus.self, forKey: .imagePreparation)
        workerDrainVersion = try? values.decode(Int.self, forKey: .workerDrainVersion)
        workerDrained = try? values.decode(Bool.self, forKey: .workerDrained)
    }
    public init(id: String, state: String, progress: Int, attempts: Int, errorCode: String?, assetId: String?, retryAt: String?, originalAssetId: String? = nil, modelDerivation: NativeModelDerivationStatus? = nil, imagePreparation: NativeImagePreparationStatus? = nil) {
        self.id = id; self.state = state; self.progress = progress; self.attempts = attempts; self.errorCode = errorCode
        self.assetId = assetId; self.retryAt = retryAt; self.originalAssetId = originalAssetId; self.modelDerivation = modelDerivation; self.imagePreparation = imagePreparation
        self.workerDrainVersion = nil; self.workerDrained = nil
    }
    public var terminal: Bool { ["approved", "cancelled"].contains(state) }
    public func confirmsIntakeDrain(for mime: String) -> Bool {
        switch mime {
        case "image/png": workerDrainVersion == 1 && workerDrained == true
        case "model/gltf-binary": modelDerivation?.workerDrained == true
        default: false
        }
    }
    public var canRetry: Bool {
        let optionalModelFailure = state == "approved" && modelDerivation?.status == "failed" && modelDerivation?.workerDrained == true
        guard state == "failed" || optionalModelFailure, attempts < 5 else { return false }
        guard let retryAt else { return true }
        let formatter = ISO8601DateFormatter(); formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let date = formatter.date(from: retryAt) ?? ISO8601DateFormatter().date(from: retryAt)
        return date.map { $0 <= Date() } ?? false
    }
}
/// Retain this exact intent/key on uncertain responses; changing bytes/rights
/// requires an explicit new intent rather than replaying a different payload.
public struct NativeImportIntent: Codable, Equatable, Sendable {
    public let artworkId: String
    public let idempotencyKey: String
    public let mime: String
    public let sha256: String
    public let bytes: Int
    public let scaleMeters: Double
    public let rights: ArtworkMetadata.Rights
    public let derived: NativeImportDerived?
    public let modelDerivation: NativeModelDerivationIntent?
    public init(artworkId: String, idempotencyKey: String = UUID().uuidString.lowercased(), mime: String, sha256: String, bytes: Int, scaleMeters: Double, rights: ArtworkMetadata.Rights, derived: NativeImportDerived? = nil, modelDerivation: NativeModelDerivationIntent? = nil) {
        self.artworkId = artworkId; self.idempotencyKey = idempotencyKey; self.mime = mime; self.sha256 = sha256; self.bytes = bytes; self.scaleMeters = scaleMeters; self.rights = rights; self.derived = derived; self.modelDerivation = modelDerivation
    }
}
