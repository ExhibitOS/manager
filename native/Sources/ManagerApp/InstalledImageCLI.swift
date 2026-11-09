// SPDX-License-Identifier: Apache-2.0
import Darwin
import Dispatch
import Foundation
import ManagerCore

@main
struct ManagerEntryPoint {
    @MainActor
    static func main() async {
        let arguments = Array(CommandLine.arguments.dropFirst())
        guard arguments.first == "--image-cli" else {
            ManagerApp.main()
            return
        }
        exit(await InstalledImageCLI.run(Array(arguments.dropFirst())))
    }
}

enum InstalledImageCLI {
    static func run(_ arguments: [String]) async -> Int32 {
        if arguments == ["--help"] {
            print("""
                Usage: ExhibitOSManager --image-cli [--profile ABSOLUTE_PATH]
                Send one JSON request per line and read one response before sending another.
                Commands: read, import-image, artwork-status, artwork-image-preparation-retry,
                          artwork-approve, artwork-register, save, artwork-finish-reviewed.
                Use a prepared personal workspace. Omit --profile to use the default.
                Quit Manager before using this command.
                End stdin to close and drain the owned runtime.
                """)
            return 0
        }
        let profilePath: String?
        if arguments.isEmpty {
            profilePath = nil
        } else if arguments.count == 2, arguments[0] == "--profile", arguments[1].hasPrefix("/") {
            profilePath = arguments[1]
        } else {
            FileHandle.standardError.write(Data("IMAGE_CLI_USAGE: ExhibitOSManager --image-cli [--help | --profile ABSOLUTE_PATH]\n".utf8))
            return 2
        }
        // Let the CLI cancel the request and close the pipe. The helper retains
        // its own signal and EOF shutdown paths.
        signal(SIGPIPE, SIG_IGN)
        let signals = [SIGINT, SIGTERM].map { number in
            signal(number, SIG_IGN)
            return DispatchSource.makeSignalSource(signal: number, queue: .global())
        }
        let task = Task { await runSession(profilePath: profilePath) }
        for source in signals {
            source.setEventHandler { task.cancel() }
            source.resume()
        }
        let status = await task.value
        for source in signals { source.cancel() }
        return status
    }

    private static func runSession(profilePath: String?) async -> Int32 {
        var session: NativeAutomationSession?
        var status: Int32 = 0
        do {
            guard Bundle.main.bundleIdentifier == "org.exhibitos.manager" else {
                throw BridgeFailure("IMAGE_CLI_INSTALLED_APP_REQUIRED")
            }
            let helper = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/exhibitos-native")
            let profile: URL
            if let profilePath {
                profile = URL(fileURLWithPath: profilePath, isDirectory: true)
                guard profile.resolvingSymlinksInPath().path == profilePath else {
                    throw BridgeFailure("AUTOMATION_SCOPE_CHANGED")
                }
            } else {
                profile = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                       appropriateFor: nil, create: false)
                    .appendingPathComponent("org.exhibitos.manager", isDirectory: true)
            }
            let owner = try NativeAutomationSession(installedImageCLIHelper: helper, profile: profile,
                                                    requireExistingManagedProfile: profilePath != nil)
            session = owner
            try emit(["event": .string("ready"), "result": try await owner.open()])
            let consumed = DispatchSemaphore(value: 0)
            let lines = AsyncThrowingStream<Data, Error>(bufferingPolicy: .bufferingOldest(1)) { continuation in
                DispatchQueue.global().async {
                    var reader = NativeAutomationLineReader(descriptor: STDIN_FILENO, maximumBytes: 65_536)
                    do {
                        while let line = try reader.next() {
                            guard case .enqueued = continuation.yield(line) else { break }
                            consumed.wait()
                        }
                        continuation.finish()
                    } catch { continuation.finish(throwing: error) }
                }
            }
            for try await line in lines {
                defer { consumed.signal() }
                try Task.checkCancellation()
                let request: DocumentValue
                do { request = try JSONDecoder().decode(DocumentValue.self, from: line) }
                catch {
                    try emit(["requestId": .null, "error": NativeAutomationFailure.value(BridgeFailure("AUTOMATION_PROTOCOL_INVALID"))])
                    continue
                }
                let result: DocumentValue
                do { result = try await owner.execute(request) }
                catch {
                    let id = request["requestId"].string.flatMap { UUID(uuidString: $0) == nil ? nil : $0 }
                    try emit(["requestId": id.map(DocumentValue.string) ?? .null, "error": NativeAutomationFailure.value(error)])
                    continue
                }
                try emit(["requestId": request["requestId"], "result": result])
            }
            try Task.checkCancellation()
        } catch {
            status = error is CancellationError ? 130 : 1
            report(error)
        }
        if let session {
            do { try await session.close() }
            catch { report(error); return 1 }
        }
        return status
    }

    private static func emit(_ payload: [String: DocumentValue]) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        var frame = payload
        frame["version"] = .integer(1)
        try FileHandle.standardOutput.write(contentsOf: encoder.encode(DocumentValue.object(frame)) + Data([10]))
    }

    private static func report(_ error: Error) {
        let code = NativeAutomationFailure.value(error)["code"].string ?? "AUTOMATION_FAILED"
        FileHandle.standardError.write(Data((code + "\n").utf8))
    }
}
