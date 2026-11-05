// swift-tools-version: 6.0
// SPDX-License-Identifier: Apache-2.0
import PackageDescription

let package = Package(
    name: "ExhibitOSManager",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "ExhibitOSManager", targets: ["ManagerApp"]), .executable(name: "exhibitos-automation", targets: ["ManagerAutomation"])],
    targets: [
        .target(name: "ManagerCore"),
        .executableTarget(name: "ManagerApp", dependencies: ["ManagerCore"]),
        .executableTarget(name: "ManagerAutomation", dependencies: ["ManagerCore"]),
        .testTarget(name: "ManagerCoreTests", dependencies: ["ManagerCore"]),
        .testTarget(name: "ManagerHostTests", dependencies: ["ManagerApp", "ManagerCore"])
    ]
)
