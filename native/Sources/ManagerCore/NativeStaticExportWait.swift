// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Client wait only. The helper still owns export phase deadlines and settlement.
struct NativeStaticExportWait {
    private let deadline: ContinuousClock.Instant

    init(started: ContinuousClock.Instant, nativeOriginal: Bool) {
        // Existing detail helper: 600 seconds preparing + 120 seconds settling.
        // Ordinary exports retain their 120-second client budget.
        deadline = started.advanced(by: .seconds(nativeOriginal ? 720 : 120))
    }

    func allowsPolling(at instant: ContinuousClock.Instant) -> Bool {
        instant < deadline
    }
}
