// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum PlaqueCommand: Equatable, Sendable {
    case add(PlaqueSettings, roomIntroductionText: String? = nil), set(PlaqueSettings, roomIntroductionText: String? = nil), remove(id: String)
    public var plaqueId: String {
        switch self { case .add(let p, _), .set(let p, _): p.id; case .remove(let id): id }
    }
    private var isAdd: Bool { if case .add = self { return true }; return false }
    public var value: DocumentValue {
        switch self {
        case .add(let p, let introduction), .set(let p, let introduction):
            var fields: [String: DocumentValue] = ["type": .string(isAdd ? "add-plaque" : "set-plaque"), "plaque": p.value]
            if let introduction { fields["roomIntroductionText"] = .string(introduction) }
            return .object(fields)
        case .remove(let id): return .object(["type": .string("remove-plaque"), "plaqueId": .string(id)])
        }
    }
    /// Exact prospective delta only; shared full source/result validation is
    /// required before constructing the preview used by the native host.
    public func proposedCandidate(in draft: NativeExhibitionDraft, selectedPlaqueId: String?) throws -> DocumentValue {
        guard selectedPlaqueId == plaqueId, BenchSettings.uuid(plaqueId) else { throw AuthoringFailure("PLAQUE_SELECTION_CHANGED") }
        let stored = try PlaqueSettings.list(in: draft), index = stored.firstIndex { $0.id.lowercased() == plaqueId.lowercased() }
        guard isAdd ? index == nil : index != nil else { throw AuthoringFailure("PLAQUE_SELECTION_CHANGED") }
        if let index, stored[index].id != plaqueId { throw AuthoringFailure("PLAQUE_SELECTION_CHANGED") }
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        let raw = try BenchSettings.extensionValue(PlaqueSettings.namespace, in: draft)
        var fields: [String: DocumentValue] = ["version": .integer(1)]
        if case .object(let stored)? = raw { fields = stored }
        var rows = fields["plaques"]?.items ?? []
        switch self {
        case .add(let plaque, let introduction), .set(let plaque, let introduction):
            if let index { rows[index] = plaque.value } else { rows.append(plaque.value) }
            if let introduction {
                guard plaque.source == .room,
                      let roomId = draft.rooms.first(where: { $0["id"].string?.lowercased() == plaque.roomId.lowercased() })?["id"].string else { throw PlaqueSettings.invalid() }
                var sources = try PlaqueSettings.roomIntroductionRows(fields["roomIntroductions"], in: draft)
                var source: [String: DocumentValue] = [:]
                if let key = sources.keys.first(where: { $0.lowercased() == roomId.lowercased() }),
                   case .object(let existing)? = sources.removeValue(forKey: key) { source = existing }
                if introduction.isEmpty {
                    guard source["translations"]?.items?.isEmpty ?? true else { throw PlaqueSettings.invalid() }
                } else {
                    guard PlaqueSettings.validIntroductionText(introduction) else { throw PlaqueSettings.invalid() }
                    source["text"] = .string(introduction); sources[roomId] = .object(source)
                }
                fields["roomIntroductions"] = sources.isEmpty ? nil : .object(sources)
            }
        case .remove: if let index { rows.remove(at: index) }
        }
        fields["version"] = .integer(fields["roomIntroductions"] == nil ? 1 : 2)
        fields["plaques"] = .array(rows)
        candidate["extensions"][PlaqueSettings.namespace] = .object(fields)
        let template = "org.exhibitos.studio/template"
        if candidate["extensions"][template] != .null {
            guard case .object = candidate["extensions"][template], case .bool = candidate["extensions"][template]["modified"] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            candidate["extensions"][template]["modified"] = .bool(true)
        }
        var projection = draft; try projection.replaceCandidate(candidate)
        _ = try PlaqueSettings.list(in: projection)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
    public func checkProposal(_ candidate: DocumentValue, base: NativeExhibitionDraft, selectedPlaqueId: String?) throws {
        guard try candidate == proposedCandidate(in: base, selectedPlaqueId: selectedPlaqueId) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
    }
}

public struct PlaquePreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft, displayDraft: NativeExhibitionDraft
    public let command: PlaqueCommand
    public let candidate: DocumentValue
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: PlaqueCommand, selectedPlaqueId: String?) throws {
        try command.checkProposal(candidate, base: base, selectedPlaqueId: selectedPlaqueId)
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, selectedPlaqueId: String?, command: PlaqueCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        try command.checkProposal(candidate, base: draft, selectedPlaqueId: selectedPlaqueId)
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}
