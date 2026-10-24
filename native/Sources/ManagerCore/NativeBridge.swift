// SPDX-License-Identifier: Apache-2.0
import Foundation
import Darwin

public enum Mode: String, Codable, CaseIterable, Sendable { case beginner, expert }
public enum Step: String, Codable, CaseIterable, Sendable { case prepare, create, select, preview, publish }
public struct Presentation: Codable, Equatable, Sendable {
    public var format: Int
    public var revision: UInt64
    public var mode: Mode
    public var step: Step
    public var textScale: Int
    public init(format: Int = 1, revision: UInt64 = 0, mode: Mode = .beginner, step: Step = .prepare, textScale: Int = 100) {
        self.format = format; self.revision = revision; self.mode = mode; self.step = step; self.textScale = textScale
    }
}
public struct Installation: Decodable, Sendable, Identifiable {
    public let id: String
    public let kind: String
    public let path: String
    public let available: Bool
}
public struct Context: Decodable, Sendable {
    public let selectionToken: String
    public let activeId: String
    public let installations: [Installation]
}
public struct Job: Decodable, Sendable, Identifiable {
    public let id: String
    public let action: String
    public let state: String
    public let guidance: String?
}
public struct RuntimeStatus: Decodable, Sendable {
    public struct Readiness: Decodable, Sendable { public let ready: Bool }
    public let installed: Bool
    public let state: String
    public let version: String?
    public let readiness: Readiness
}
public struct NativePorts: Decodable, Sendable {
    public let web: UInt16
    public let api: UInt16
    public let database: UInt16
}
public struct NativeConfiguration: Decodable, Sendable {
    public let kind: String?
    public let tenantId: String?
    public let subject: String?
    public let ports: NativePorts
    public let manifestSha256: String
}
public struct NativeRuntime: Decodable, Sendable {
    public struct Readiness: Decodable, Sendable { public let ready: Bool }
    public let jobId: String?
    public let state: String
    public let ports: NativePorts
    public let readiness: Readiness
    public let errorCode: String?
    public let failureCode: String?
    public let postgresPid: UInt32?
    public let nodePid: UInt32?
    public let auxiliaryPid: UInt32?
    public let exits: [String: Int]
    public let logTruncated: Bool
    public var hasOwnedProcesses: Bool { postgresPid != nil || nodePid != nil || auxiliaryPid != nil }
}
public struct NativeHostStatus: Decodable, Sendable {
    public struct Backup: Decodable, Sendable { public let id: String, backupId: String, manifestSha256: String, state: String }
    public struct Restoration: Decodable, Sendable { public let id: String, state: String; public let backupId: String? }
    public let busy: Bool
    public let operation: String
    public let configuration: NativeConfiguration?
    public let accountRequired: Bool
    public let runtime: NativeRuntime?
    public let errorCode: String?
    public let backup: Backup?
    public let restoration: Restoration?
    public var needsShutdown: Bool { busy || runtime?.hasOwnedProcesses == true }
}
public struct LocalAuthoringSession: Decodable, Sendable {
    public let tenantId: String
    public let principalId: String
    public let origin: String
    public let token: String
}
public struct BridgeFailure: Error, LocalizedError, Sendable {
    public let code: String
    public init(_ code: String) { self.code = code }
    public var errorDescription: String? {
        switch code {
        case "NATIVE_STATIC_OUTPUT_EXISTS": "이미 같은 위치에 파일이 있습니다. 기존 파일을 보관하고 다른 위치를 선택하세요."
        case "NATIVE_STATIC_OUTPUT_INVALID": "저장 위치를 사용할 수 없습니다. 접근할 수 있는 폴더를 선택하세요."
        case "NATIVE_STATIC_EXPORT_UNAVAILABLE": "이 앱에는 정적 전시 저장 도구가 없습니다. 검증된 최신 앱을 설치하세요."
        case "NATIVE_STATIC_STOP_PENDING": "전시 파일 저장을 정지하고 있습니다. 완료될 때까지 앱을 열어 두세요."
        case "NATIVE_STATIC_EXPORT_FAILED", "NATIVE_STATIC_EXPORT_INVALID": "전시 파일을 완성하지 못했습니다. 기존 파일을 보관하고 새 폴더에 다시 저장하세요."
        case "PROFILE_BUSY", "MANAGER_PROFILE_BUSY": "다른 Manager가 열려 있습니다. 해당 앱을 닫은 뒤 다시 확인하세요."
        case "NATIVE_HELPER_MISSING": "앱의 실행 파일을 찾을 수 없습니다. 앱을 다시 설치하세요."
        case "NATIVE_UI_STATE_CHANGED": "설정이 바뀌었습니다. 다시 확인한 뒤 선택하세요."
        case "INITIAL_ACCOUNT_INVALID": "계정 형식과 비밀번호를 확인하세요. 영문·숫자 등으로 계정을 만들고 12자 이상의 비밀번호를 입력하세요."
        case "NATIVE_MIGRATION_REQUIRED": "기존 설치를 찾았습니다. 데이터 이전을 마친 뒤 네이티브 제작을 시작할 수 있습니다."
        case "NATIVE_RECOVERY_REQUIRED": "이전 로컬 작업이 남아 있습니다. 데이터를 보존한 채 복구 상태를 확인해야 합니다."
        case "NATIVE_STOP_REQUIRED": "로컬 제작을 정지한 뒤 다시 진행하세요."
        case "BACKUP_KEY_NEW_FILE_REQUIRED": "같은 이름의 파일이 있습니다. 키 파일을 덮어쓰지 않았습니다. 새 이름으로 저장하세요."
        case "BACKUP_KEY_PATH_INVALID": "키 파일은 작업 공간과 백업 폴더 밖의 안전한 위치에 저장하세요."
        case "BACKUP_KEY_WRITE_UNCERTAIN": "키 파일 저장을 확인하지 못했습니다. 생성된 파일을 보관하고 다시 확인하세요."
        case "NATIVE_RESTORE_TARGET_NOT_EMPTY": "이 작업 공간에는 데이터가 있습니다. 새 복구 작업 공간을 선택하세요."
        case "NATIVE_RESTORE_PENDING": "진행 중인 복원을 먼저 확인하세요. 기존 데이터와 복원 후보는 보관됩니다."
        case "NATIVE_RESTORE_CANDIDATE_CHANGED": "복원 후보나 백업이 이전 요청과 다릅니다. 원래 백업과 작업 공간을 다시 선택하세요."
        case "NATIVE_BACKUP_RESOURCE_BUDGET": "이 작업에 필요한 여유 공간이 부족합니다. 기존 백업을 보관하고 저장 위치를 확인하세요."
        case "NATIVE_PORT_IN_USE": "다른 앱이 필요한 로컬 연결을 사용하고 있습니다. 해당 작업을 마친 뒤 다시 시작하세요."
        case "NATIVE_CANCELLED": "시작을 중단했습니다. 저장된 설정을 사용해 다시 시작할 수 있습니다."
        case "NATIVE_STOP_PENDING": "로컬 작업이 아직 정지 중입니다. 정지가 끝날 때까지 앱을 열어 두세요."
        case "NATIVE_PROCESS_EXITED": "로컬 제작 서비스가 종료되었습니다. 정지가 끝나면 다시 시작하세요."
        case "NATIVE_RUNTIME_DISTRIBUTION_PENDING": "이 실행 패키지는 배포 검증이 필요합니다. 검증된 앱을 설치하세요."
        case "NATIVE_TIMEOUT": "응답이 늦어지고 있습니다. 저장된 작업은 유지됩니다. 앱을 다시 열어 확인하세요."
        default: "상태를 확인하지 못했습니다. 기존 데이터를 보존하고 다시 확인하세요."
        }
    }
}

