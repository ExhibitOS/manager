// SPDX-License-Identifier: Apache-2.0
import Foundation

struct WorkspaceDefaultSeatProposal: Encodable, Sendable {
    static let installedBackedBenchId = "89b3fd6d-b8b8-4568-9094-93f0aac1e63f"

    struct Target: Encodable, Sendable {
        let placementId: String
        let benchId = UUID().uuidString.lowercased()
        let viewpointId = UUID().uuidString.lowercased()
    }

    let draft: NativeExhibitionDraft
    let targets: [Target]

    init(draft: NativeExhibitionDraft, placementIds: [String]) throws {
        guard (1...32).contains(placementIds.count), Set(placementIds).count == placementIds.count,
              placementIds.allSatisfy(workspaceFurnishingID) else { throw BenchSettings.invalid() }
        self.draft = draft
        targets = placementIds.map { Target(placementId: $0) }
    }

    static func isInstalledBackedBench(_ placement: WorkspaceFurnishingPlacement) -> Bool {
        placement.furnishingId == installedBackedBenchId && placement.furnishingVersion == 1 && placement.scale == 1
    }

    func needsInstalledBackedBenchAuthorization() throws -> Bool {
        let linked = Set(try BenchSettings.list(in: draft).compactMap { $0.workspaceVisual?.placementId })
        let unlinkedTargets = Set(targets.map(\.placementId)).subtracting(linked)
        let workspace = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]
        return workspace["placements"].items?.contains { row in
            guard let id = row["id"].string, unlinkedTargets.contains(id) else { return false }
            return row["furnishingId"].string == Self.installedBackedBenchId
                && row["furnishingVersion"].integer == 1 && row["scale"].double == 1
        } ?? false
    }

    func applying(_ response: DocumentValue, to base: NativeExhibitionDraft,
                  installedBackedBenchAuthorized: Bool = false) throws -> NativeExhibitionDraft? {
        guard case .object(let fields) = response, Set(fields.keys) == ["additions"],
              let additions = response["additions"].items, additions.count <= targets.count else { throw BenchSettings.invalid() }
        let workspace = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]
        let linked = Set(try BenchSettings.list(in: draft).compactMap { $0.workspaceVisual?.placementId })
        let decoder = JSONDecoder(), encoder = JSONEncoder()
        var expected: [String: (target: Target, placement: WorkspaceFurnishingPlacement, furnishing: WorkspaceFurnishing)] = [:]
        for target in targets {
            guard let row = workspace["placements"].items?.first(where: { $0["id"].string == target.placementId }) else {
                throw BenchSettings.invalid()
            }
            if linked.contains(target.placementId) { continue }
            let placement = try decoder.decode(WorkspaceFurnishingPlacement.self, from: encoder.encode(row))
            guard let item = workspace["furnishings"].items?.first(where: {
                $0["id"].string == placement.furnishingId && $0["version"].integer == Int64(placement.furnishingVersion)
            }) else { throw BenchSettings.invalid() }
            let furnishing = try decoder.decode(WorkspaceFurnishing.self, from: encoder.encode(item))
            let eligible = Self.isInstalledBackedBench(placement)
                ? installedBackedBenchAuthorized && furnishing.details.kind == .bench
                : BenchSettings.isDefaultSeatModel(furnishing.details.model, kind: furnishing.details.kind)
            if eligible {
                expected[target.placementId] = (target, placement, furnishing)
            }
        }
        guard additions.count == expected.count else { throw BenchSettings.invalid() }
        if additions.isEmpty { return nil }

        var seats = (try BenchSettings.extensionValue(BenchSettings.namespace, in: draft))?["benches"].items ?? []
        var presentation = try BenchSettings.extensionValue(BenchSettings.presentationNamespace, in: draft)
            ?? .object(["version": .integer(1), "viewpoints": .array([]), "credits": .string("")])
        guard var views = presentation["viewpoints"].items else { throw BenchSettings.invalid() }
        for addition in additions {
            guard case .object(let fields) = addition, Set(fields.keys) == ["placementId", "bench", "viewpoint"],
                  let placementId = addition["placementId"].string,
                  let (target, placement, furnishing) = expected.removeValue(forKey: placementId) else { throw BenchSettings.invalid() }
            let seat = try BenchSettings(value: addition["bench"])
            let view = try BenchViewpoint(value: addition["viewpoint"])
            let validSizeAndView = Self.isInstalledBackedBench(placement)
                ? Self.matchesInstalledBackedBench(seat, view: view, placement: placement)
                : BenchSettings.defaultSeatSize(kind: seat.kind, width: seat.width, depth: seat.depth, height: seat.height,
                                                visual: seat.workspaceVisual, scale: placement.scale)
            guard seat.id == target.benchId, seat.viewpointId == target.viewpointId, view.id == target.viewpointId,
                  seat.name == furnishing.details.name, view.name == furnishing.details.name, view.fov == 50,
                  seat.material == .defaults(.wood),
                  seat.workspaceVisual?.placementId == placementId, seat.workspaceVisual?.modelSha256 == furnishing.details.model.sha256,
                  seat.roomId == placement.roomId, view.roomId == placement.roomId, seat.position == placement.position, seat.yaw == placement.yaw,
                  validSizeAndView else { throw BenchSettings.invalid() }
            seats.append(addition["bench"])
            views.append(addition["viewpoint"])
        }

        var candidate = draft.candidate
        presentation["viewpoints"] = .array(views)
        candidate["extensions"][BenchSettings.presentationNamespace] = presentation
        candidate["extensions"][BenchSettings.namespace] = .object(["version": .integer(1), "benches": .array(seats)])
        var next = base
        try next.replaceCandidate(candidate)
        _ = try BenchSettings.list(in: next)
        guard try encoder.encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return next
    }

    private static func matchesInstalledBackedBench(_ seat: BenchSettings, view: BenchViewpoint,
                                                    placement: WorkspaceFurnishingPlacement) -> Bool {
        let x = placement.position[0], y = placement.position[1] + 0.45 + 0.7, z = placement.position[2]
        let position = [x, y, z]
        let target = [x - 2 * sin(placement.yaw), y, z - 2 * cos(placement.yaw)]
        return seat.kind == .backed && seat.width == 1.5 && seat.depth == 0.70 && seat.height == 0.45
            && seat.workspaceVisual?.modelForward == .positiveZ
            && zip(view.position, position).allSatisfy { abs($0.0 - $0.1) <= 1e-8 }
            && zip(view.target, target).allSatisfy { abs($0.0 - $0.1) <= 1e-8 }
    }
}
