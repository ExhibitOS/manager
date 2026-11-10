// SPDX-License-Identifier: Apache-2.0
import Foundation

struct WorkspaceDefaultSeatProposal: Encodable, Sendable {
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

    func applying(_ response: DocumentValue, to base: NativeExhibitionDraft) throws -> NativeExhibitionDraft? {
        guard case .object(let fields) = response, Set(fields.keys) == ["additions"],
              let additions = response["additions"].items, additions.count <= targets.count else { throw BenchSettings.invalid() }
        if additions.isEmpty { return nil }
        let workspace = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]
        var unused = Dictionary(uniqueKeysWithValues: targets.map { ($0.placementId, $0) })
        let linked = Set(try BenchSettings.list(in: draft).compactMap { $0.workspaceVisual?.placementId })
        let decoder = JSONDecoder(), encoder = JSONEncoder()
        var seats = (try BenchSettings.extensionValue(BenchSettings.namespace, in: draft))?["benches"].items ?? []
        var presentation = try BenchSettings.extensionValue(BenchSettings.presentationNamespace, in: draft)
            ?? .object(["version": .integer(1), "viewpoints": .array([]), "credits": .string("")])
        guard var views = presentation["viewpoints"].items else { throw BenchSettings.invalid() }
        for addition in additions {
            guard case .object(let fields) = addition, Set(fields.keys) == ["placementId", "bench", "viewpoint"],
                  let placementId = addition["placementId"].string, let target = unused.removeValue(forKey: placementId),
                  !linked.contains(placementId),
                  let row = workspace["placements"].items?.first(where: { $0["id"].string == placementId }) else { throw BenchSettings.invalid() }
            let placement = try decoder.decode(WorkspaceFurnishingPlacement.self, from: encoder.encode(row))
            guard let item = workspace["furnishings"].items?.first(where: {
                $0["id"].string == placement.furnishingId && $0["version"].integer == Int64(placement.furnishingVersion)
            }) else { throw BenchSettings.invalid() }
            let furnishing = try decoder.decode(WorkspaceFurnishing.self, from: encoder.encode(item))
            let seat = try BenchSettings(value: addition["bench"])
            let view = try BenchViewpoint(value: addition["viewpoint"])
            guard BenchSettings.isDefaultSeatModel(furnishing.details.model, kind: furnishing.details.kind),
                  seat.id == target.benchId, seat.viewpointId == target.viewpointId, view.id == target.viewpointId,
                  seat.name == furnishing.details.name, view.name == furnishing.details.name, view.fov == 50,
                  seat.workspaceVisual?.placementId == placementId, seat.workspaceVisual?.modelSha256 == furnishing.details.model.sha256,
                  seat.roomId == placement.roomId, view.roomId == placement.roomId, seat.position == placement.position, seat.yaw == placement.yaw,
                  BenchSettings.defaultSeatSize(kind: seat.kind, width: seat.width, depth: seat.depth, height: seat.height,
                                               visual: seat.workspaceVisual, scale: placement.scale) else { throw BenchSettings.invalid() }
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
}
