// SPDX-License-Identifier: Apache-2.0
import CoreFoundation
import Foundation

enum NativeThumbnailPreviewTrace {
    static func validRequestID(_ id: String) -> Bool {
        guard let uuid = UUID(uuidString: id), uuid.uuidString.lowercased() == id else { return false }
        let fields = id.split(separator: "-")
        return fields[2].first == "4" && fields[3].first.map { "89ab".contains($0) } == true
    }

    static func webTrace(_ raw: Any?, requestID: String) -> DocumentValue? {
        guard let input = raw as? [String: Any],
              Set(input.keys) == ["version", "requestId", "anchorUnixMs", "dispatchElapsedMs", "remainingBudgetEstimateMs", "terminal"],
              input["requestId"] as? String == requestID, integer(input["version"], in: 1...1) == 1,
              let anchor = integer(input["anchorUnixMs"], in: 1...9_007_199_254_740_991),
              let dispatch = integer(input["dispatchElapsedMs"], in: 0...60_000),
              let remaining = integer(input["remainingBudgetEstimateMs"], in: 0...10_000),
              let terminal = input["terminal"] as? [String: Any],
              let kind = terminal["kind"] as? String,
              let elapsed = integer(terminal["elapsedMs"], in: dispatch...60_000) else { return nil }
        var event: [String: DocumentValue] = ["kind": .string(kind), "elapsedMs": .integer(elapsed)]
        switch kind {
        case "headers":
            guard let status = integer(terminal["status"], in: 100...599) else { return nil }
            event["status"] = .integer(status)
        case "cancelled":
            guard let reason = terminal["reason"] as? String, ["timeout", "cancelled"].contains(reason) else { return nil }
            event["reason"] = .string(reason)
        case "failed": break
        default: return nil
        }
        guard Set(terminal.keys) == Set(event.keys) else { return nil }
        return .object(["version": .integer(1), "requestId": .string(requestID), "anchorUnixMs": .integer(anchor),
            "dispatchElapsedMs": .integer(dispatch), "remainingBudgetEstimateMs": .integer(remaining),
            "terminal": .object(event)])
    }

    private static func integer(_ raw: Any?, in allowed: ClosedRange<Int64>) -> Int64? {
        guard let number = raw as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        let value = number.doubleValue
        guard value.isFinite, value.rounded(.towardZero) == value,
              value >= Double(allowed.lowerBound), value <= Double(allowed.upperBound) else { return nil }
        return Int64(value)
    }
}
