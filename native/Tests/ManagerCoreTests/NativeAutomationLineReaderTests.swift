// SPDX-License-Identifier: Apache-2.0
import XCTest
import Darwin
@testable import ManagerCore

final class NativeAutomationLineReaderTests: XCTestCase {
    func testShortLineReturnsBeforeWriterCloses() throws {
        let pipe = Pipe(), received = expectation(description: "short open-pipe line")
        let descriptor = dup(pipe.fileHandleForReading.fileDescriptor)
        DispatchQueue.global().async {
            defer { Darwin.close(descriptor) }
            var reader = NativeAutomationLineReader(descriptor: descriptor)
            do { XCTAssertEqual(try reader.next(), Data("{\"request\":1}\n".utf8)) }
            catch { XCTFail("Short read failed: \(error)") }
            received.fulfill()
        }
        defer { try? pipe.fileHandleForWriting.close(); try? pipe.fileHandleForReading.close() }
        try pipe.fileHandleForWriting.write(contentsOf: Data("{\"request\":1}\n".utf8))
        wait(for: [received], timeout: 2) // Writer remains open throughout.
    }
    func testFragmentedUTF8AndCoalescedLinesPreserveExactFramesAndEOF() throws {
        let pipe = Pipe()
        defer { try? pipe.fileHandleForReading.close() }
        let first = Data("{\"이름\":1}\n".utf8), second = Data("{}\n".utf8)
        try pipe.fileHandleForWriting.write(contentsOf: first.prefix(4))
        try pipe.fileHandleForWriting.write(contentsOf: first.dropFirst(4) + second)
        try pipe.fileHandleForWriting.close()
        var reader = NativeAutomationLineReader(descriptor: pipe.fileHandleForReading.fileDescriptor)
        XCTAssertEqual(try reader.next(), first); XCTAssertEqual(try reader.next(), second); XCTAssertNil(try reader.next())
    }
    func testTruncatedAndOversizedFramesRefuseWithoutExecutingThem() throws {
        for bytes in [Data("abc".utf8), Data(repeating: 120, count: 9)] {
            let pipe = Pipe()
            defer { try? pipe.fileHandleForReading.close() }
            try pipe.fileHandleForWriting.write(contentsOf: bytes); try pipe.fileHandleForWriting.close()
            var reader = NativeAutomationLineReader(descriptor: pipe.fileHandleForReading.fileDescriptor, maximumBytes: 8)
            XCTAssertThrowsError(try reader.next()) { XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_PROTOCOL_INVALID") }
        }
    }
}
