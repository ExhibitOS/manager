// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Display-response mappings qualified by the shared D1 comparison. Selecting
/// one is an explicit edit, never a load default or a persisted preset name.
public enum NativeSpaceLightingPreset: String, CaseIterable, Sendable {
    case calm, bright
    fileprivate var baseline: String { self == .calm ? "0.15" : "0.2" }
    fileprivate var reflection: String { self == .calm ? "0.2" : "0.45" }
}

/// Native text adapter and strict reply correspondence only. Full document and
/// space-lighting admission belong to the shared Platform validator.
public struct SpaceLightingFormBuffer: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/space-lighting"
    private static let intensityKeys: Set<String> = ["version", "baselineIntensity", "reflectionIntensity"]
    private var roomLocal = false

    public static func canAddRoomLocal(to candidate: DocumentValue) -> Bool {
        if case .object(let extensions) = candidate["extensions"] { return extensions[namespace] == nil }
        return candidate["extensions"] == .null
    }

    static func addingRoomLocal(to candidate: DocumentValue) -> DocumentValue {
        guard canAddRoomLocal(to: candidate) else { return candidate }
        var next = candidate
        if next["extensions"] == .null { next["extensions"] = .object([:]) }
        next["extensions"][namespace] = .object([
            "version": .integer(1), "baselineIntensity": .number(0), "reflectionIntensity": .number(0),
            "mode": RoomCeilingFill.roomLocalMode
        ])
        return next
    }

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
        guard case .object(let fields) = value, value["version"] == .integer(1),
              let baseline = value["baselineIntensity"].double, let reflection = value["reflectionIntensity"].double,
              baseline.isFinite, reflection.isFinite, (0...1).contains(baseline), (0...1).contains(reflection)
        else { throw AuthoringFailure("SPACE_LIGHTING_INVALID") }
        let keys = Set(fields.keys)
        roomLocal = keys == Self.intensityKeys.union(["mode"]) && fields["mode"] == RoomCeilingFill.roomLocalMode
            && baseline == 0 && reflection == 0
        guard roomLocal || keys == Self.intensityKeys else { throw AuthoringFailure("SPACE_LIGHTING_INVALID") }
        usesOverride = true; baselineIntensity = String(baseline); reflectionIntensity = String(reflection)
    }
    public mutating func selectPreset(_ preset: NativeSpaceLightingPreset) {
        roomLocal = false; usesOverride = true; baselineIntensity = preset.baseline; reflectionIntensity = preset.reflection
    }
    public var preset: NativeSpaceLightingPreset? {
        guard usesOverride else { return nil }
        return NativeSpaceLightingPreset.allCases.first { Double(baselineIntensity) == Double($0.baseline) && Double(reflectionIntensity) == Double($0.reflection) }
    }
    public func command() throws -> SpaceLightingCommand {
        if !usesOverride { return SpaceLightingCommand(settings: .null) }
        func number(_ text: String) throws -> DocumentValue {
            guard let value = Double(text.trimmingCharacters(in: .whitespacesAndNewlines)), value.isFinite, (0...1).contains(value) else { throw AuthoringFailure("SPACE_LIGHTING_FIELDS_INVALID") }
            return .number(value)
        }
        let baseline = try number(baselineIntensity), reflection = try number(reflectionIntensity)
        var fields: [String: DocumentValue] = ["version": .integer(1), "baselineIntensity": baseline, "reflectionIntensity": reflection]
        if roomLocal, baseline.double == 0, reflection.double == 0 { fields["mode"] = RoomCeilingFill.roomLocalMode }
        return SpaceLightingCommand(settings: .object(fields))
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
