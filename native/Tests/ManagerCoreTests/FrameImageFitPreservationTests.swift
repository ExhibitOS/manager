// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

enum FrameFitSynthetic {
    static let owner = "11111111-1111-4111-8111-111111111111"
    static let placement = "22222222-2222-4222-8222-222222222222"
    static let otherPlacement = "22222222-2222-4222-8222-222222222223"
    static let artwork = "33333333-3333-4333-8333-333333333333"
    static let asset = "44444444-4444-4444-8444-444444444444"
    static let frameId = "55555555-5555-4555-8555-555555555555"
    static let alternativeId = "55555555-5555-4555-8555-555555555556"
    static let pedestalId = "66666666-6666-4666-8666-666666666666"
    static let portalId = "77777777-7777-4777-8777-777777777777"
    static let opening = "88888888-8888-4888-8888-888888888888"
    static let model = String(repeating: "a", count: 64)
    static let image = String(repeating: "b", count: 64)
    static let ns = WorkspaceFurnishingPlacement.namespace
    static let fit: DocumentValue = .object(["version": .integer(1), "profile": .string("contain-contact-v1"),
        "sourceSha256": .string(image), "pixelWidth": .integer(1000), "pixelHeight": .integer(1000)])

    static func furnishing(id: String, kind: String, profile: String? = nil) throws -> WorkspaceFurnishing {
        let rights: DocumentValue = .object(["holder": .string("Synthetic"), "ownership": .string("owner"),
            "licenseId": .string("CC0-1.0"), "creditLine": .string(""), "permissions": .object([
                "display": .bool(true), "download": .bool(false), "export": .bool(false), "commercial": .bool(false)])])
        var value: DocumentValue = .object(["id": .string(id), "version": .integer(1), "kind": .string(kind),
            "name": .string("Synthetic"), "provenance": .string("Synthetic"), "rights": rights,
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(23), "sha256": .string(model)])])
        if let profile {
            let geometry: DocumentValue = profile == "rectangular-portal-v1"
                ? .object(["profile": .string(profile), "openingWidth": .number(1.2), "openingHeight": .number(2.2), "minZ": .number(-0.1), "maxZ": .number(0.1)])
                : .object(["profile": .string(profile), "openingWidth": .number(1), "openingHeight": .number(1), "imagePlaneZ": .number(-0.012)])
            value["fit"] = .object(["version": .integer(1), "sourceSha256": .string(model), "sourceBytes": .integer(23), "geometry": geometry])
        }
        return try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(value))
    }
    static func attachment(_ furnishingId: String = frameId) -> WorkspaceArtworkFrameCommand {
        .attach(.init(placementId: placement, artworkRevisionId: artwork, assetId: asset,
                      furnishingId: furnishingId, furnishingVersion: 1, modelSha256: model))
    }
    static func draft(version: Int64 = 10, fitted: Bool = true) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(title: "Synthetic", now: Date(timeIntervalSince1970: 1000))
        let room = draft.rooms[0]["id"].string!
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(self.artwork),
            "artworkType": .string("image"), "primaryAssetId": .string(asset),
            "dimensions": .object(["width": .number(1), "height": .number(1)]),
            "assets": .array([.object(["id": .string(asset), "mime": .string("image/png"), "sha256": .string(image)])])])
        try draft.addApprovedArtwork(artwork, roomId: room, placementId: placement, transform: NativeExhibitionDraft.identity)
        var candidate = draft.candidate, placements = candidate["placements"].items!
        var other = placements[0]; other["id"] = .string(otherPlacement); placements.append(other)
        candidate["placements"] = .array(placements)
        candidate["openings"] = .array([.object(["id": .string(opening), "type": .string("door")])])
        try draft.replaceCandidate(candidate)
        try draft.setInstallation(.init(frame: .init(kind: .flat)), for: otherPlacement)
        let frame = try furnishing(id: frameId, kind: "artwork-frame", profile: "straight-miter-ring-v1")
        draft = try XCTUnwrap(attachment().applying(to: draft, furnishing: frame, supportedExtensionVersion: 9))
        candidate = draft.candidate
        if fitted {
            var rows = candidate["extensions"][ns]["frameAttachments"].items!
            rows[0]["imageFit"] = fit; candidate["extensions"][ns]["frameAttachments"] = .array(rows)
        }
        candidate["extensions"][ns]["version"] = .integer(version)
        try draft.replaceCandidate(candidate)
        return draft
    }
}

