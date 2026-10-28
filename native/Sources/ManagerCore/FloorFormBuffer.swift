// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Partial text stays in this buffer until explicit Apply. Neither selection nor
/// parsing starts audio or changes the persisted document.
public struct FloorFormBuffer: Equatable, Sendable {
    public enum Numeric: String, CaseIterable, Sendable { case roughness, metalness, repeatU, repeatV, rotation, normalScale, aoStrength, gain }
    public private(set) var settings: FloorSettings?
    public var numbers: [Numeric: String]
    public var color: String
    /// nil means keep the exact authored/legacy sound; not a fourth wire mode.
    public var soundMode: String?
    public var assetIds: [String]
    public var restoreLegacy = false
    private var baselineNumbers: [Numeric: String]
    private var baselineColor: String
    private var baselineSoundMode: String?
    private var baselineAssetIds: [String]
    public init(settings: FloorSettings?) {
        self.settings = settings
        let v = settings?.value ?? .null
        func text(_ value: DocumentValue) -> String { value.double.map { String($0) } ?? "" }
        let repeated = v["repeatMeters"].items ?? []
        numbers = [.roughness: text(v["materialOverride"]["roughness"]), .metalness: text(v["materialOverride"]["metalness"]),
                   .repeatU: repeated.count == 2 ? text(repeated[0]) : "", .repeatV: repeated.count == 2 ? text(repeated[1]) : "",
                   .rotation: text(v["rotation"]), .normalScale: text(v["normalScale"]), .aoStrength: text(v["aoStrength"]), .gain: text(v["sound"]["gain"])]
        color = v["materialOverride"]["color"].string ?? ""
        soundMode = v["sound"]["mode"].string; assetIds = v["sound"]["assetIds"].items?.compactMap(\.string) ?? []
        baselineNumbers = numbers; baselineColor = color; baselineSoundMode = soundMode; baselineAssetIds = assetIds
    }
    private static func numericFields(_ field: FloorSettings.Field) -> [Numeric] {
        switch field {
        case .roughness: [.roughness]; case .metalness: [.metalness]
        case .repeatMeters: [.repeatU, .repeatV]; case .rotation: [.rotation]
        case .normalScale: [.normalScale]; case .aoStrength: [.aoStrength]; case .sound: [.gain]
        default: []
        }
    }
    public mutating func stage(_ settings: FloorSettings, replacing fields: Set<FloorSettings.Field>, candidate: DocumentValue) throws {
        let old = self
        // The new settings already contain the currently edited values for
        // unselected fields. Refuse invalid text before rebasing comparisons.
        var pending = old; pending.restoreLegacy = false
        if pending.settings != nil { _ = try pending.value(candidate: candidate) }
        self = Self(settings: settings)
        let selectedNumbers = Set(fields.flatMap(Self.numericFields))
        for key in Numeric.allCases where !selectedNumbers.contains(key) {
            numbers[key] = old.numbers[key]
        }
        if !fields.contains(.color) { color = old.color }
        if !fields.contains(.sound) {
            soundMode = old.soundMode; assetIds = old.assetIds
        }
        // Compare the next edit with the exact visible text at this staging
        // boundary, rather than the older persisted value or normalized text.
        baselineNumbers = numbers; baselineColor = color
        baselineSoundMode = soundMode; baselineAssetIds = assetIds
    }
    public mutating func selectSoundMode(_ mode: String?) {
        if let mode {
            guard ["material-default", "explicit", "none"].contains(mode) else { return }
            soundMode = mode
            if (numbers[.gain] ?? "").isEmpty { numbers[.gain] = "1" }
        } else {
            // This choice exists only for a binding with absent sound. Discard
            // the temporary sound controls, preserving every appearance input.
            guard settings?.value["sound"] == .null else { return }
            soundMode = nil; assetIds = []; numbers[.gain] = ""
            baselineSoundMode = nil; baselineAssetIds = []; baselineNumbers[.gain] = ""
        }
    }
    /// Reset just the requested field to the persisted base preset, including
    /// an invalid intermediate value. Other raw input is never parsed or rebased.
    public mutating func reset(_ field: FloorSettings.Field, to persistedPreset: FloorSettings.Preset, candidate: DocumentValue) throws {
        guard let settings, settings.preset == persistedPreset, !restoreLegacy,
              let value = FloorSettings.defaults(persistedPreset)[field] else { throw AuthoringFailure("FLOOR_SETTINGS_INVALID") }
        let replaced = try settings.replacing(field, with: value, candidate: candidate)
        let fresh = Self(settings: replaced)
        self.settings = replaced
        for key in Self.numericFields(field) {
            numbers[key] = fresh.numbers[key]; baselineNumbers[key] = fresh.numbers[key]
        }
        if field == .color { color = fresh.color; baselineColor = fresh.color }
        if field == .sound {
            soundMode = fresh.soundMode; baselineSoundMode = fresh.soundMode
            assetIds = fresh.assetIds; baselineAssetIds = fresh.assetIds
        }
    }
    public func value(candidate: DocumentValue) throws -> FloorSettings? {
        if restoreLegacy { return nil }
        guard var result = settings else { throw AuthoringFailure("FLOOR_PRESET_REQUIRED") }
        func parsed(_ key: Numeric) throws -> Double {
            let raw = (numbers[key] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                .replacingOccurrences(of: Locale.current.decimalSeparator ?? ".", with: ".")
            guard let number = Double(raw), number.isFinite else { throw AuthoringFailure("FLOOR_SETTINGS_INVALID") }; return number
        }
        if color != baselineColor { result = try result.replacing(.color, with: .string(color), candidate: candidate) }
        for (key, field) in [(Numeric.roughness, FloorSettings.Field.roughness), (.metalness, .metalness), (.rotation, .rotation), (.normalScale, .normalScale), (.aoStrength, .aoStrength)] where numbers[key] != baselineNumbers[key] {
            result = try result.replacing(field, with: .number(parsed(key)), candidate: candidate)
        }
        if numbers[.repeatU] != baselineNumbers[.repeatU] || numbers[.repeatV] != baselineNumbers[.repeatV] {
            result = try result.replacing(.repeatMeters, with: .vector([parsed(.repeatU), parsed(.repeatV)]), candidate: candidate)
        }
        if soundMode != baselineSoundMode || numbers[.gain] != baselineNumbers[.gain] || assetIds != baselineAssetIds {
            guard let soundMode else { throw AuthoringFailure("FLOOR_SETTINGS_INVALID") }
            var sound: [String: DocumentValue] = ["mode": .string(soundMode), "gain": .number(try parsed(.gain))]
            if soundMode == "explicit" { sound["assetIds"] = .array(assetIds.map(DocumentValue.string)) }
            result = try result.replacing(.sound, with: .object(sound), candidate: candidate)
        }
        return result
    }
}
