// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Adds seating to a registered visual already present in the saved scene.
public struct WorkspaceBenchSeatAttachment: Sendable {
    public let benchId: String, furnishingId: String, viewpointId: String
    public let visual: BenchSettings.WorkspaceVisual
    public let furnishingVersion: Int
    public let width: Double, depth: Double, height: Double
    public let viewpoint: BenchViewpoint?

    public init(benchId: String, placementId: String, furnishingId: String, furnishingVersion: Int, modelSha256: String,
                width: Double, depth: Double, height: Double, viewpointId: String, viewpoint: BenchViewpoint?) throws {
        guard BenchSettings.uuid(benchId), workspaceFurnishingID(furnishingId), furnishingVersion == 1,
              BenchSettings.uuid(viewpointId), viewpoint == nil || viewpoint?.id == viewpointId else {
            throw BenchSettings.invalid()
        }
        visual = try .init(placementId: placementId, modelSha256: modelSha256)
        self.benchId = benchId; self.furnishingId = furnishingId; self.furnishingVersion = furnishingVersion
        self.width = width; self.depth = depth; self.height = height
        self.viewpointId = viewpointId; self.viewpoint = viewpoint
    }

    public static func placements(furnishingId: String, version: Int, in draft: NativeExhibitionDraft) throws
        -> [WorkspaceFurnishingPlacement] {
        let rows = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"].items ?? []
        return try rows.filter { $0["furnishingId"].string == furnishingId && $0["furnishingVersion"].integer == Int64(version) }
            .map { try JSONDecoder().decode(WorkspaceFurnishingPlacement.self, from: JSONEncoder().encode($0)) }
            .filter { $0.scale == 1 }
    }

    func applying(_ furnishing: WorkspaceFurnishing, to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft? {
        let placements = try Self.placements(furnishingId: furnishingId, version: furnishingVersion, in: draft)
            .filter { $0.id == visual.placementId }
        let localFurnishings = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"].items ?? []
        let descriptor = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(furnishing))
        let kind = furnishing.details.kind
        let galleryStoolSeat = BenchSettings.isGalleryStoolSeat(kind: .slab, width: width, depth: depth, height: height, visual: visual)
        guard furnishing.id == furnishingId, furnishing.version == furnishingVersion,
              kind == .bench || (kind == .stool && galleryStoolSeat && furnishing.details.model.bytes == 415_848),
              furnishing.details.model.sha256 == visual.modelSha256, placements.count == 1, let placement = placements.first,
              localFurnishings.filter({ $0["id"].string == furnishingId }) == [descriptor] else {
            throw AuthoringFailure("BENCH_SELECTION_CHANGED")
        }
        let existing = try BenchSettings.list(in: draft)
        let selected = existing.first { $0.id.lowercased() == benchId.lowercased() }
        let attached = existing.filter { $0.workspaceVisual?.placementId == visual.placementId }
        guard attached.isEmpty ? selected == nil : attached.count == 1 && selected?.id == benchId && selected?.workspaceVisual == visual else {
            throw AuthoringFailure("BENCH_SELECTION_CHANGED")
        }
        if kind == .bench && galleryStoolSeat { throw BenchSettings.invalid() }
        let settings = try BenchSettings(id: benchId, name: furnishing.details.name, roomId: placement.roomId,
            kind: .slab, position: placement.position, yaw: placement.yaw, width: width, depth: depth, height: height,
            material: .defaults(.wood), viewpointId: viewpointId, workspaceVisual: visual)
        let views = try BenchViewpoint.list(in: draft)
        let storedView = views.first { $0.id.lowercased() == viewpointId.lowercased() }
        if selected == settings, viewpoint == nil || viewpoint == storedView { return nil }
        let command: BenchCommand = selected == nil ? .add(settings, viewpoint: viewpoint) : .set(settings, viewpoint: viewpoint)
        var next = draft
        try next.replaceCandidate(command.proposedCandidate(in: draft, selectedBenchId: benchId))
        return next
    }
}
