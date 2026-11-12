// SPDX-License-Identifier: Apache-2.0
#if DEBUG
import Foundation
import CoreFoundation

/// Bounded diagnostics only. An accepted record is never an authority or receipt.
public struct NativeGPUDecodeObservationLimiter {
    public private(set) var accepted = 0
    public init() {}

    public mutating func accept(_ body: Any) -> String? {
        guard accepted < 12, JSONSerialization.isValidJSONObject(body),
              let data = try? JSONSerialization.data(withJSONObject: body, options: [.sortedKeys]),
              data.count <= 4096, let object = body as? [String: Any],
              Self.valid(object), let text = String(data: data, encoding: .utf8) else { return nil }
        accepted += 1
        return text
    }

    private static func integer(_ value: Any?, max: Int) -> Int? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite, number.doubleValue.rounded(.towardZero) == number.doubleValue,
              number.doubleValue >= 0, number.doubleValue <= Double(max) else { return nil }
        return number.intValue
    }
    private static func boolean(_ value: Any?) -> Bool {
        guard let number = value as? NSNumber else { return false }
        return CFGetTypeID(number) == CFBooleanGetTypeID()
    }
    private static func valid(_ o: [String: Any]) -> Bool {
        let common: Set<String> = ["version", "phase", "callId", "imageIndex"]
        guard integer(o["version"], max: 1) == 1, integer(o["callId"], max: 31) != nil,
              integer(o["imageIndex"], max: 31) != nil, let phase = o["phase"] as? String else { return false }
        let keys = Set(o.keys), cap = 64 * 1024 * 1024
        switch phase {
        case "call":
            guard keys == common.union(["bytes", "byteOffset", "bufferBytes", "blobBytes", "blobType", "disposed", "visibility", "hasFocus", "secureContext", "nativeViewportPath"]),
                  let bytes = integer(o["bytes"], max: cap), let offset = integer(o["byteOffset"], max: cap),
                  let buffer = integer(o["bufferBytes"], max: cap), let blob = integer(o["blobBytes"], max: cap),
                  bytes == blob, offset <= buffer, bytes <= buffer - offset,
                  o["blobType"] as? String == "image/png", boolean(o["disposed"]), boolean(o["hasFocus"]),
                  boolean(o["secureContext"]), boolean(o["nativeViewportPath"]),
                  let visibility = o["visibility"] as? String,
                  ["hidden", "visible", "prerender"].contains(visibility) else { return false }
            return true
        case "settled":
            guard let outcome = o["outcome"] as? String, boolean(o["disposed"]) else { return false }
            if outcome == "success" { return keys == common.union(["outcome", "disposed"]) }
            guard outcome == "decoder-error", keys == common.union(["outcome", "disposed", "errorName"]),
                  let name = o["errorName"] as? String else { return false }
            return ["Error", "TypeError", "RangeError", "EncodingError", "InvalidStateError", "NotSupportedError", "SecurityError", "AbortError", "UnknownError"].contains(name)
        case "hash":
            guard let outcome = o["outcome"] as? String else { return false }
            if outcome == "observation-failed" { return keys == common.union(["outcome"]) }
            guard outcome == "success", keys == common.union(["outcome", "sha256"]),
                  let digest = o["sha256"] as? String, digest.utf8.count == 64 else { return false }
            return digest.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
        default: return false
        }
    }
}
#endif
