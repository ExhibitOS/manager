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

        for command in ["--authoring-cli", "--export-cli"] {
            XCTAssertEqual(InstalledImageCLI.localValidationRuntime(marker, bundle: app,
                                                                    arguments: [command] + scopeArgs), digest)
        }
        XCTAssertNil(InstalledImageCLI.localValidationRuntime(marker, bundle: app, arguments: []))
        XCTAssertNil(InstalledImageCLI.localValidationRuntime(marker, bundle: app,
                                                              arguments: ["--image-cli", "--help"]))
        XCTAssertNil(InstalledImageCLI.localValidationRuntime(marker, bundle: app, arguments: [
            "--authoring-cli", "--profile", root.path, "--output-root", output.path,
        ]))
        XCTAssertNil(InstalledImageCLI.localValidationRuntime(marker, bundle: root,
                                                              arguments: ["--authoring-cli"] + scopeArgs))

        try FileManager.default.removeItem(at: profile)
        try FileManager.default.createSymbolicLink(at: profile, withDestinationURL: output)
        XCTAssertNil(InstalledImageCLI.localValidationRuntime(marker, bundle: app,
                                                              arguments: ["--authoring-cli"] + scopeArgs))

        try Data("mismatched inventory".utf8).write(to: manifest)
        XCTAssertNil(InstalledImageCLI.localValidationRuntime(marker, bundle: app,
                                                              arguments: ["--authoring-cli", "--help"]))
    }
}
#endif
