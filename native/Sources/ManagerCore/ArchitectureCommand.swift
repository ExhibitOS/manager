// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Numeric edits for one existing architectural selection. Platform remains
/// responsible for geometry validation; this adapter preserves undisplayed data.
public enum ArchitectureCommand: Equatable, Sendable {
    case room(id: String, name: String, width: Double, height: Double, depth: Double, position: [Double])
    case surface(id: String, width: Double, height: Double, position: [Double])
    case opening(id: String, width: Double, height: Double, offset: [Double])

    public var selection: ViewportSelection {
        switch self {
        case .room(let id, _, _, _, _, _): ViewportSelection(kind: "room", id: id)
        case .surface(let id, _, _, _): ViewportSelection(kind: "surface", id: id)
        case .opening(let id, _, _, _): ViewportSelection(kind: "opening", id: id)
        }
    }
    private var collection: String {
        switch self { case .room: "rooms"; case .surface: "surfaces"; case .opening: "openings" }
    }
    private func replacement(in draft: NativeExhibitionDraft, selected: ViewportSelection?) throws -> (Int, DocumentValue) {
        guard selected == selection, selection.belongs(to: draft),
              let index = draft.candidate[collection].items?.firstIndex(where: { $0["id"].string == selection.id }),
              let original = draft.candidate[collection].items?[index], case .object = original else {
            throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED")
        }
        var row = original
        let width: Double, height: Double
        switch self {
        case .room(_, let name, let w, let h, let depth, let position):
            guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, name.count <= 512,
                  Self.dimension(depth), Self.coordinates(position, count: 3), case .object = row["transform"] else { throw Self.invalid() }
            width = w; height = h
            row["name"] = .string(name)
            row["dimensions"]["depth"] = .number(depth)
            row["transform"]["position"] = .vector(position)
        case .surface(_, let w, let h, let position):
            guard Self.coordinates(position, count: 3), case .object = row["transform"] else { throw Self.invalid() }
            width = w; height = h
            row["transform"]["position"] = .vector(position)
        case .opening(_, let w, let h, let offset):
            guard Self.coordinates(offset, count: 2) else { throw Self.invalid() }
            width = w; height = h
            row["offset"] = .vector(offset)
        }
        guard Self.dimension(width), Self.dimension(height), case .object = row["dimensions"] else { throw Self.invalid() }
        row["dimensions"]["width"] = .number(width)
        row["dimensions"]["height"] = .number(height)
        return (index, row)
    }
    private static func dimension(_ value: Double) -> Bool { value.isFinite && value > 0 && value <= 1_000_000 }
    private static func coordinates(_ values: [Double], count: Int) -> Bool { values.count == count && values.allSatisfy { $0.isFinite && abs($0) <= 1_000_000 } }
    private static func invalid() -> AuthoringFailure { AuthoringFailure("ARCHITECTURE_FIELDS_INVALID") }

    public func encodedValue(in draft: NativeExhibitionDraft, selected: ViewportSelection?) throws -> DocumentValue {
        let (_, row) = try replacement(in: draft, selected: selected)
        guard case .object(var fields) = row else { throw Self.invalid() }
        fields.removeValue(forKey: "id")
        return .object(["type": .string("set-" + selection.kind), "id": .string(selection.id), selection.kind: .object(fields)])
    }
    /// Correspondence check only, after the shared validator has returned a
    /// proposal. Reject unrelated changes, dropped fields and child rescaling.
    public func checkProposal(_ candidate: DocumentValue, base: NativeExhibitionDraft, selected: ViewportSelection?) throws {
        let (index, row) = try replacement(in: base, selected: selected)
        var expected = base.candidate
        var rows = expected[collection].items!
        rows[index] = row; expected[collection] = .array(rows)
        // Existing Platform applyGeometryCommand marks this exact provenance
        // flag after every command. No other extension change is permitted.
        let template = "org.exhibitos.studio/template"
        if expected["extensions"][template] != .null {
            guard case .object = expected["extensions"][template],
                  case .bool = expected["extensions"][template]["modified"] else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
            expected["extensions"][template]["modified"] = .bool(true)
        }
        guard expected == candidate else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
    }
}

extension NativeExhibitionDraft {
    public mutating func acceptArchitectureGeometry(_ candidate: DocumentValue, command: ArchitectureCommand, selected: ViewportSelection?) throws {
        try command.checkProposal(candidate, base: self, selected: selected)
        // The correspondence check includes identity, all undisplayed data and
        // only the documented template flag; generic geometry permissions stay.
        try replaceCandidate(candidate)
    }
}

/// Keep invalid/incomplete text until Apply or Cancel; it never mutates a draft.
public struct ArchitectureFormBuffer: Equatable, Sendable {
    public let selection: ViewportSelection
    public var name = ""
    public var width = "", height = "", depth = ""
    public var x = "", y = "", z = ""
    public init(draft: NativeExhibitionDraft, selection: ViewportSelection) throws {
        let keys = ["room": "rooms", "surface": "surfaces", "opening": "openings"]
        guard let key = keys[selection.kind], selection.belongs(to: draft),
              let row = draft.candidate[key].items?.first(where: { $0["id"].string == selection.id }) else { throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED") }
        self.selection = selection
        func text(_ value: DocumentValue) throws -> String {
            guard let n = value.double, n.isFinite else { throw AuthoringFailure("ARCHITECTURE_FIELDS_INVALID") }
            return String(n)
        }
        width = try text(row["dimensions"]["width"]); height = try text(row["dimensions"]["height"])
        if selection.kind == "room" { name = row["name"].string ?? ""; depth = try text(row["dimensions"]["depth"]) }
        let values = selection.kind == "opening" ? row["offset"].items : row["transform"]["position"].items
        guard let values, values.count == (selection.kind == "opening" ? 2 : 3) else { throw AuthoringFailure("ARCHITECTURE_FIELDS_INVALID") }
        x = try text(values[0]); y = try text(values[1])
        if values.count == 3 { z = try text(values[2]) }
    }
    public func command() throws -> ArchitectureCommand {
        func number(_ text: String) throws -> Double {
            guard let n = Double(text.trimmingCharacters(in: .whitespacesAndNewlines)), n.isFinite else { throw AuthoringFailure("ARCHITECTURE_FIELDS_INVALID") }
            return n
        }
        let w = try number(width), h = try number(height), px = try number(x), py = try number(y)
        switch selection.kind {
        case "room": return .room(id: selection.id, name: name, width: w, height: h, depth: try number(depth), position: [px, py, try number(z)])
        case "surface": return .surface(id: selection.id, width: w, height: h, position: [px, py, try number(z)])
        case "opening": return .opening(id: selection.id, width: w, height: h, offset: [px, py])
        default: throw AuthoringFailure("ARCHITECTURE_SELECTION_CHANGED")
        }
    }
}
