// SPDX-License-Identifier: Apache-2.0
#if EXHIBITOS_LOCAL_INSTALLED_CLI
import Foundation
import ManagerCore
import XCTest
@testable import ManagerApp

final class LocalInstalledCLIScopeTests: XCTestCase {
    func testLocalBundleAcceptsOnlyItsOwnEntryAndDisjointManagedPaths() throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let app = root.appendingPathComponent("ExhibitOS Manager Local CLI.app")
        let runtime = app.appendingPathComponent("Contents/Resources/native-runtime")
        let profile = root.appendingPathComponent("profile")
        let output = root.appendingPathComponent("output")
        for dir in [root, runtime, profile, output] {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true,
                                                    attributes: [.posixPermissions: 0o700])
        }
        let manifest = runtime.appendingPathComponent("manifest.json")
        let data = Data("local test inventory".utf8)
        try data.write(to: manifest)
        let digest = NativeAutomationSession.digest(data)
        let marker = ["version": "org.exhibitos.manager.local-installed-cli/v1",
                      "root": root.path, "runtimeManifestSHA256": digest]
        let scopeArgs = ["--profile", profile.path, "--output-root", output.path]
        let helper = app.appendingPathComponent("Contents/Helpers/exhibitos-native")
        try FileManager.default.createDirectory(at: helper.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        let helperBytes = Data("unsigned fixture".utf8)
        try helperBytes.write(to: helper)
        func admitted(_ arguments: [String], at bundle: URL? = nil,
                      validApp: Bool = true, validHelper: Bool = true) -> String? {
            InstalledImageCLI.localValidationRuntime(marker, bundle: bundle ?? app, arguments: arguments) {
                path, identifier, nested in
                if nested {
                    return validApp && path.path == app.path && identifier == "org.exhibitos.manager"
                }
                return validHelper && path.path == helper.path && identifier == "org.exhibitos.manager.native"
                    && (try? Data(contentsOf: path)) == helperBytes
            }
        }

        for command in ["--authoring-cli", "--export-cli"] {
            XCTAssertEqual(admitted([command] + scopeArgs), digest)
        }
        let scopedAuthoring = ["--authoring-cli"] + scopeArgs
        XCTAssertNil(InstalledImageCLI.localValidationRuntime(marker, bundle: app, arguments: scopedAuthoring))
        XCTAssertNil(admitted(scopedAuthoring, validApp: false))
        XCTAssertNil(admitted(scopedAuthoring, validHelper: false))
        try Data("changed nested fixture".utf8).write(to: helper)
        XCTAssertNil(admitted(scopedAuthoring))
        try helperBytes.write(to: helper)
        XCTAssertNil(admitted([]))
        XCTAssertNil(admitted(["--image-cli", "--help"]))
        XCTAssertNil(admitted([
            "--authoring-cli", "--profile", root.path, "--output-root", output.path,
        ]))
        XCTAssertNil(admitted(scopedAuthoring, at: root))

        try FileManager.default.removeItem(at: profile)
        try FileManager.default.createSymbolicLink(at: profile, withDestinationURL: output)
        XCTAssertNil(admitted(scopedAuthoring))

        try Data("mismatched inventory".utf8).write(to: manifest)
        XCTAssertNil(admitted(["--authoring-cli", "--help"]))
    }
}
#endif
