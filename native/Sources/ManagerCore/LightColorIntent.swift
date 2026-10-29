// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum NativeLightColorMode: String, CaseIterable, Sendable {
    case direct, temperature
}

/// Explicit edit intent for the existing shared light-details contract.
/// Loading a stored K value never recomputes its stored linear RGB.
public enum LightColorIntent: Equatable, Sendable {
    case preserve, direct, temperature(Double)

    /// Matches studio-contract lightKelvinHex/lightKelvinLinear, including its
    /// 8-bit sRGB rounding. This is a visual approximation, not photometry.
    public static func linearRGB(kelvin: Double) throws -> [Double] {
        guard kelvin.isFinite, (1000...20000).contains(kelvin) else { throw AuthoringFailure("LIGHT_FIELDS_INVALID") }
        let t = kelvin / 100
        let channels = [t <= 66 ? 255 : 329.698727446 * pow(t - 60, -0.1332047592),
                        t <= 66 ? 99.4708025861 * log(t) - 161.1195681661 : 288.1221695283 * pow(t - 60, -0.0755148492),
                        t >= 66 ? 255 : t <= 19 ? 0 : 138.5177312231 * log(t - 10) - 305.0447927307]
        return channels.map {
            let c = floor(max(0, min(255, $0)) + 0.5) / 255
            return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
        }
    }
}
