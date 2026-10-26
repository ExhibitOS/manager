// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum CoverEntryTheme: String, CaseIterable, Sendable {
    case catalogue, poster, artwork, invitation, threshold
    public var title: String {
        switch self {
        case .catalogue: "전시 도록"
        case .poster: "전시 포스터"
        case .artwork: "작품 중심"
        case .invitation: "초대장"
        case .threshold: "공간의 문턱"
        }
    }
    public func effects(for stage: String) -> [String] {
        let value: String
        switch stage {
        case "cover": value = [.catalogue: "fold", .poster: "mask", .artwork: "fade", .invitation: "card", .threshold: "fade"][self]!
        case "response": value = [.catalogue: "tear", .poster: "press", .artwork: "press", .invitation: "confirm", .threshold: "press"][self]!
        default: value = [.catalogue: "doors", .poster: "fade", .artwork: "fade", .invitation: "panels", .threshold: "fade"][self]!
        }
        return ["none", value]
    }
}

/// Sparse authored JSON is retained exactly. Defaults belong to the shared
/// renderer; opening a form or switching modes never materializes them.
public struct CoverEntryDesign: Equatable, Sendable {
    public private(set) var raw: DocumentValue
    public var theme: CoverEntryTheme { CoverEntryTheme(rawValue: raw["theme"].string ?? "")! }
    public init(theme: CoverEntryTheme) { raw = .object(["version": .integer(1), "theme": .string(theme.rawValue)]) }
    public init(raw: DocumentValue) throws { self.raw = raw; try validate() }
    public func field(_ group: String?, _ name: String) -> DocumentValue {
        group.map { raw[$0][name] } ?? raw[name]
    }
    public mutating func set(_ group: String?, _ name: String, _ value: DocumentValue?) {
        guard case .object(var root) = raw else { return }
        if let group {
            var fields: [String: DocumentValue] = [:]
            if case .object(let saved)? = root[group] { fields = saved }
            fields[name] = value
            root[group] = fields.isEmpty ? nil : .object(fields)
        } else { root[name] = value }
        raw = .object(root)
    }
    public mutating func selectTheme(_ theme: CoverEntryTheme) { set(nil, "theme", .string(theme.rawValue)) }
    public mutating func resetLayoutAndEffects() { set(nil, "layout", nil); set(nil, "effects", nil) }
    public func validate() throws {
        func invalid() -> AuthoringFailure { AuthoringFailure("VIEWER_ENTRANCE_INVALID") }
        func object(_ value: DocumentValue, allowed: Set<String>) throws -> [String: DocumentValue] {
            guard case .object(let fields) = value, Set(fields.keys).isSubset(of: allowed) else { throw invalid() }
            return fields
        }
        func number(_ value: DocumentValue, _ range: ClosedRange<Double>) throws {
            guard let n = value.double, n.isFinite, range.contains(n) else { throw invalid() }
        }
        func choice(_ value: DocumentValue, _ choices: [String]) throws {
            guard let text = value.string, choices.contains(text) else { throw invalid() }
        }
        func boolean(_ value: DocumentValue) throws { guard case .bool = value else { throw invalid() } }
        let root = try object(raw, allowed: ["version", "theme", "image", "palette", "type", "ticket", "layout", "effects", "translations"])
        guard root["version"] == .integer(1), let name = root["theme"]?.string, let selected = CoverEntryTheme(rawValue: name) else { throw invalid() }
        if let value = root["palette"] { try choice(value, ["paper", "light", "dark"]) }
        if let value = root["type"] { try choice(value, ["system", "serif"]) }
        if let value = root["image"] {
            let fields = try object(value, allowed: ["fit", "focus"])
            if let fit = fields["fit"] { try choice(fit, ["contain", "cover"]) }
            if let focus = fields["focus"] {
                guard let coordinates = focus.items, coordinates.count == 2 else { throw invalid() }
                for coordinate in coordinates { try number(coordinate, 0...1) }
            }
        }
        if let value = root["ticket"] {
            for (_, flag) in try object(value, allowed: ["showDate", "showTime"]) { try boolean(flag) }
        }
        if let value = root["layout"] {
            for (key, n) in try object(value, allowed: ["copyRatio", "margin", "titleScale"]) {
                try number(n, key == "copyRatio" ? 0.30...0.70 : key == "margin" ? 0...64 : 0.75...1.5)
            }
            if selected == .catalogue, let ratio = value["copyRatio"].double, ratio != 0.5 { throw invalid() }
        }
        if let value = root["effects"] {
            let fields = try object(value, allowed: ["enabled", "cover", "response", "transition", "strength", "speed", "mobile"])
            if let enabled = fields["enabled"] { try boolean(enabled) }
            for stage in ["cover", "response", "transition"] {
                if let effect = fields[stage] { try choice(effect, selected.effects(for: stage)) }
            }
            if let strength = fields["strength"] { try number(strength, 0...1) }
            if let speed = fields["speed"] { try number(speed, 0.5...2) }
            if let mobile = fields["mobile"] { try choice(mobile, ["simple", "none"]) }
        }
        if let value = root["translations"] {
            guard let rows = value.items, rows.count <= 16 else { throw invalid() }
            var locales = Set<String>()
            for row in rows {
                let fields = try object(row, allowed: ["locale", "title", "shortTitle", "artist", "introduction"])
                guard let locale = fields["locale"]?.string, locale.utf16.count <= 35,
                      locale.range(of: "^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$", options: .regularExpression) != nil,
                      locales.insert(locale.lowercased()).inserted else { throw invalid() }
                for key in ["title", "shortTitle", "artist", "introduction"] {
                    if let text = fields[key] {
                        guard let string = text.string, string.utf16.count <= (key == "introduction" ? 4096 : key == "shortTitle" ? 160 : 512) else { throw invalid() }
                    }
                }
            }
        }
        let encoder = JSONEncoder(); encoder.outputFormatting = [.withoutEscapingSlashes]
        let json = try encoder.encode(raw)
        guard json.count <= 100 * 1024 else { throw invalid() }
    }
}