final class FrameImageFitPreservationTests: XCTestCase {
    func testRepeatAttachRetainsPolicyAndDifferentBindingStaysLegacy() throws {
        let base = try FrameFitSynthetic.draft(), frame = try FrameFitSynthetic.furnishing(id: FrameFitSynthetic.frameId, kind: "artwork-frame", profile: "straight-miter-ring-v1")
        XCTAssertNil(try FrameFitSynthetic.attachment().applying(to: base, furnishing: frame, supportedExtensionVersion: 10))
        XCTAssertThrowsError(try FrameFitSynthetic.attachment().applying(to: base, furnishing: frame, supportedExtensionVersion: 9))
        let replacement = try FrameFitSynthetic.furnishing(id: FrameFitSynthetic.alternativeId, kind: "artwork-frame", profile: "straight-miter-ring-v1")
        let next = try XCTUnwrap(FrameFitSynthetic.attachment(FrameFitSynthetic.alternativeId).applying(to: base, furnishing: replacement, supportedExtensionVersion: 10))
        let row = try XCTUnwrap(next.candidate["extensions"][FrameFitSynthetic.ns]["frameAttachments"].items?.first)
        XCTAssertEqual(row["furnishingId"], .string(FrameFitSynthetic.alternativeId))
        guard case .object(let fields) = row else { return XCTFail("Expected row") }
        XCTAssertNil(fields["imageFit"])
        let legacy = try FrameFitSynthetic.draft(version: 9, fitted: false)
        XCTAssertNil(try FrameFitSynthetic.attachment().applying(to: legacy, furnishing: frame, supportedExtensionVersion: 9))
        XCTAssertEqual(legacy.candidate["extensions"][FrameFitSynthetic.ns]["version"], .integer(9))
    }
    func testOtherRoomFurnishingPortalAndProceduralEditPreserveFittedRows() throws {
        let base = try FrameFitSynthetic.draft(), ns = FrameFitSynthetic.ns, rows = base.candidate["extensions"][ns]["frameAttachments"]
        let pedestal = try FrameFitSynthetic.furnishing(id: FrameFitSynthetic.pedestalId, kind: "pedestal")
        let place = WorkspaceFurnishingPlacement(id: "99999999-9999-4999-8999-999999999999", roomId: base.rooms[0]["id"].string!,
            furnishingId: pedestal.id, furnishingVersion: 1, position: [0, 0, 0], yaw: 0, scale: 1)
        XCTAssertThrowsError(try place.applying(pedestal, to: base, supportedExtensionVersion: 9))
        let placed = try XCTUnwrap(place.applying(pedestal, to: base, supportedExtensionVersion: 10))
        XCTAssertEqual(placed.candidate["extensions"][ns]["version"], .integer(10))
        XCTAssertEqual(placed.candidate["extensions"][ns]["frameAttachments"], rows)
        let portal = try FrameFitSynthetic.furnishing(id: FrameFitSynthetic.portalId, kind: "portal-finish", profile: "rectangular-portal-v1")
        XCTAssertThrowsError(try WorkspacePortalFinish.applying(to: placed, openingId: FrameFitSynthetic.opening, furnishing: portal, admitsRectangularFit: true, admitsMiterFrames: true))
        let withPortal = try XCTUnwrap(WorkspacePortalFinish.applying(to: placed, openingId: FrameFitSynthetic.opening, furnishing: portal, admitsRectangularFit: true, admitsMiterFrames: true, admitsFrameImageFit: true))
        XCTAssertEqual(withPortal.candidate["extensions"][ns]["version"], .integer(10))
        XCTAssertEqual(withPortal.candidate["extensions"][ns]["frameAttachments"], rows)
        let clear = try NativeProceduralFrameClear.Proposal(command: .init(.object(["placementId": .string(FrameFitSynthetic.otherPlacement)])), draft: withPortal)
        XCTAssertEqual(try clear.apply(to: withPortal).candidate["extensions"][ns]["frameAttachments"], rows)
    }
}