/// One inherited pipe session owns the existing Rust profile lock. There is no
/// network listener, shell, PATH lookup, or user-selected executable.
public actor NativeBridge {
    private let process: Process
    private let input: FileHandle
    private let output: FileHandle
    private var sequence: UInt64 = 0
    private var buffered = Data()

    public init(helper: URL, profile: URL) throws {
        guard helper.isFileURL, profile.isFileURL,
              FileManager.default.isExecutableFile(atPath: helper.path) else {
            throw BridgeFailure("NATIVE_HELPER_MISSING")
        }
        let process = Process()
        let incoming = Pipe(), outgoing = Pipe()
        process.executableURL = helper
        process.arguments = ["--profile", profile.path]
        // Legacy status may inspect system-installed engines, but never inherits
        // development PATH, credential environment, or a development checkout.
        process.environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": NSHomeDirectory(), "LANG": "en_US.UTF-8"]
        process.standardInput = incoming
        process.standardOutput = outgoing
        process.standardError = FileHandle.nullDevice
        self.process = process
        self.input = incoming.fileHandleForWriting
        self.output = outgoing.fileHandleForReading
        try process.run()
    }

    deinit {
        // EOF tells the helper to stop its owned native services before it
        // releases the profile. Closing a window keeps this bridge alive.
        try? input.close()
        try? output.close()
    }

    /// EOF requests owned shutdown; wait for the helper to release its profile
    /// before another native presentation reopens that same workspace.
    public func close() throws {
        try input.close()
        process.waitUntilExit()
        try output.close()
        guard process.terminationStatus == 0 else { throw BridgeFailure("NATIVE_HELPER_EXITED") }
    }
    public func context() throws -> Context { try request("context") }
    public func presentation() throws -> Presentation { try request("presentation") }
    public func save(_ state: Presentation) throws -> Presentation {
        let object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state))
        return try request("save_presentation", fields: ["state": object])
    }
    public func status(_ context: Context) throws -> RuntimeStatus {
        try request("status", fields: ["selection_token": context.selectionToken])
    }
    public func jobs(_ context: Context) throws -> [Job] {
        try request("jobs", fields: ["selection_token": context.selectionToken])
    }

    public func authoringSession(_ context: Context) throws -> LocalAuthoringSession {
        try request("native_authoring_session", fields: ["selection_token": context.selectionToken])
    }
    public func publicationState(_ context: Context) throws -> NativePublicationState {
        try request("publication_state", fields: ["selection_token": context.selectionToken])
    }
    public func staticExportStatus(_ context: Context) throws -> NativeStaticExportStatus? {
        try request("static_export_status", fields: ["selection_token": context.selectionToken])
    }
    public func startStaticExport(_ context: Context, publicationId: String, output: URL) throws -> NativeStaticExportStatus {
        guard output.isFileURL else { throw BridgeFailure("NATIVE_STATIC_OUTPUT_INVALID") }
        return try request("static_export", fields: ["selection_token": context.selectionToken, "export": ["publicationId": publicationId, "output": output.path]])
    }
    public func cancelStaticExport(_ context: Context) throws -> NativeStaticExportStatus? {
        try request("static_export_cancel", fields: ["selection_token": context.selectionToken])
    }
    public func savePublicationState(_ context: Context, state: NativePublicationState) throws -> NativePublicationState {
        let object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state))
        return try request("save_publication_state", fields: ["selection_token": context.selectionToken, "publication": object])
    }
    public func documentState(_ context: Context) throws -> NativeDocumentState {
        try request("document_state", fields: ["selection_token": context.selectionToken])
    }
    public func saveDocumentState(_ context: Context, state: NativeDocumentState) throws -> NativeDocumentState {
        let object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state))
        return try request("save_document_state", fields: ["selection_token": context.selectionToken, "document": object])
    }
    public func uploadState(_ context: Context) throws -> NativeUploadState {
        try request("upload_state", fields: ["selection_token": context.selectionToken])
    }
    public func saveUploadState(_ context: Context, state: NativeUploadState) throws -> NativeUploadState {
        let object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state))
        return try request("save_upload_state", fields: ["selection_token": context.selectionToken, "transfer": object])
    }
    public func nativeStatus(_ context: Context) throws -> NativeHostStatus {
        try request("native_status", fields: ["selection_token": context.selectionToken])
    }
    public func prepare(_ context: Context, subject: String? = nil, password: String? = nil) throws -> NativeHostStatus {
        var fields: [String: Any] = ["selection_token": context.selectionToken]
        if let subject, let password { fields["account"] = ["subject": subject, "password": password] }
        return try request("native_prepare", fields: fields)
    }
    public func start(_ context: Context) throws -> NativeHostStatus {
        try request("native_start", fields: ["selection_token": context.selectionToken])
    }
    public func stop(_ context: Context) throws -> NativeHostStatus {
        try request("native_stop", fields: ["selection_token": context.selectionToken])
    }
    public func createBackupKey(_ context: Context, at path: URL) throws {
        guard path.isFileURL else { throw BridgeFailure("NATIVE_BACKUP_INPUT_INVALID") }
        let _: DocumentValue = try request("native_maintenance", fields: ["selection_token": context.selectionToken, "maintenance": ["action": "create_key", "path": path.path]])
    }
    public func createRecoveryWorkspace(_ context: Context) throws -> Context {
        try request("native_maintenance", fields: ["selection_token": context.selectionToken, "maintenance": ["action": "create_recovery", "acknowledged": true]])
    }
    public func selectWorkspace(_ context: Context, id: String) throws -> Context {
        try request("native_maintenance", fields: ["selection_token": context.selectionToken, "maintenance": ["action": "select", "id": id, "acknowledged": true]])
    }
    public func backup(_ context: Context, key: URL, destination: URL) throws -> NativeHostStatus {
        guard key.isFileURL, destination.isFileURL else { throw BridgeFailure("NATIVE_BACKUP_INPUT_INVALID") }
        return try request("native_backup", fields: ["selection_token": context.selectionToken, "backup": ["keyFile": key.path, "destination": destination.path]])
    }
    public func restore(_ context: Context, key: URL, source: URL) throws -> NativeHostStatus {
        guard key.isFileURL, source.isFileURL else { throw BridgeFailure("NATIVE_BACKUP_INPUT_INVALID") }
        return try request("native_maintenance", fields: ["selection_token": context.selectionToken, "maintenance": ["action": "stage_restore", "source": source.path, "keyFile": key.path]])
    }
    public func activateRestoration(_ context: Context, id: String) throws -> NativeHostStatus {
        try request("native_maintenance", fields: ["selection_token": context.selectionToken, "maintenance": ["action": "activate_restore", "id": id]])
    }

    private func request<T: Decodable>(_ command: String, fields: [String: Any] = [:]) throws -> T {
        guard process.isRunning else { throw BridgeFailure("NATIVE_HELPER_EXITED") }
        try Task.checkCancellation()
        sequence += 1
        var frame = fields
        frame["version"] = 1; frame["id"] = sequence; frame["command"] = command
        var data = try JSONSerialization.data(withJSONObject: frame)
        guard data.count < 3_145_728 else { throw BridgeFailure("NATIVE_PROTOCOL_INVALID") }
        data.append(10)
        defer { data.resetBytes(in: data.startIndex..<data.endIndex) }
        try input.write(contentsOf: data)
        // Once a frame is sent, drain its reply even if the calling view
        // disappears. Cancelling one view must not terminate the shared helper
        // or leave the next transaction reading this response.
        let deadline = ContinuousClock.now.advanced(by: .seconds(60))
        while !buffered.contains(10) {
            if ContinuousClock.now >= deadline {
                process.terminate()
                throw BridgeFailure("NATIVE_TIMEOUT")
            }
            var descriptor = pollfd(fd: output.fileDescriptor, events: Int16(POLLIN), revents: 0)
            let ready = Darwin.poll(&descriptor, 1, 100)
            if ready < 0 && errno == EINTR { continue }
            guard ready >= 0 else { throw BridgeFailure("NATIVE_PROTOCOL_INVALID") }
            if ready == 0 { continue }
            // Foundation's read(upToCount:) can fill the requested length on
            // a pipe, waiting beyond our deadline after a short JSON response.
            // A single POSIX read returns the bytes currently available.
            var bytes = [UInt8](repeating: 0, count: 4096)
            let count = Darwin.read(output.fileDescriptor, &bytes, bytes.count)
            if count < 0 && errno == EINTR { continue }
            guard count > 0 else {
                throw BridgeFailure("NATIVE_HELPER_EXITED")
            }
            buffered.append(contentsOf: bytes.prefix(count))
            guard buffered.count <= 4 * 1024 * 1024 else {
                process.terminate(); throw BridgeFailure("NATIVE_PROTOCOL_INVALID")
            }
        }
        let newline = buffered.firstIndex(of: 10)!
        let response = buffered.prefix(upTo: newline)
        buffered.removeSubrange(...newline)
        guard let object = try JSONSerialization.jsonObject(with: response) as? [String: Any],
              (object["version"] as? Int) == 1,
              (object["id"] as? UInt64) == sequence else { throw BridgeFailure("NATIVE_PROTOCOL_INVALID") }
        if let error = object["error"] as? [String: Any] {
            throw BridgeFailure(error["code"] as? String ?? "NATIVE_PROTOCOL_INVALID")
        }
        guard let result = object["result"] else { throw BridgeFailure("NATIVE_PROTOCOL_INVALID") }
        return try JSONDecoder().decode(T.self, from: JSONSerialization.data(withJSONObject: result, options: .fragmentsAllowed))
    }
}