/// Numeric text is model-owned, including invalid intermediate input. Empty
/// text means the shared theme default, never an invented native substitute.
public struct CoverEntryFormBuffer: Equatable, Sendable {
    public enum NumberField: String, CaseIterable, Sendable {
        case focusX, focusY, copyRatio, margin, titleScale, strength, speed
        var group: String { switch self { case .focusX, .focusY: "image"; case .copyRatio, .margin, .titleScale: "layout"; case .strength, .speed: "effects" } }
    }
    public var design: CoverEntryDesign?
    public var numbers: [NumberField: String]
    public struct Translation: Equatable, Identifiable, Sendable {
        public let id: UUID
        public var fields: DocumentValue
        public init(fields: DocumentValue = .object(["locale": .string("")])) { id = UUID(); self.fields = fields }
    }
    public var translations: [Translation]
    private var initialTranslations: [Translation]
    private var initialNumbers: [NumberField: String]
    private var serializationNumbers: [NumberField: String]
    private var initialDesign: CoverEntryDesign?
    public init(design: CoverEntryDesign?) {
        self.design = design; initialDesign = design
        var fields: [NumberField: String] = [:]
        for field in NumberField.allCases {
            let value: Double?
            if field == .focusX || field == .focusY { value = design?.field("image", "focus").items?[field == .focusX ? 0 : 1].double }
            else { value = design?.field(field.group, field.rawValue).double }
            fields[field] = value.map { String($0) } ?? ""
        }
        numbers = fields; initialNumbers = fields; serializationNumbers = fields
        translations = (design?.raw["translations"].items ?? []).map { Translation(fields: $0) }
        initialTranslations = translations
    }
    public var isDirty: Bool { design != initialDesign || numbers != initialNumbers || translations != initialTranslations }
    public mutating func set(_ group: String?, _ name: String, _ value: DocumentValue?) {
        if design == nil { design = CoverEntryDesign(theme: .catalogue) }
        design?.set(group, name, value)
    }
    public mutating func selectTheme(_ theme: CoverEntryTheme) {
        if design == nil { design = CoverEntryDesign(theme: theme) } else { design?.selectTheme(theme) }
        resetLayoutAndEffects()
    }
    public mutating func resetLayoutAndEffects() {
        design?.resetLayoutAndEffects()
        for field in NumberField.allCases where field.group != "image" {
            numbers[field] = ""
            // Reset removed these authored fields. Subsequent text must be
            // compared with that empty state, even if it equals the old value.
            serializationNumbers[field] = ""
        }
    }
    public func value() throws -> CoverEntryDesign? {
        var result = design
        if translations != initialTranslations {
            if result == nil { result = CoverEntryDesign(theme: .catalogue) }
            result?.set(nil, "translations", .array(translations.map(\.fields)))
        }
        func parsed(_ field: NumberField) throws -> Double? {
            let text = numbers[field] ?? ""
            if text.isEmpty { return nil }
            guard let n = Double(text), n.isFinite, text == text.trimmingCharacters(in: .whitespacesAndNewlines) else { throw AuthoringFailure("VIEWER_ENTRANCE_INVALID") }
            return n
        }
        for field in NumberField.allCases where field != .focusX && field != .focusY && numbers[field] != serializationNumbers[field] {
            if result == nil { result = CoverEntryDesign(theme: .catalogue) }
            result?.set(field.group, field.rawValue, try parsed(field).map { .number($0) })
        }
        if numbers[.focusX] != initialNumbers[.focusX] || numbers[.focusY] != initialNumbers[.focusY] {
            let x = try parsed(.focusX), y = try parsed(.focusY)
            guard (x == nil) == (y == nil) else { throw AuthoringFailure("VIEWER_ENTRANCE_INVALID") }
            if result == nil { result = CoverEntryDesign(theme: .catalogue) }
            result?.set("image", "focus", x.map { .vector([$0, y!]) })
        }
        try result?.validate()
        return result
    }
}
