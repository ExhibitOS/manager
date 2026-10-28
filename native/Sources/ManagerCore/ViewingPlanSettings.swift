// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Native adapter for the shared optional viewing-plan v1. These are metadata
/// checks; safe visitor camera positions remain the shared runtime's authority.
public struct ViewingPlanSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.viewer/viewing-plan"
    public let value: DocumentValue
    public var stops: [DocumentValue] { value["stops"].items ?? [] }
    public init(value: DocumentValue, in draft: NativeExhibitionDraft) throws {
        guard case .object(let fields) = value, Set(fields.keys) == Set(["version", "stops"]),
              value["version"] == .integer(1), let stops = value["stops"].items, stops.count <= 128,
              try JSONEncoder().encode(value).count <= 32_768 else { throw Self.invalid() }
        let views = try BenchViewpoint.list(in: draft)
        var seen = Set<String>()
        for stop in stops {
            guard case .object(let fields) = stop,
                  Set(fields.keys) == Set(fields["orbit"] == nil ? ["placementId", "viewpointId"] : ["placementId", "viewpointId", "orbit"]),
                  let placementID = stop["placementId"].string, UUID(uuidString: placementID) != nil,
                  let viewpointID = stop["viewpointId"].string, UUID(uuidString: viewpointID) != nil,
                  seen.insert(placementID.lowercased()).inserted,
                  let placement = draft.placements.first(where: { $0["id"].string?.lowercased() == placementID.lowercased() }),
                  views.contains(where: { $0.id.lowercased() == viewpointID.lowercased() }) else { throw Self.invalid() }
            if let orbit = fields["orbit"] {
                guard case .object(let bounds) = orbit, Set(bounds.keys) == Set(["minYaw", "maxYaw"]),
                      let min = orbit["minYaw"].double, let max = orbit["maxYaw"].double,
                      min.isFinite, max.isFinite, -Double.pi <= min, min <= 0, 0 <= max, max <= Double.pi, min < max,
                      let artwork = draft.artworks.first(where: { $0["revisionId"].string?.lowercased() == placement["artworkRevisionId"].string?.lowercased() }),
                      artwork["artworkType"].string == "sculpture" else { throw Self.invalid() }
            }
        }
        self.value = value
    }
    public static func read(in draft: NativeExhibitionDraft) throws -> Self? {
        guard case .object(let candidate) = draft.candidate, let raw = candidate["extensions"] else { return nil }
        guard case .object(let extensions) = raw else { throw invalid() }
        guard let value = extensions[namespace] else { return nil }
        return try Self(value: value, in: draft)
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("VIEWING_PLAN_INVALID") }
}

