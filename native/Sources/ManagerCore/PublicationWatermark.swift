// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Native adapter for the shared Presentation v1 watermark field. It changes
/// publication choices only; artwork grants, required credits and source bytes
/// remain owned by their existing contracts.
public struct PublicationWatermark: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/presentation"
    public enum Mode: String, CaseIterable, Sendable { case none, overlay, embedded }
    public enum Position: String, CaseIterable, Sendable {
        case topLeft = "top-left", topRight = "top-right", bottomLeft = "bottom-left", bottomRight = "bottom-right"
    }
    public var mode: Mode
    public var text: String
    public var position: Position
    public var size: Int
    public var opacity: Double
    // ECMAScript WhiteSpace + LineTerminator, as used by the shared String.trim.
    // Foundation's general whitespace set differs (notably FEFF and U+0085).
    private static let sharedTrimCharacters = CharacterSet(charactersIn:
        "\u{0009}\u{000A}\u{000B}\u{000C}\u{000D}\u{0020}\u{00A0}\u{1680}" +
        "\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200A}" +
        "\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}")
    private static func trimmedText(_ text: String) -> String { text.trimmingCharacters(in: sharedTrimCharacters) }
    public init(mode: Mode = .none, text: String = "", position: Position = .bottomRight, size: Int = 12, opacity: Double = 0.25) {
        self.mode = mode; self.text = text; self.position = position; self.size = size; self.opacity = opacity
    }
    public func validate() throws {
        let label = Self.trimmedText(text)
        guard (8...24).contains(size), opacity.isFinite, (0.1...0.6).contains(opacity),
              text.isEmpty || !label.isEmpty,
              text.unicodeScalars.count <= (mode == .embedded ? 80 : 160),
              !text.unicodeScalars.contains(where: { $0.value <= 31 || (127...159).contains($0.value) }) else { throw Self.invalid() }
        if mode == .embedded, !Self.supportsEmbeddedText(label) { throw AuthoringFailure("WATERMARK_TEXT_UNSUPPORTED") }
    }
    public static func supportsEmbeddedText(_ text: String) -> Bool {
        let allowed = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .,:;!?\"'_/@()+-"
        return text.unicodeScalars.allSatisfy { allowed.unicodeScalars.contains($0) }
    }
    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value,
              Set(["version", "mode"]).isSubset(of: Set(fields.keys)),
              Set(fields.keys).isSubset(of: Set(["version", "mode", "text", "position", "size", "opacity"])),
              value["version"] == .integer(1), let raw = value["mode"].string, let mode = Mode(rawValue: raw),
              fields["text"] == nil || fields["text"]?.string != nil,
              fields["position"] == nil || fields["position"]?.string.flatMap(Position.init(rawValue:)) != nil,
              fields["size"] == nil || fields["size"]?.double.flatMap(Int.init(exactly:)) != nil,
              fields["opacity"] == nil || fields["opacity"]?.double != nil else { throw Self.invalid() }
        self.init(mode: mode, text: value["text"].string ?? "", position: value["position"].string.flatMap(Position.init(rawValue:)) ?? .bottomRight,
                  size: value["size"].double.flatMap(Int.init(exactly:)) ?? 12, opacity: value["opacity"].double ?? 0.25)
        if fields["text"] != nil, Self.trimmedText(text).isEmpty { throw Self.invalid() }
        try validate()
    }
    public var value: DocumentValue {
        var fields: [String: DocumentValue] = ["version": .integer(1), "mode": .string(mode.rawValue)]
        if mode != .none {
            if !text.isEmpty { fields["text"] = .string(Self.trimmedText(text)) }
            fields["position"] = .string(position.rawValue); fields["size"] = .integer(Int64(size)); fields["opacity"] = .number(opacity)
        }
        return .object(fields)
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("WATERMARK_FIELDS_INVALID") }
    private static func presentation(in draft: NativeExhibitionDraft) throws -> [String: DocumentValue]? {
        guard case .object(let candidate) = draft.candidate else { throw invalid() }
        guard let raw = candidate["extensions"] else { return nil }
        guard case .object(let extensions) = raw else { throw invalid() }
        guard let rawPresentation = extensions[namespace] else { return nil }
        guard case .object(let presentation) = rawPresentation, rawPresentation["version"] == .integer(1) else { throw invalid() }
        return presentation
    }
    /// Absence is distinct from explicit NONE. Reads never migrate old drafts.
    public static func read(from draft: NativeExhibitionDraft) throws -> Self? {
        guard let value = try presentation(in: draft)?["watermark"] else { return nil }
        return try Self(value: value)
    }
    public func proposedCandidate(in draft: NativeExhibitionDraft) throws -> DocumentValue {
        try validate()
        var presentation = try Self.presentation(in: draft) ?? ["version": .integer(1), "viewpoints": .array([]), "credits": .string("")]
        if let old = presentation["watermark"] { _ = try Self(value: old) } // Never default-rewrite an unknown schema.
        var candidate = draft.candidate
        var extensions: [String: DocumentValue] = [:]
        if case .object(let old) = candidate["extensions"] { extensions = old }
        presentation["watermark"] = value
        extensions[Self.namespace] = .object(presentation); candidate["extensions"] = .object(extensions)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
}

public struct PublicationWatermarkFormBuffer: Equatable, Sendable {
    public var mode: PublicationWatermark.Mode
    public var text: String
    public var position: PublicationWatermark.Position
    public var size: String
    public var opacity: String
    private let original: PublicationWatermark
    public init(settings: PublicationWatermark? = nil) {
        let settings = settings ?? PublicationWatermark()
        original = settings; mode = settings.mode; text = settings.text; position = settings.position
        size = String(settings.size); opacity = String(settings.opacity)
    }
    public var isDirty: Bool {
        mode != original.mode || (mode != .none && (text != original.text || position != original.position || size != String(original.size) || opacity != String(original.opacity)))
    }
    public func settings() throws -> PublicationWatermark {
        if mode == .none { return PublicationWatermark() }
        guard let size = Int(size.trimmingCharacters(in: .whitespacesAndNewlines)),
              let opacity = Double(opacity.trimmingCharacters(in: .whitespacesAndNewlines)) else { throw PublicationWatermark.invalid() }
        let result = PublicationWatermark(mode: mode, text: text, position: position, size: size, opacity: opacity)
        try result.validate(); return result
    }
}

/// Exact-manifest adoption of the qualified shared watermark packet. Schema21
/// alone does not imply this feature or publication derivation policy.
public enum PublicationWatermarkRuntime {
    public static let qualifiedManifestSHA256: String? = "6473b883ca56779bffc5f5cddbab1aae62c56c10b3be47a948b461f3a45387c5"
    public static func supports(manifestSHA256: String?, ready: Bool) -> Bool {
        guard ready, let qualifiedManifestSHA256 else { return false }
        return manifestSHA256 == qualifiedManifestSHA256
    }
}

extension NativeExhibitionDraft {
    public func publicationWatermark() throws -> PublicationWatermark? { try PublicationWatermark.read(from: self) }
    public mutating func setPublicationWatermark(_ settings: PublicationWatermark) throws {
        let candidate = try settings.proposedCandidate(in: self)
        if candidate != self.candidate { try replaceCandidate(candidate) }
    }
}
