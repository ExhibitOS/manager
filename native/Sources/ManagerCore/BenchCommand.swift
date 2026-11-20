// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Typed consumer of shared add/set/remove-bench. Only a negotiated auxiliary
/// bridge may validate it; ordinary v1 geometry/selection permissions stay closed.
public enum BenchCommand: Equatable, Sendable {
    case add(BenchSettings, viewpoint: BenchViewpoint?)
    case set(BenchSettings, viewpoint: BenchViewpoint?)
    case remove(id: String)
    case setViewpointTargetHeight(benchId: String, viewpointId: String, targetY: Double)
    public var benchId: String {
        switch self {
        case .add(let bench, _), .set(let bench, _): bench.id
        case .remove(let id), .setViewpointTargetHeight(let id, _, _): id
        }
    }
    public var requiresViewpointTargetHeight: Bool {
        if case .setViewpointTargetHeight = self { return true }
        return false
    }
    private var bench: BenchSettings? {
        switch self { case .add(let value, _), .set(let value, _): value; case .remove, .setViewpointTargetHeight: nil }
    }
    private var viewpoint: BenchViewpoint? {
        switch self { case .add(_, let value), .set(_, let value): value; case .remove, .setViewpointTargetHeight: nil }
    }
    public var value: DocumentValue {
        switch self {
        case .add(let bench, let viewpoint), .set(let bench, let viewpoint):
            var fields: [String: DocumentValue] = ["type": .string(isAdd ? "add-bench" : "set-bench"), "bench": bench.value]
            if let viewpoint { fields["viewpoint"] = viewpoint.value }
            return .object(fields)
        case .remove(let id): return .object(["type": .string("remove-bench"), "benchId": .string(id)])
        case .setViewpointTargetHeight(let benchId, let viewpointId, let targetY):
            return .object(["type": .string("set-bench-viewpoint-target-height"),
                            "benchId": .string(benchId), "viewpointId": .string(viewpointId), "targetY": .number(targetY)])
        }
    }
    private var isAdd: Bool { if case .add = self { return true }; return false }
    /// Expected narrow delta only. Full shared validators remain authoritative
    /// for bench bounds, affine geometry, view footprint and runtime clearance.
    public func proposedCandidate(in draft: NativeExhibitionDraft, selectedBenchId: String?) throws -> DocumentValue {
        guard BenchSettings.uuid(benchId),
              requiresViewpointTargetHeight ? selectedBenchId?.lowercased() == benchId.lowercased() : selectedBenchId == benchId
        else { throw AuthoringFailure("BENCH_SELECTION_CHANGED") }
        let benches = try BenchSettings.list(in: draft)
        if case .setViewpointTargetHeight(_, let viewpointId, let targetY) = self {
            return try targetHeightCandidate(in: draft, benches: benches, viewpointId: viewpointId, targetY: targetY)
        }
        let existing = benches.firstIndex { $0.id.lowercased() == benchId.lowercased() }
        guard isAdd ? existing == nil : existing != nil else { throw AuthoringFailure("BENCH_SELECTION_CHANGED") }
        if let existing { guard benches[existing].id == benchId else { throw AuthoringFailure("BENCH_SELECTION_CHANGED") } }
        var candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        // Keep every unselected record verbatim, including numeric encodings.
        var rows = (try BenchSettings.extensionValue(BenchSettings.namespace, in: draft))?["benches"].items ?? []
        if let bench {
            guard draft.rooms.contains(where: { $0["id"].string?.lowercased() == bench.roomId.lowercased() }) else { throw BenchSettings.invalid() }
            if let viewpoint {
                let current = try BenchViewpoint.list(in: draft)
                guard viewpoint.id.lowercased() == bench.viewpointId.lowercased(), viewpoint.roomId.lowercased() == bench.roomId.lowercased(),
                      !current.contains(where: { $0.id.lowercased() == viewpoint.id.lowercased() }), current.count < 64 else { throw BenchSettings.invalid() }
                var presentation = try BenchSettings.extensionValue(BenchSettings.presentationNamespace, in: draft) ?? .object(["version": .integer(1), "viewpoints": .array([]), "credits": .string("")])
                guard var views = presentation["viewpoints"].items else { throw BenchSettings.invalid() }
                views.append(viewpoint.value)
                presentation["viewpoints"] = .array(views)
                guard try JSONEncoder().encode(presentation).count <= 32_768 else { throw BenchSettings.invalid() }
                candidate["extensions"][BenchSettings.presentationNamespace] = presentation
            }
            if let existing { rows[existing] = bench.value } else { rows.append(bench.value) }
        } else if let existing { rows.remove(at: existing) }
        candidate["extensions"][BenchSettings.namespace] = .object(["version": .integer(1), "benches": .array(rows)])
        var projection = draft; try projection.replaceCandidate(candidate)
        _ = try BenchSettings.list(in: projection)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
    private func targetHeightCandidate(in draft: NativeExhibitionDraft, benches: [BenchSettings],
                                       viewpointId: String, targetY: Double) throws -> DocumentValue {
        let linkedId = viewpointId.lowercased()
        guard BenchSettings.uuid(viewpointId), targetY.isFinite,
              let bench = benches.first(where: { $0.id.lowercased() == benchId.lowercased() }),
              bench.viewpointId.lowercased() == linkedId,
              benches.filter({ $0.viewpointId.lowercased() == linkedId }).count == 1,
              let room = draft.rooms.first(where: { $0["id"].string?.lowercased() == bench.roomId.lowercased() }),
              let ceiling = room["dimensions"]["height"].double, targetY >= 0, targetY <= ceiling,
              let viewpoint = try BenchViewpoint.list(in: draft).first(where: { $0.id.lowercased() == linkedId }),
              viewpoint.roomId.lowercased() == bench.roomId.lowercased(),
              var presentation = try BenchSettings.extensionValue(BenchSettings.presentationNamespace, in: draft),
              var rows = presentation["viewpoints"].items,
              let index = rows.firstIndex(where: { $0["id"].string == viewpoint.id }),
              var target = rows[index]["target"].items else { throw BenchSettings.invalid() }
        if target[1].double == targetY { return draft.candidate }
        target[1] = .number(targetY)
        rows[index]["target"] = .array(target)
        presentation["viewpoints"] = .array(rows)
        guard try JSONEncoder().encode(presentation).count <= 32_768 else { throw BenchSettings.invalid() }
        var candidate = draft.candidate
        candidate["extensions"][BenchSettings.presentationNamespace] = presentation
        var projection = draft
        try projection.replaceCandidate(candidate)
        _ = try BenchSettings.list(in: projection)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
    public func checkProposal(_ candidate: DocumentValue, base: NativeExhibitionDraft, selectedBenchId: String?) throws {
        guard try candidate == proposedCandidate(in: base, selectedBenchId: selectedBenchId) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
    }
}

public struct BenchPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft
    public let command: BenchCommand
    public let candidate: DocumentValue
    public let displayDraft: NativeExhibitionDraft
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: BenchCommand, selectedBenchId: String?) throws {
        try command.checkProposal(candidate, base: base, selectedBenchId: selectedBenchId)
        self.base = base; self.command = command; self.candidate = candidate
        var display = base; try display.replaceCandidate(candidate); displayDraft = display
    }
    public func apply(to draft: NativeExhibitionDraft, selectedBenchId: String?, command: BenchCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        try command.checkProposal(candidate, base: draft, selectedBenchId: selectedBenchId)
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}
