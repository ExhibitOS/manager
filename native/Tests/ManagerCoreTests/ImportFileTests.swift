// SPDX-License-Identifier: Apache-2.0
import XCTest
import Darwin
@testable import ManagerCore

@MainActor
final class ImportFileTests: XCTestCase {
    func testStaticOutputAdvisoryAppliesOnlyToGLBOver25MiB() {
        let cap = 25 * 1024 * 1024
        XCTAssertFalse(ImportFileSnapshot.mayExceedStaticOutputLimit(mime: "model/gltf-binary", bytes: cap))
        XCTAssertTrue(ImportFileSnapshot.mayExceedStaticOutputLimit(mime: "model/gltf-binary", bytes: cap + 1))
        XCTAssertTrue(ImportFileSnapshot.mayExceedStaticOutputLimit(mime: "model/gltf-binary", bytes: ImportFileSnapshot.limit))
        XCTAssertFalse(ImportFileSnapshot.mayExceedStaticOutputLimit(mime: "image/png", bytes: cap + 1))
    }
    func testImmutableBoundedSnapshotWithUnicodeFilename() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("한글 공백.GLb")
        try Data("abc".utf8).write(to: source)
        let snapshot = try await ImportFileSnapshot.load(source)
        XCTAssertEqual(snapshot.name, "한글 공백.GLb")
        XCTAssertEqual(snapshot.mime, "model/gltf-binary")
        XCTAssertEqual(snapshot.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        try Data("changed".utf8).write(to: source)
        XCTAssertTrue(snapshot.data == Data("abc".utf8))
    }
    func testJPEGRequiresExplicitRecipeOptInAndKeepsOriginalBytes() async throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID()).JpEg")
        let original = Data("synthetic JPEG transport bytes; reader does not decode".utf8)
        try original.write(to: url); defer { try? FileManager.default.removeItem(at: url) }
        do { _ = try await ImportFileSnapshot.load(url); XCTFail("Legacy/default chooser must not enable JPEG") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "FILE_FORMAT_UNSUPPORTED") }
        let held = try await ImportFileSnapshot.load(url, allowJPEG: true)
        try Data("source changed later".utf8).write(to: url)
        XCTAssertEqual(held.mime, "image/jpeg"); XCTAssertEqual(held.data, original)
        XCTAssertEqual(held.sha256, nativeImportSHA256(original))
    }
    func testRejectsSymlinkFIFOEmptyUnsupportedAndOversizeWithoutReading() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let original = root.appendingPathComponent("original.glb")
        try Data("synthetic".utf8).write(to: original)
        let link = root.appendingPathComponent("link.glb")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: original)
        let fifo = root.appendingPathComponent("fifo.glb")
        XCTAssertEqual(mkfifo(fifo.path, 0o600), 0)
        let empty = root.appendingPathComponent("empty.glb"); try Data().write(to: empty)
        let huge = root.appendingPathComponent("huge.glb"); try Data().write(to: huge)
        let writer = try FileHandle(forWritingTo: huge); try writer.truncate(atOffset: UInt64(ImportFileSnapshot.limit + 1)); try writer.close()
        let unsupported = root.appendingPathComponent("input.zip"); try Data("x".utf8).write(to: unsupported)
        for url in [link, fifo, empty, huge, unsupported] {
            do { _ = try await ImportFileSnapshot.load(url); XCTFail("Unsafe or unsupported input must fail") }
            catch is AuthoringFailure {}
        }
        XCTAssertTrue(try Data(contentsOf: original) == Data("synthetic".utf8))
    }
}
