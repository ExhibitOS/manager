// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Native text adapter and strict reply correspondence only. Full document and
/// space-lighting admission belong to the shared Platform validator.
public struct SpaceLightingFormBuffer: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/space-lighting"
    public var usesOverride: Bool
    public var baselineIntensity: String
    public var reflectionIntensity: String
    public init(draft: NativeExhibitionDraft) throws {
        guard case .object(let extensions) = draft.candidate["extensions"] else {
            if draft.candidate["extensions"] == .null { usesOverride = false; baselineIntensity = ""; reflectionIntensity = ""; return }
            throw AuthoringFailure("SPACE_LIGHTING_INVALID")
        }
        guard let value = extensions[Self.namespace] else {
            usesOverride = false; baselineIntensity = ""; reflectionIntensity = ""; return
        }
        guard case .object(let fields) = value, Set(fields.keys) == Set(["version", "baselineIntensity", "reflectionIntensity"]), value["version"] == .integer(1),
              let baseline = value["baselineIntensity"].double, let reflection = value["reflectionIntensity"].double else { throw AuthoringFailure("SPACE_LIGHTING_INVALID") }
        usesOverride = true; baselineIntensity = String(baseline); reflectionIntensity = String(reflection)
    }
    public func command() throws -> SpaceLightingCommand {
        if !usesOverride { return SpaceLightingCommand(settings: .null) }
        func number(_ text: String) throws -> DocumentValue {
            guard let value = Double(text.trimmingCharacters(in: .whitespacesAndNewlines)), value.isFinite, (0...1).contains(value) else { throw AuthoringFailure("SPACE_LIGHTING_FIELDS_INVALID") }
            return .number(value)
        }
        return try SpaceLightingCommand(settings: .object(["version": .integer(1), "baselineIntensity": number(baselineIntensity), "reflectionIntensity": number(reflectionIntensity)]))
    }
}

public struct SpaceLightingCommand: Equatable, Sendable {
    public let settings: DocumentValue
    fileprivate init(settings: DocumentValue) { self.settings = settings }
    /// Predict the ONLY authorized delta, never an alternative admission path.
    public func proposedCandidate(in draft: NativeExhibitionDraft) throws -> DocumentValue {
        var candidate = draft.candidate
        let namespace = SpaceLightingFormBuffer.namespace
        var extensions: [String: DocumentValue]
        if case .object(let existing) = candidate["extensions"] { extensions = existing }
        else if candidate["extensions"] == .null { extensions = [:] }
        else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        if settings == .null {
            guard extensions.removeValue(forKey: namespace) != nil else { return candidate }
        } else {
            if extensions[namespace] == settings { return candidate }
            extensions[namespace] = settings
        }
        let template = "org.exhibitos.studio/template"
        if case .object(var fields) = extensions[template] { fields["modified"] = .bool(true); extensions[template] = .object(fields) }
        candidate["extensions"] = .object(extensions)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
}

public struct SpaceLightingPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft
    public let command: SpaceLightingCommand
    public let candidate: DocumentValue
    public let displayDraft: NativeExhibitionDraft
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: SpaceLightingCommand) throws {
        guard try candidate == command.proposedCandidate(in: base) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, command: SpaceLightingCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        guard try candidate == command.proposedCandidate(in: draft) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}
