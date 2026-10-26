// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum PlaqueCommand: Equatable, Sendable {
    case add(PlaqueSettings), set(PlaqueSettings), remove(id: String)
    public var plaqueId: String {
        switch self { case .add(let p), .set(let p): p.id; case .remove(let id): id }
    }
    private var isAdd: Bool { if case .add = self { return true }; return false }
    public var value: DocumentValue {
        switch self {
        case .add(let p), .set(let p): .object(["type": .string(isAdd ? "add-plaque" : "set-plaque"), "plaque": p.value])
        case .remove(let id): .object(["type": .string("remove-plaque"), "plaqueId": .string(id)])
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
        var rows = (try BenchSettings.extensionValue(PlaqueSettings.namespace, in: draft))?["plaques"].items ?? []
        switch self {
        case .add(let plaque), .set(let plaque):
            if let index { rows[index] = plaque.value } else { rows.append(plaque.value) }
        case .remove: if let index { rows.remove(at: index) }
        }
        candidate["extensions"][PlaqueSettings.namespace] = .object(["version": .integer(1), "plaques": .array(rows)])
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
