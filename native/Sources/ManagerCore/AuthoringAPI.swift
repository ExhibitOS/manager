// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct AuthoringSession: Decodable, Sendable {
    public let userId: String
    public let tenantId: String
    public let role: String
    public let csrfToken: String
    public let expiresAt: String
}
public struct AuthoringPage<Item: Decodable & Sendable>: Decodable, Sendable {
    public let items: [Item]
    public let nextCursor: String?
}
public struct NativeArtist: Decodable, Sendable, Identifiable {
    public let id: String
    public let userId: String?
    public let revision: Int
    public let name: String
    public let bio: String
    public let archived: Bool
}
public struct ArtworkSummary: Decodable, Sendable, Identifiable {
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
    public var title = "", description = ""
    public var medium: String?
    public var creationYear: Int?
    public var dimensions = Dimensions()
    public var rights = Rights()
    public var provenance = Provenance()
    public init() {}
}
public struct NativeArtwork: Decodable, Sendable, Identifiable {
    public struct Asset: Decodable, Sendable, Identifiable {
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
    public init(_ code: String, status: Int = 0) { self.code = code; self.status = status }
    public var errorDescription: String? {
        switch code {
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
        case "ARCHITECTURE_SELECTION_CHANGED": return "선택한 대상이 바뀌었습니다. 입력을 취소한 뒤 대상을 다시 선택하세요."
        case "PLACEMENT_APPLY_REQUIRED": return "배치와 위치 입력을 먼저 적용하거나 취소하세요. 입력과 기존 전시는 그대로 유지됩니다."
        case "ARCHITECTURE_APPLY_REQUIRED": return "공간·재질 변경을 적용하거나 입력을 취소한 뒤 전시를 저장하세요."
        case "FILE_SIZE_INVALID": return "파일은 1바이트에서 32 MiB까지 지원합니다."
        case "FILE_FORMAT_UNSUPPORTED": return "GLB 또는 PNG 파일을 선택하세요."
        case "FILE_CHANGED": return "읽는 동안 파일이 바뀌었습니다. 저장을 마친 뒤 다시 선택하세요."
        case "FILE_READ_FAILED", "FILE_REGULAR_REQUIRED": return "파일을 읽을 수 없습니다. 파일 위치와 접근 권한을 확인하세요."
        case "EXHIBITION_EXPORT_SAVE_REQUIRED": return "현재 입력을 전시 저장본에 반영한 뒤 내보내세요."
        case "VIEWER_ENTRANCE_INVALID": return "시작 화면의 작가·소개·이미지 설정을 확인하세요. 작가는 512자, 소개는 4,096자, 입장권 제목은 160자, 안내는 512자, 이미지 설명은 2,048자까지 지원하며, 표지는 전시에 포함한 8 MiB 이하 PNG와 설명이 필요합니다."
        case "PUBLICATION_REVIEW_REQUIRED": return "현재 저장본을 확인하고 작품의 표시 권리를 확인하세요."
        case "PUBLICATION_PLAN_CHANGED": return "공개 준비를 시작한 실행본 또는 설정과 현재 상태가 다릅니다. 보관한 요청·식별자와 이전 공개본은 유지했습니다. 같은 준비 상태에서 다시 확인하세요."
        case "WATERMARK_FIELDS_INVALID": return "워터마크 문구·위치·크기·진하기를 확인하세요. 크기는 8–24 px, 불투명도는 0.1–0.6입니다."
        case "WATERMARK_TEXT_UNSUPPORTED": return "사본 삽입은 지원하는 영문·숫자·기호 80자까지 사용합니다. 다른 언어는 화면 표시를 선택하세요."
        case "WATERMARK_RUNTIME_UNAVAILABLE": return "이 실행본은 새 워터마크 설정을 지원하지 않습니다. 입력과 저장 요청을 보존했습니다. 지원 실행본에서 다시 확인하세요."
        case "PUBLICATION_RESPONSE_INVALID": return "공개 준비 상태를 확인하지 못했습니다. 같은 요청으로 다시 확인하세요."
        case "PUBLICATION_NOT_READY": return "공개 준비에 필요한 항목이 남아 있습니다. 작품의 승인·표시 권리와 전시 정보를 확인하세요."
        case "PUBLISHED_DRAFT_IMMUTABLE": return "이 공개용 저장본은 변경할 수 없습니다. 원래 제작 전시에서 편집하세요."
        case "OEX_LIMIT": return "내보내기 파일은 최대 64 MiB를 지원합니다. 전시의 작품 파일 크기를 확인하세요."
        case "RIGHTS_DENIED": return "내보내기를 허용하지 않은 작품이 있습니다. 작품의 권리와 내보내기 허용을 확인하세요."
        case "OEX_SOURCE_CHANGED", "ASSET_INTEGRITY": return "저장한 전시와 현재 작품 파일 또는 승인 상태가 다릅니다. 작품을 확인한 뒤 전시를 다시 저장하세요."
        case "OEX_PROFILE_INVALID", "OEX_EXTENSION_UNSUPPORTED", "OEX_LEGAL_NOTICE_INVALID", "OEX_INVALID": return "현재 전시에 OEX로 내보낼 수 없는 정보가 있습니다. 전시와 작품 정보를 확인하세요."
        case "OEX_RESPONSE_INVALID": return "내보내기 파일을 확인하지 못했습니다. 저장하지 않았습니다. 다시 시도하세요."
        case "EXPORT_WRITE_FAILED": return "파일을 저장하지 못했습니다. 저장 위치와 여유 공간을 확인하고 다시 시도하세요."
        case "IMPORT_DERIVED_REQUIRED": return "같은 JPEG 원본을 다시 선택해 변환 파일을 준비하세요. 저장한 요청은 유지됩니다."
        case "IMPORT_CONTEXT_CHANGED": return "작업 공간이나 파일 검사 요청이 바뀌었습니다. 저장한 요청을 다시 확인하세요."
        case "IMPORT_DRAIN_PENDING": return "작품 파일 처리의 종료를 아직 확인하지 못했습니다. 저장한 요청과 원본을 유지한 채 상태를 확인하거나 로컬 제작을 정상 정지하세요."
        case "IMPORT_MODEL_BINDING_INVALID": return "표시 사본 요청의 파일·설정·작업 식별 정보가 일치하지 않습니다. 저장한 원본과 요청을 유지했습니다."
        case "IMPORT_MODEL_UNAVAILABLE": return "이 실행본은 저장한 표시 사본 요청을 지원하지 않습니다. 원본과 요청을 보존했으니 지원 실행본에서 다시 확인하세요."
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
private struct APIErrorEnvelope: Decodable { let code: String }
private final class LocalRedirectBlocker: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

/// A private in-memory cookie jar per selected native installation. Callers get
/// typed CMS operations, never an arbitrary authenticated URL request API.
public actor AuthoringAPI {
    private let base: URL
    private let tenant: String
    private let transport: URLSession
    private var session: AuthoringSession?
    private var generation: UInt64 = 0
    private var authenticationBusy = false
    private var activeRequests = 0

    public init(port: UInt16, tenantId: String) throws {
        guard port >= 1024, let id = UUID(uuidString: tenantId) else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        base = URL(string: "http://127.0.0.1:\(port)")!
        tenant = id.uuidString.lowercased()
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
        for cookie in transport.configuration.httpCookieStorage?.cookies ?? [] { transport.configuration.httpCookieStorage?.deleteCookie(cookie) }
    }
    private func identifier(_ value: String) throws -> String {
        guard let id = UUID(uuidString: value) else { throw AuthoringFailure("IDENTIFIER_INVALID") }
        return id.uuidString.lowercased()
    }
    private func encoded<T: Encodable>(_ body: T) throws -> Data { try JSONEncoder().encode(body) }
    private func request<T: Decodable & Sendable>(_ path: String, method: String = "GET", body: Data? = nil, authenticated: Bool = true, contentType: String = "application/json", ifMatch: String? = nil, timeout: TimeInterval = 15) async throws -> T {
        guard !(authenticated && authenticationBusy && path != "/api/v1/auth/logout") else { throw AuthoringFailure("AUTH_BUSY") }
        activeRequests += 1
        defer { activeRequests -= 1 }
        let current = generation
        if authenticated && session == nil { throw AuthoringFailure("AUTH_REQUIRED", status: 401) }
        guard let url = URL(string: path, relativeTo: base)?.absoluteURL, url.host == "127.0.0.1", url.port == base.port, url.scheme == "http", url.user == nil, url.password == nil else { throw AuthoringFailure("LOCAL_SCOPE_INVALID") }
        var request = URLRequest(url: url, timeoutInterval: timeout)
        request.httpMethod = method; request.httpBody = body
        if let ifMatch {
            guard ifMatch.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil else { throw AuthoringFailure("ETAG_INVALID") }
            request.setValue(ifMatch, forHTTPHeaderField: "If-Match")
        }
        request.setValue(base.absoluteString, forHTTPHeaderField: "Origin")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if body != nil { request.setValue(contentType, forHTTPHeaderField: "Content-Type") }
        if method != "GET", authenticated, let session { request.setValue(session.csrfToken, forHTTPHeaderField: "x-csrf-token") }
        let (bytes, response) = try await transport.bytes(for: request)
        guard let http = response as? HTTPURLResponse, http.url == url else { throw AuthoringFailure("RESPONSE_INVALID") }
        var data = Data()
        for try await byte in bytes {
            guard data.count < 2 * 1024 * 1024 else { throw AuthoringFailure("RESPONSE_LIMIT") }
            data.append(byte)
        }
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        guard (200..<300).contains(http.statusCode) else {
            let code = (try? JSONDecoder().decode(APIErrorEnvelope.self, from: data).code) ?? "REQUEST_FAILED"
            if http.statusCode == 401 { generation &+= 1; clear() }
            throw AuthoringFailure(code.range(of: "^[A-Z0-9_]{1,80}$", options: .regularExpression) == nil ? "REQUEST_FAILED" : code, status: http.statusCode)
        }
        let result = try JSONDecoder().decode(T.self, from: data)
        if let studio = result as? NativeStudioResponse {
            guard studio.revision > 0, studio.revision <= 2_147_483_647,
                  studio.etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil,
                  studio.etag.hasPrefix("\"studio-r\(studio.revision)-"), http.value(forHTTPHeaderField: "ETag") == studio.etag else { throw AuthoringFailure("ETAG_INVALID") }
        }
        return result
    }
    public func furnishings() async throws -> Bool {
        let value: DocumentValue = try await request("/api/v1/tenants/\(tenant)/studio/furnishings")
        return try FurnishingCatalog.checkedCatalog(value)
    }
    public func surfaceMaterials() async throws -> NativeSurfaceMaterialCatalog {
        let value: DocumentValue = try await request("/api/v1/tenants/\(tenant)/studio/materials")
        return try NativeSurfaceMaterialCatalog(value: value, tenantId: tenant)
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
        return value
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
    public func publicationReady(exhibitionId: String) async throws -> NativePublicationReady {
        let result: NativePublicationReady = try await request(studioPrefix + "/exhibitions/" + identifier(exhibitionId) + "/ready", timeout: 120)
        return try result.validated()
    }
    public func preparePublication(exhibitionId: String, etag: String, requestId: String) async throws -> NativePublication {
        struct Input: Encodable { let requestId: String }
        let result: NativePublication = try await request(studioPrefix + "/exhibitions/" + identifier(exhibitionId) + "/publications", method: "POST", body: encoded(Input(requestId: identifier(requestId))), ifMatch: etag, timeout: 120)
        return try result.validated()
    }
    public func publications(exhibitionId: String) async throws -> [NativePublication] {
        struct Page: Decodable, Sendable { let items: [NativePublication] }
        let result: Page = try await request(studioPrefix + "/exhibitions/" + identifier(exhibitionId) + "/publications")
        guard result.items.count <= 100 else { throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID") }
        return try result.items.map { try $0.validated() }
    }
    /// Export only a saved exact revision through the existing rights/integrity gate.
    /// Bounded streaming prevents a malformed local response from allocating an unbounded archive.
    public func exportOEX(exhibitionId: String, etag: String) async throws -> NativeOEXArchive {
        guard !authenticationBusy, let session else { throw AuthoringFailure("AUTH_REQUIRED", status: 401) }
        guard etag.range(of: #"^"studio-r[1-9][0-9]*-[a-f0-9]{64}"$"#, options: .regularExpression) != nil else { throw AuthoringFailure("ETAG_INVALID") }
        let id = try identifier(exhibitionId)
        let url = base.appendingPathComponent(studioPrefix + "/exhibitions/" + id + "/oex/export")
        activeRequests += 1
        defer { activeRequests -= 1 }
        let current = generation
        var request = URLRequest(url: url, timeoutInterval: 120)
        request.httpMethod = "POST"; request.httpBody = Data("{}".utf8)
        request.setValue(base.absoluteString, forHTTPHeaderField: "Origin")
        request.setValue(session.csrfToken, forHTTPHeaderField: "x-csrf-token")
        request.setValue(etag, forHTTPHeaderField: "If-Match")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/vnd.exhibitos.oex+zip", forHTTPHeaderField: "Accept")
        request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        let (bytes, response) = try await transport.bytes(for: request)
        defer { bytes.task.cancel() } // also cancels early header/size rejection
        guard let http = response as? HTTPURLResponse, http.url == url else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        let success = http.statusCode == 200
        let limit = success ? NativeOEXArchive.maximumBytes : 64 * 1024
        guard http.expectedContentLength <= Int64(limit) else { throw AuthoringFailure(success ? "OEX_LIMIT" : "RESPONSE_LIMIT") }
        if success {
            guard http.mimeType?.lowercased() == NativeOEXArchive.mime,
                  http.value(forHTTPHeaderField: "Content-Encoding").map({ $0.lowercased() == "identity" }) ?? true else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        }
        var data = Data()
        if http.expectedContentLength > 0 { data.reserveCapacity(Int(http.expectedContentLength)) }
        try await withTaskCancellationHandler {
            for try await byte in bytes {
                try Task.checkCancellation()
                guard data.count < limit else { throw AuthoringFailure(success ? "OEX_LIMIT" : "RESPONSE_LIMIT") }
                data.append(byte)
            }
        } onCancel: { bytes.task.cancel() }
        guard generation == current else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        guard success else {
            let code = (try? JSONDecoder().decode(APIErrorEnvelope.self, from: data).code) ?? "REQUEST_FAILED"
            if http.statusCode == 401 { generation &+= 1; clear() }
            throw AuthoringFailure(code.range(of: "^[A-Z0-9_]{1,80}$", options: .regularExpression) == nil ? "REQUEST_FAILED" : code, status: http.statusCode)
        }
        guard http.expectedContentLength < 0 || Int64(data.count) == http.expectedContentLength else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        return try NativeOEXArchive(data: data)
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
    public func approveArtwork(_ id: String, revision: Int, assetId: String) async throws -> NativeArtwork {
        struct Approval: Encodable { let revision: Int; let assetId: String }
        return try await request(prefix + "/artworks/" + identifier(id) + "/approve", method: "POST", body: encoded(Approval(revision: revision, assetId: try identifier(assetId))))
    }

}

public enum NativeImportAction: String, Sendable { case complete, cancel, retry }
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
    public init(id: String, state: String, progress: Int, attempts: Int, errorCode: String?, assetId: String?, retryAt: String?, originalAssetId: String? = nil, modelDerivation: NativeModelDerivationStatus? = nil) {
        self.id = id; self.state = state; self.progress = progress; self.attempts = attempts; self.errorCode = errorCode
        self.assetId = assetId; self.retryAt = retryAt; self.originalAssetId = originalAssetId; self.modelDerivation = modelDerivation
    }
    public var terminal: Bool { ["approved", "cancelled"].contains(state) }
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