/// Raw text belongs to the exhibition model, not the inspector's lifetime or
/// mode. Unchanged orbit values retain their original radians without rounding.
public struct ViewingPlanFormBuffer: Equatable, Sendable {
    public struct Stop: Equatable, Sendable, Identifiable {
        public var id: String { placementId }
        public let placementId: String
        public var viewpointId: String
        public var usesOrbit: Bool
        public var minimumDegrees: String
        public var maximumDegrees: String
        private let originalOrbit: DocumentValue?
        private let originalMinimum: String
        private let originalMaximum: String
        public init(placementId: String, viewpointId: String = "") {
            self.placementId = placementId; self.viewpointId = viewpointId
            usesOrbit = false; minimumDegrees = "-45"; maximumDegrees = "45"
            originalOrbit = nil; originalMinimum = ""; originalMaximum = ""
        }
        fileprivate init(_ value: DocumentValue) {
            placementId = value["placementId"].string!; viewpointId = value["viewpointId"].string!
            let orbit = value["orbit"]
            usesOrbit = orbit != .null
            minimumDegrees = orbit["minYaw"].double.map { String($0 * 180 / .pi) } ?? "-45"
            maximumDegrees = orbit["maxYaw"].double.map { String($0 * 180 / .pi) } ?? "45"
            originalMinimum = minimumDegrees; originalMaximum = maximumDegrees
            originalOrbit = usesOrbit ? orbit : nil
        }
        fileprivate func value(in draft: NativeExhibitionDraft) throws -> DocumentValue {
            guard let placement = draft.placements.first(where: { $0["id"].string?.lowercased() == placementId.lowercased() }),
                  let canonicalID = placement["id"].string,
                  let view = try BenchViewpoint.list(in: draft).first(where: { $0.id.lowercased() == viewpointId.lowercased() }) else { throw ViewingPlanSettings.invalid() }
            var fields: [String: DocumentValue] = ["placementId": .string(canonicalID), "viewpointId": .string(view.id)]
            if usesOrbit {
                if let originalOrbit, minimumDegrees == originalMinimum, maximumDegrees == originalMaximum {
                    fields["orbit"] = originalOrbit
                } else {
                    guard let min = Double(minimumDegrees.trimmingCharacters(in: .whitespacesAndNewlines)),
                          let max = Double(maximumDegrees.trimmingCharacters(in: .whitespacesAndNewlines)),
                          min.isFinite, max.isFinite, (-180...0).contains(min), (0...180).contains(max), min < max else { throw ViewingPlanSettings.invalid() }
                    fields["orbit"] = .object(["minYaw": .number(min * .pi / 180), "maxYaw": .number(max * .pi / 180)])
                }
            }
            return .object(fields)
        }
    }
    public var usesPlan: Bool
    public var stops: [Stop]
    public init(draft: NativeExhibitionDraft) throws {
        let settings = try ViewingPlanSettings.read(in: draft)
        usesPlan = settings != nil; stops = settings?.stops.map(Stop.init) ?? []
    }
    public mutating func append(placementId: String) {
        guard stops.count < 128, !stops.contains(where: { $0.placementId.lowercased() == placementId.lowercased() }) else { return }
        usesPlan = true; stops.append(Stop(placementId: placementId))
    }
    public mutating func move(_ placementId: String, by offset: Int) {
        guard offset == -1 || offset == 1, let index = stops.firstIndex(where: { $0.placementId == placementId }), stops.indices.contains(index + offset) else { return }
        stops.swapAt(index, index + offset)
    }
    public func command(in draft: NativeExhibitionDraft) throws -> ViewingPlanCommand {
        let plan = usesPlan ? try ViewingPlanSettings(value: .object(["version": .integer(1), "stops": .array(try stops.map { try $0.value(in: draft) })]), in: draft) : nil
        return ViewingPlanCommand(plan: plan)
    }
}

public struct ViewingPlanCommand: Equatable, Sendable {
    public let plan: ViewingPlanSettings?
    public init(plan: ViewingPlanSettings?) { self.plan = plan }
    /// Only the negotiated optional profile may change. Do not generate cameras,
    /// rewrite Presentation/Curation, reorder placements or store visitor state.
    public func proposedCandidate(in draft: NativeExhibitionDraft) throws -> DocumentValue {
        _ = try ViewingPlanSettings.read(in: draft)
        var candidate = draft.candidate
        var extensions: [String: DocumentValue]
        if case .object(let existing) = candidate["extensions"] { extensions = existing }
        else if candidate["extensions"] == .null { extensions = [:] }
        else { throw ViewingPlanSettings.invalid() }
        if let plan {
            _ = try ViewingPlanSettings(value: plan.value, in: draft)
            if extensions[ViewingPlanSettings.namespace] == plan.value { return candidate }
            extensions[ViewingPlanSettings.namespace] = plan.value
        } else {
            guard extensions.removeValue(forKey: ViewingPlanSettings.namespace) != nil else { return candidate }
        }
        candidate["extensions"] = .object(extensions)
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }
}

/// A shared-validator reply is still pinned to its exact base and form command.
/// Constructing this comparison alone is not runtime/GUI qualification.
public struct ViewingPlanPreview: Equatable, Sendable {
    public let base: NativeExhibitionDraft
    public let command: ViewingPlanCommand
    public let candidate: DocumentValue
    public init(candidate: DocumentValue, base: NativeExhibitionDraft, command: ViewingPlanCommand) throws {
        guard try candidate == command.proposedCandidate(in: base) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        self.base = base; self.command = command; self.candidate = candidate
    }
    public func apply(to draft: NativeExhibitionDraft, command: ViewingPlanCommand) throws -> NativeExhibitionDraft {
        guard draft == base, command == self.command,
              try candidate == command.proposedCandidate(in: draft) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        var next = draft; try next.replaceCandidate(candidate); return next
    }
}
