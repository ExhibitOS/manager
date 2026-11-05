// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore
import Darwin
import Dispatch

struct Configuration: Decodable {
    let helper: String
    let profile: String
    let ports: [String: UInt16]
    let outputRoot: String
    let temporaryRoot: String
    let inputs: [String: String]
    let selectedOEX9: Bool?
    let selectedOEX10: Bool?
}
actor Driver {
    let session: NativeAutomationSession
    private var active: Task<Void, Never>?
    private var activeID: String?
    private var closed = false
    init(_ session: NativeAutomationSession) { self.session = session }
    func emit(_ value: DocumentValue) {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        if let bytes = try? encoder.encode(value) { FileHandle.standardOutput.write(bytes + Data([10])) }
    }
    func failure(_ id: String?, _ error: Error) {
        emit(.object(["version": .integer(1), "requestId": id.map(DocumentValue.string) ?? .null, "error": NativeAutomationFailure.value(error)]))
    }
    func receive(_ request: DocumentValue) async {
        let id = request["requestId"].string
        do {
            guard !closed, request["version"] == .integer(1), let id, UUID(uuidString: id) != nil else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
            if request["operation"].string == "status" {
                guard case .object(let fields) = request,
                      Set(fields.keys) == ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId"] else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
                try await session.authorizeControl(request)
                emit(.object(["version": .integer(1), "requestId": .string(id), "result": .object(["activeJobId": activeID.map(DocumentValue.string) ?? .null, "operationTaskRunning": .bool(active != nil), "closed": .bool(false), "native": try await session.nativeStatusValue()])]))
                return
            }
            if ["cancel", "close"].contains(request["operation"].string ?? "") {
                guard case .object(let fields) = request,
                      Set(fields.keys).isSubset(of: ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId", "jobId"]),
                      activeID == nil || request["jobId"].string == activeID else { throw BridgeFailure("AUTOMATION_JOB_CHANGED") }
                try await session.authorizeControl(request)
                // Only this process's in-flight operation is cancelled. Wait for
                // its structured task before EOF closes the owning helper.
                let task = active, cancelledID = activeID; task?.cancel(); await task?.value
                var runtimeDrained: DocumentValue = .null
                if request["operation"].string == "close" { try await session.close(); closed = true; runtimeDrained = .bool(true) }
                else if cancelledID != nil { runtimeDrained = .bool(try await session.cancelOwnedRuntime()) }
                emit(.object(["version": .integer(1), "requestId": .string(id), "result": .object(["cancelledJobId": cancelledID.map(DocumentValue.string) ?? .null, "operationTaskFinished": .bool(true), "runtimeDrained": runtimeDrained, "closed": .bool(closed)])]))
                return
            }
            guard active == nil else { throw BridgeFailure("AUTOMATION_BUSY") }
            activeID = id
            active = Task {
                do { let result = try await session.execute(request); self.emit(.object(["version": .integer(1), "requestId": .string(id), "result": result])) }
                catch { self.failure(id, error) }
                self.finished()
            }
        } catch { failure(id, error) }
    }
    private func finished() { active = nil; activeID = nil }
    func isClosed() -> Bool { closed }
    func eof() async -> Bool {
        let task = active; task?.cancel(); await task?.value
        do { try await session.close(); closed = true; return true }
        catch { failure(nil, error); return false }
    }
}

func run() async -> Int32 {
    guard CommandLine.arguments.count == 3, CommandLine.arguments[1] == "--configuration" else {
        FileHandle.standardError.write(Data("AUTOMATION_CONFIGURATION_REQUIRED\n".utf8)); return 2
    }
    var session: NativeAutomationSession?
    do {
        let url = URL(fileURLWithPath: CommandLine.arguments[2])
        guard url.path.hasPrefix("/"), url.resolvingSymlinksInPath() == url,
              let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size < 65_536 else { throw BridgeFailure("AUTOMATION_CONFIGURATION_INVALID") }
        let data = try Data(contentsOf: url)
        let fields = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        let names = Set(fields?.keys.map { $0 } ?? [])
        guard names.subtracting(["selectedOEX9", "selectedOEX10"]) == ["helper", "profile", "ports", "outputRoot", "temporaryRoot", "inputs"] else { throw BridgeFailure("AUTOMATION_CONFIGURATION_INVALID") }
        let config = try JSONDecoder().decode(Configuration.self, from: data)
        guard setenv("TMPDIR", config.temporaryRoot, 1) == 0 else { throw BridgeFailure("AUTOMATION_PATH_INVALID") }
        let owned = try NativeAutomationSession(helper: URL(fileURLWithPath: config.helper), profile: URL(fileURLWithPath: config.profile), ports: config.ports, outputRoot: URL(fileURLWithPath: config.outputRoot), temporaryRoot: URL(fileURLWithPath: config.temporaryRoot), inputs: config.inputs, selectedOEX9: config.selectedOEX9 ?? false, selectedOEX10: config.selectedOEX10 ?? false)
        session = owned
        let driver = Driver(owned)
        await driver.emit(.object(["version": .integer(1), "event": .string("ready"), "result": try await owned.open()]))
        // A dedicated blocking reader keeps cancel/close available while an
        // async operation owns the authenticated API or export worker.
        let consumed = DispatchSemaphore(value: 0)
        let lines = AsyncThrowingStream<Data, Error>(bufferingPolicy: .bufferingOldest(1)) { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                var reader = NativeAutomationLineReader(descriptor: STDIN_FILENO)
                do {
                    while let line = try reader.next() {
                        switch continuation.yield(line) {
                        case .enqueued: consumed.wait() // one bounded frame; pipe supplies backpressure
                        case .dropped: throw BridgeFailure("AUTOMATION_QUEUE_OVERFLOW")
                        case .terminated: return
                        @unknown default: throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID")
                        }
                    }
                    continuation.finish()
                }
                catch { continuation.finish(throwing: error) }
            }
        }
        var protocolFailed = false
        do {
            for try await line in lines {
                do { await driver.receive(try JSONDecoder().decode(DocumentValue.self, from: line)) }
                catch { protocolFailed = true; await driver.failure(nil, BridgeFailure("AUTOMATION_PROTOCOL_INVALID")) }
                consumed.signal()
                if await driver.isClosed() { break }
            }
        } catch { protocolFailed = true; await driver.failure(nil, error) }
        let drained = await driver.eof()
        return protocolFailed ? 2 : drained ? 0 : 1
    } catch {
        if let session { try? await session.close() }
        let code = (error as? BridgeFailure)?.code ?? (error as? AuthoringFailure)?.code ?? "AUTOMATION_FAILED"
        FileHandle.standardError.write(Data((code + "\n").utf8)); return 1
    }
}
exit(await run())
