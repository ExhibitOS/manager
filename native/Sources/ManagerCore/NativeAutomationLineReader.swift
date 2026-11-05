// SPDX-License-Identifier: Apache-2.0
import Foundation
import Darwin

/// POSIX short reads preserve interactive JSON-lines semantics. Foundation's
/// fixed-count pipe read can retain a short line until more bytes or EOF arrive.
public struct NativeAutomationLineReader {
    private let descriptor: Int32
    private let maximumBytes: Int
    private var buffered = Data()
    public init(descriptor: Int32, maximumBytes: Int = 3_145_728) {
        self.descriptor = descriptor; self.maximumBytes = maximumBytes
    }
    public mutating func next() throws -> Data? {
        guard (1...3_145_728).contains(maximumBytes) else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
        while true {
            if let newline = buffered.firstIndex(of: 10) {
                let end = buffered.index(after: newline)
                let line = Data(buffered[..<end]); buffered.removeSubrange(..<end)
                return line
            }
            guard buffered.count < maximumBytes else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
            var block = [UInt8](repeating: 0, count: min(4096, maximumBytes - buffered.count))
            let capacity = block.count
            let count = Darwin.read(descriptor, &block, capacity)
            if count < 0 {
                if errno == EINTR { continue }
                throw BridgeFailure("AUTOMATION_INPUT_FAILED")
            }
            if count == 0 {
                guard buffered.isEmpty else { throw BridgeFailure("AUTOMATION_PROTOCOL_INVALID") }
                return nil
            }
            buffered.append(contentsOf: block.prefix(count))
        }
    }
}
