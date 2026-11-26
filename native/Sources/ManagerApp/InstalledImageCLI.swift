// SPDX-License-Identifier: Apache-2.0
import Darwin
import Dispatch
import Foundation
import ManagerCore
#if EXHIBITOS_LOCAL_INSTALLED_CLI
import Security
#endif

#if EXHIBITOS_LOCAL_INSTALLED_CLI && !DEBUG
#error("The local installed CLI is a debug-only build")
#endif

@main
struct ManagerEntryPoint {
    @MainActor
    static func main() async {
        let arguments = Array(CommandLine.arguments.dropFirst())
        let bundle = Bundle.main
        let marker = bundle.infoDictionary?["ExhibitOSLocalInstalledCLI"]
        #if EXHIBITOS_LOCAL_INSTALLED_CLI
        guard let marker, let localRuntime = InstalledImageCLI.localValidationRuntime(
            marker, bundle: bundle.bundleURL, arguments: arguments
        ) else {
            FileHandle.standardError.write(Data("LOCAL_INSTALLED_CLI_SCOPE_INVALID\n".utf8))
            exit(2)
        }
        #else
        guard marker == nil else { exit(2) }
        let localRuntime: String? = nil
        #endif
        switch arguments.first {
        #if !EXHIBITOS_LOCAL_INSTALLED_CLI
        case "--image-cli":
            exit(await InstalledImageCLI.run(Array(arguments.dropFirst())))
        #endif
        case "--authoring-cli":
            exit(await InstalledImageCLI.runExport(Array(arguments.dropFirst()), authoring: true, localRuntime: localRuntime))
        case "--export-cli":
            exit(await InstalledImageCLI.runExport(Array(arguments.dropFirst()), localRuntime: localRuntime))
        default:
            #if EXHIBITOS_LOCAL_INSTALLED_CLI
            exit(2)
            #else
            ManagerApp.main()
            #endif
        }
    }
}

enum InstalledImageCLI {
    #if EXHIBITOS_LOCAL_INSTALLED_CLI
    static func localValidationRuntime(_ marker: Any, bundle: URL, arguments: [String],
                                       verifyCode: (URL, String, Bool) -> Bool = verifyLocalCode) -> String? {
        guard let fields = marker as? [String: String],
              Set(fields.keys) == ["version", "root", "runtimeManifestSHA256"],
              fields["version"] == "org.exhibitos.manager.local-installed-cli/v1",
              let root = fields["root"], root.hasPrefix("/"),
              let digest = fields["runtimeManifestSHA256"],
              bundle.path == root + "/ExhibitOS Manager Local CLI.app",
              bundle.resolvingSymlinksInPath().path == bundle.path,
              let operation = arguments.first, ["--authoring-cli", "--export-cli"].contains(operation),
              privateLocalDirectory(root) else { return nil }
        let helper = bundle.appendingPathComponent("Contents/Helpers/exhibitos-native")
        guard helper.resolvingSymlinksInPath().path == helper.path,
              verifyCode(bundle, "org.exhibitos.manager", true),
              verifyCode(helper, "org.exhibitos.manager.native", false) else { return nil }
        if Array(arguments.dropFirst()) != ["--help"] {
            let profile = root + "/profile", output = root + "/output"
            guard Array(arguments.dropFirst()) == ["--profile", profile, "--output-root", output],
                  privateLocalDirectory(profile), privateLocalDirectory(output) else { return nil }
        }
        let manifest = bundle.appendingPathComponent("Contents/Resources/native-runtime/manifest.json")
        var info = stat()
        guard manifest.resolvingSymlinksInPath().path == manifest.path,
              lstat(manifest.path, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
              info.st_size > 0, info.st_size <= 16 * 1024 * 1024,
              let bytes = try? Data(contentsOf: manifest), NativeAutomationSession.digest(bytes) == digest else { return nil }
        return digest
    }

    private static func verifyLocalCode(_ path: URL, identifier: String, nested: Bool) -> Bool {
        var staticCode: SecStaticCode?
        var information: CFDictionary?
        let options = kSecCSCheckAllArchitectures | kSecCSStrictValidate | (nested ? kSecCSCheckNestedCode : 0)
        guard SecStaticCodeCreateWithPath(path as CFURL, [], &staticCode) == errSecSuccess,
              let staticCode,
              SecStaticCodeCheckValidity(staticCode, SecCSFlags(rawValue: options), nil) == errSecSuccess,
              SecCodeCopySigningInformation(staticCode, SecCSFlags(rawValue: kSecCSSigningInformation),
                                            &information) == errSecSuccess,
              let sealed = information as? [String: Any],
              sealed[kSecCodeInfoIdentifier as String] as? String == identifier,
              let flags = sealed[kSecCodeInfoFlags as String] as? NSNumber,
              flags.uint32Value & SecCodeSignatureFlags.adhoc.rawValue != 0 else { return false }
        return sealed[kSecCodeInfoEntitlements as String] == nil
            && sealed[kSecCodeInfoEntitlementsDict as String] == nil
            && sealed[kSecCodeInfoTeamIdentifier as String] == nil
    }

    private static func privateLocalDirectory(_ path: String) -> Bool {
        var info = stat()
        return URL(fileURLWithPath: path).resolvingSymlinksInPath().path == path
            && lstat(path, &info) == 0 && info.st_mode & S_IFMT == S_IFDIR
            && info.st_mode & 0o777 == 0o700 && info.st_uid == getuid()
    }
    #endif

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
        return await run(profilePath: profilePath)
    }

