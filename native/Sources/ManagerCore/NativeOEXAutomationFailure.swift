// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Safe observations for a failed CLI export, never a retry or owner-release proof.
struct NativeOEXAutomationFailure: Error, Sendable {
    enum Phase: String, Sendable { case requesting, packing, checkingPackage, receiving, verifying, writing }
    let phase: Phase
    let operationId: String?
    let receivedBytes: Int?
    let totalBytes: Int?
    let errorDomain: String
    let errorCode: Int?

    init(_ error: Error, phase: Phase, operationId: String?, receivedBytes: Int?, totalBytes: Int?) {
        self.phase = phase
        self.operationId = operationId.flatMap { UUID(uuidString: $0)?.uuidString.lowercased() == $0 ? $0 : nil }
        let limit = NativeOEXImportFile.boundedMaximumBytes
        self.receivedBytes = receivedBytes.flatMap { (0...limit).contains($0) ? $0 : nil }
        self.totalBytes = totalBytes.flatMap { (1...limit).contains($0) ? $0 : nil }
        let value = error as NSError
        switch value.domain {
        case NSURLErrorDomain:
            errorDomain = "url"
            errorCode = [-1, -999, -1000, -1001, -1002, -1003, -1004, -1005, -1006, -1007, -1008, -1009, -1011, -1015, -1016, -1017, -1020, -1200].contains(value.code) ? value.code : nil
        case NSCocoaErrorDomain:
            errorDomain = "cocoa"
            errorCode = [4, 256, 257, 260, 512, 513, 516, 640, 642, 768, 769].contains(value.code) ? value.code : nil
        case NSPOSIXErrorDomain:
            errorDomain = "posix"
            errorCode = [2, 5, 9, 13, 28, 32, 54, 60].contains(value.code) ? value.code : nil
        default:
            errorDomain = "other"; errorCode = nil
        }
    }
}

@MainActor
final class NativeOEXAutomationObservation {
    private(set) var phase: NativeOEXAutomationFailure.Phase = .requesting
    private(set) var receivedBytes: Int?
    private(set) var totalBytes: Int?
    func update(_ value: NativeOEXExportProgress) {
        switch value {
        case .packing: phase = .packing
        case .checkingPackage: phase = .checkingPackage
        case .receiving(let bytes, let total): phase = .receiving; receivedBytes = bytes; totalBytes = total
        case .verifying: phase = .verifying
        }
    }
    func writing() { phase = .writing }
    func failure(_ error: Error, operationId: String?) -> NativeOEXAutomationFailure {
        .init(error, phase: phase, operationId: operationId, receivedBytes: receivedBytes, totalBytes: totalBytes)
    }
}
