// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeThumbnailPersistenceTests: XCTestCase {
    func testPersistedRequestRefusesUnknownVersionAndInvalidIdentity() throws {
        let id = UUID().uuidString.lowercased()
        var request: [String: Any] = ["version": 1, "epoch": id, "tenantId": id, "principalId": id,
            "requestId": id, "artworkId": id, "sequence": 1, "expectedRevision": 1]
        func decode() throws -> ViewportThumbnailRequest {
            try JSONDecoder().decode(ViewportThumbnailRequest.self, from: JSONSerialization.data(withJSONObject: request))
        }
        XCTAssertEqual(try decode().version, 1)
        request["version"] = 2
        XCTAssertThrowsError(try decode())
        request["version"] = 1; request["artworkId"] = "invalid"
        XCTAssertThrowsError(try decode())
    }
    func testInterruptedPreviewResumesOnlyTheSameVerifiedBytes() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let name = "thumbnail-\(UUID().uuidString).png", bytes = Data([1, 2, 3, 4])
        let file = root.appendingPathComponent(name)
        try Data(bytes.prefix(2)).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        try NativeThumbnailCandidate.materializePreview(bytes, root: root, name: name)
        XCTAssertEqual(try Data(contentsOf: file), bytes)
        try NativeThumbnailCandidate.materializePreview(bytes, root: root, name: name)
        XCTAssertEqual(try Data(contentsOf: file), bytes)
        try FileManager.default.removeItem(at: file)
        try NativeThumbnailCandidate.materializePreview(bytes, root: root, name: name)
        XCTAssertEqual(try Data(contentsOf: file), bytes)
    }

    func testForeignPreviewAndSymlinkAreRejectedWithoutReplacement() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let name = "thumbnail-\(UUID().uuidString).png", file = root.appendingPathComponent(name)
        let foreign = Data([9, 8])
        try foreign.write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        XCTAssertThrowsError(try NativeThumbnailCandidate.materializePreview(Data([1, 2, 3]), root: root, name: name))
        XCTAssertEqual(try Data(contentsOf: file), foreign)
        let link = "thumbnail-\(UUID().uuidString).png"
        try FileManager.default.createSymbolicLink(atPath: root.appendingPathComponent(link).path, withDestinationPath: file.path)
        XCTAssertThrowsError(try NativeThumbnailCandidate.materializePreview(Data([1, 2, 3]), root: root, name: link))
        XCTAssertEqual(try Data(contentsOf: file), foreign)
        XCTAssertThrowsError(try NativeThumbnailCandidate.materializePreview(Data([1]), root: root, name: "../thumbnail-escape.png"))
    }
}
