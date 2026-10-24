// swift-tools-version: 6.0
// SPDX-License-Identifier: Apache-2.0
import PackageDescription

let package = Package(
    name: "ExhibitOSManager",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "ExhibitOSManager", targets: ["ManagerApp"])],
    targets: [
        .target(name: "ManagerCore"),
        .executableTarget(name: "ManagerApp", dependencies: ["ManagerCore"]),
        .testTarget(name: "ManagerCoreTests", dependencies: ["ManagerCore"])
    ]
)
