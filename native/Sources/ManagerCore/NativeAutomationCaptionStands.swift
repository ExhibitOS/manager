// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Exact protected ordinary edit for the two reviewed r209 plaque supports.
enum NativeAutomationCaptionStands {
    static let model = "ec9e424fe07eceeb4b422c0ed3dfa6981ac9f5dc335bd6ed32954d2c1a190752"
    private static let sourceETag = "\"studio-r209-f8c8c14fcbce761fb552dc7028093e35e1afc51c9066976be7cb90b2e688785f\""
    private static let room = "d295d832-8ec0-40b2-8d9d-72d6a1ac3666"
    private static let furnishing = "a358b4d2-8f6c-5c0f-8421-a06e90624a94"

    private struct Target: Sendable {
        let stand: WorkspaceFurnishingPlacement
        let plaque: String
        let artworkPlacement: String
        let plaquePosition: [Double]
        let plaqueRotation: [Double]
    }

    private static let targets = [
        Target(stand: .init(id: "e81bf2a0-7cec-53d3-9bec-884c202faa33", roomId: room,
            furnishingId: furnishing, furnishingVersion: 1, position: [-3.45167856, 0, -2.20167856],
            yaw: 0.7853981633974483, scale: 1),
            plaque: "35ec53a9-99a5-43c5-9e78-f1e018faf375", artworkPlacement: "0fe2ce5c-ae6a-4298-ac3e-52a7454f2012",
            plaquePosition: [-3.447609310090159, 0.988419864895628, -2.197609310090159],
            plaqueRotation: [-0.496400110977961, 0.322751933311031, 0.205615658330581, 0.779192094681627]),
        Target(stand: .init(id: "54b26da4-2146-5f43-8a63-4cf5dcc063fd", roomId: room,
            furnishingId: furnishing, furnishingVersion: 1, position: [3.65167856, 0, -2.30167856],
            yaw: -0.7853981633974483, scale: 1),
            plaque: "05c334fe-fd20-4f09-893b-55d436980a3c", artworkPlacement: "2e2291ec-1368-40bc-a003-e643c7378e8d",
            plaquePosition: [3.647609310090159, 0.988419864895628, -2.297609310090159],
            plaqueRotation: [-0.496400110977961, -0.322751933311031, -0.205615658330581, 0.779192094681627]),
    ]

    static func checkPlacement(_ placement: WorkspaceFurnishingPlacement, model digest: String?, intake: String?) throws {
        guard digest == model, intake == nil, targets.contains(where: { $0.stand == placement }) else {
            throw BridgeFailure("VIEWPORT_COMMAND_DENIED")
        }
    }

    static func checkSource(_ current: NativeDocumentState) throws {
        guard current.etag == sourceETag, current.savedEditVersion == 266, let draft = current.draft else {
            throw BridgeFailure("EXHIBITION_COMPARISON_STALE")
        }
        try checkPlaques(draft)
    }

    static func checkSave(_ current: NativeDocumentState) throws {
        try checkSource(current)
        guard let draft = current.draft else { throw BridgeFailure("EXHIBITION_REQUIRED") }
        try checkStands(draft)
    }

    static func checkOutput(_ current: NativeDocumentState) throws {
        guard !current.dirty, current.pending == nil, current.etag?.hasPrefix("\"studio-r210-") == true,
              let draft = current.draft else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        try checkPlaques(draft)
        try checkStands(draft)
    }

    private static func checkPlaques(_ draft: NativeExhibitionDraft) throws {
        let plaques = try PlaqueSettings.list(in: draft)
        for target in targets {
            guard plaques.contains(where: { plaque in
                plaque.id == target.plaque && plaque.roomId == room
                    && plaque.source == .artwork(placementId: target.artworkPlacement)
                    && plaque.position == target.plaquePosition && plaque.rotation == target.plaqueRotation
            }) else { throw BridgeFailure("EXHIBITION_COMPARISON_STALE") }
        }
    }

    private static func checkStands(_ draft: NativeExhibitionDraft) throws {
        let raw = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]
        guard let rows = raw["placements"].items else { throw BridgeFailure("VIEWPORT_COMMAND_DENIED") }
        let expected = targets.map(\.stand)
        let expectedIDs = Set(expected.map(\.id))
        let matching = rows.filter { row in row["id"].string.map(expectedIDs.contains) == true }
        let actual = try matching.map { row in
            try JSONDecoder().decode(WorkspaceFurnishingPlacement.self, from: JSONEncoder().encode(row))
        }
        guard actual.count == expected.count, expected.allSatisfy(actual.contains) else {
            throw BridgeFailure("VIEWPORT_COMMAND_DENIED")
        }
    }
}