    static func runExport(_ arguments: [String], authoring: Bool = false, localRuntime: String? = nil) async -> Int32 {
        let usage = "Usage: ExhibitOSManager \(authoring ? "--authoring-cli" : "--export-cli") --profile ABSOLUTE_PATH --output-root ABSOLUTE_PATH"
        if arguments == ["--help"] {
            print("""
                \(usage)
                Commands: \(authoring ? "read, exhibition-new, architecture-apply, save, reopen-saved, pedestal/light/plaque-prepare, -apply, -cancel, thumbnail-prepare, thumbnail-status, thumbnail-select." : "read, export-oex, export-static, export-static-status.")
                Send one JSON request per line.
                Use a saved personal workspace and a separate owner-only output directory on the same volume.
                End stdin to close and drain the owned runtime.
                """)
            return 0
        }
        guard arguments.count == 4, arguments[0] == "--profile", arguments[1].hasPrefix("/"),
              arguments[2] == "--output-root", arguments[3].hasPrefix("/") else {
            FileHandle.standardError.write(Data((usage + "\n").utf8))
            return 2
        }
        let output = URL(fileURLWithPath: arguments[3], isDirectory: true)
        guard output.path == arguments[3] else {
            report(BridgeFailure("AUTOMATION_OUTPUT_INVALID"))
            return 2
        }
        return await run(profilePath: arguments[1], outputRoot: output, authoring: authoring, localRuntime: localRuntime)
    }

    private static func run(profilePath: String?, outputRoot: URL? = nil, authoring: Bool = false,
                            localRuntime: String? = nil) async -> Int32 {
        // Let the CLI cancel the request and close the pipe. The helper retains
        // its own signal and EOF shutdown paths.
        signal(SIGPIPE, SIG_IGN)
        let signals = [SIGINT, SIGTERM].map { number in
            signal(number, SIG_IGN)
            return DispatchSource.makeSignalSource(signal: number, queue: .global())
        }
        let task = Task { await runSession(profilePath: profilePath, outputRoot: outputRoot,
                                           authoring: authoring, localRuntime: localRuntime) }
        for source in signals {
            source.setEventHandler { task.cancel() }
            source.resume()
        }
        let status = await task.value
        for source in signals { source.cancel() }
        return status
    }

    private static func runSession(profilePath: String?, outputRoot: URL?, authoring: Bool,
                                   localRuntime: String?) async -> Int32 {
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
            let owner: NativeAutomationSession
            if authoring, let outputRoot {
                owner = try NativeAutomationSession(installedAuthoringCLIHelper: helper, profile: profile, outputRoot: outputRoot,
                                                    localRuntimeManifest: localRuntime)
            } else if let outputRoot {
                owner = try NativeAutomationSession(installedExportCLIHelper: helper, profile: profile, outputRoot: outputRoot,
                                                    localRuntimeManifest: localRuntime)
            } else {
                owner = try NativeAutomationSession(installedImageCLIHelper: helper, profile: profile,
                                                    requireExistingManagedProfile: profilePath != nil)
            }
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
