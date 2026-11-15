// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore

/// A read-only current-frame sample, never a camera request or edit authority.
enum NativeCameraPoseReadout {
    static let unavailable: DocumentValue = .object(["status": .string("unavailable")])

    static func projected(_ raw: String?, epoch: String, sequence: Int) -> DocumentValue {
        guard let raw, raw.utf8.count <= 4096,
              UUID(uuidString: epoch)?.uuidString.lowercased() == epoch,
              sequence > 0, sequence <= 9_007_199_254_740_991,
              let data = raw.data(using: .utf8),
              case .object(let fields) = try? JSONDecoder().decode(DocumentValue.self, from: data),
              Set(fields.keys) == Set(["version", "frameToken", "position", "target"]),
              fields["version"]?.double == 1,
              fields["frameToken"]?.string == "\(epoch):\(sequence)",
              let position = vector(fields["position"]), let target = vector(fields["target"]) else {
            return unavailable
        }
        let distanceSquared = zip(position, target).reduce(0.0) { total, pair in
            let delta = pair.0 - pair.1
            return total + delta * delta
        }
        guard distanceSquared > 1e-12 else { return unavailable }
        return .object(["status": .string("current"),
                        "position": .array(position.map(DocumentValue.number)),
                        "target": .array(target.map(DocumentValue.number))])
    }

    private static func vector(_ value: DocumentValue?) -> [Double]? {
        guard let values = value?.items, values.count == 3 else { return nil }
        let numbers = values.compactMap(\.double)
        guard numbers.count == 3, numbers.allSatisfy({ $0.isFinite && abs($0) <= 10_000 }) else { return nil }
        return numbers
    }
}
