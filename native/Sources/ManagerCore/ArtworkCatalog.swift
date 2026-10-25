// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A source artwork is distinct from any number of exhibition placements.
/// Catalog filtering and selection never mutate the artwork or draft.
public struct ArtworkCatalogEntry: Identifiable, Equatable, Sendable {
    public let id: String
    public let revision: Int
    public let artwork: DocumentValue
    public init(id: String, revision: Int, artwork: DocumentValue) {
        self.id = id.lowercased(); self.revision = revision; self.artwork = artwork
    }
    public var title: String { artwork["metadata"]["title"].string ?? "제목 없는 작품" }
    public var artist: String { artwork["metadata"]["artist"].string ?? "작가 미상" }
    public var kind: String { artwork["artworkType"].string ?? "" }
    public var typeLabel: String { kind == "painting" ? "회화" : kind == "sculpture" ? "조각" : "작품" }
    public var dimensions: [Double] { ["width", "height", "depth"].compactMap { artwork["dimensions"][$0].double } }
    public var primaryMime: String? {
        artwork["assets"].items?.first { $0["id"].string == artwork["primaryAssetId"].string }?["mime"].string
    }
    public func matches(query: String, filter: ArtworkCatalogFilter, in draft: NativeExhibitionDraft) -> Bool {
        let text = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let matchesText = text.isEmpty || [title, artist, typeLabel].contains { $0.range(of: text, options: [.caseInsensitive, .diacriticInsensitive]) != nil }
        guard matchesText else { return false }
        switch filter {
        case .all: return true
        case .painting: return kind == "painting"
        case .sculpture: return kind == "sculpture"
        case .unplaced: return draft.catalogPlacementIDs(for: id).isEmpty
        }
    }
}
public enum ArtworkCatalogFilter: String, CaseIterable, Hashable, Sendable {
    case all, painting, sculpture, unplaced
    public var label: String {
        switch self { case .all: "전체"; case .painting: "회화"; case .sculpture: "조각"; case .unplaced: "미배치" }
    }
}
extension NativeExhibitionDraft {
    public func catalogSourceID(for placementID: String) -> String? {
        guard let revision = placements.first(where: { $0["id"].string == placementID })?["artworkRevisionId"].string,
              let artwork = artworks.first(where: { $0["revisionId"].string == revision }) else { return nil }
        return (artwork["extensions"]["org.exhibitos.studio/cms"]["artworkId"].string ?? artwork["id"].string)?.lowercased()
    }
    public func catalogPlacementIDs(for sourceID: String) -> [String] {
        placements.compactMap { placement in
            guard let id = placement["id"].string, catalogSourceID(for: id) == sourceID.lowercased() else { return nil }
            return id
        }
    }
    public func catalogPlacement(for sourceID: String, preferring currentID: String?) -> String? {
        let matches = catalogPlacementIDs(for: sourceID)
        if let currentID, matches.contains(currentID) { return currentID }
        return matches.first
    }
}
