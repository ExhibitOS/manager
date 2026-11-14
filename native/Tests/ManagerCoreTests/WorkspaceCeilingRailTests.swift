// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceCeilingRailTests: XCTestCase {
    private let rail = "10000000-0000-4000-8000-000000000001"
    private let placement = "10000000-0000-4000-8000-000000000002"
    private let ceiling = "10000000-0000-4000-8000-000000000003"
    private let modelHash = String(repeating: "a", count: 64)

    private func descriptor() -> DocumentValue {
        .object(["id": .string(rail), "version": .integer(1), "kind": .string("ceiling-rail"),
            "name": .string("Synthetic rail"), "provenance": .string("Synthetic"),
            "rights": .object(["holder": .string("Synthetic"), "ownership": .string("owner"), "licenseId": .string("CC0-1.0"),
                "creditLine": .string(""), "permissions": .object(["display": .bool(true), "download": .bool(false), "export": .bool(false), "commercial": .bool(false)])]),
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(23), "sha256": .string(modelHash)]),
            "fit": .object(["version": .integer(1), "sourceBytes": .integer(23), "sourceSha256": .string(modelHash),
                "geometry": .object(["profile": .string("passive-ceiling-rail-v1"), "min": .vector([-0.5,-0.035,-0.0175]), "max": .vector([0.5,0,0.0175])])])])
    }
    private func furnishing(_ wire: DocumentValue? = nil) throws -> WorkspaceFurnishing {
        try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(wire ?? descriptor()))
    }
    private func draft() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(now: Date(timeIntervalSince1970: 1000)), candidate = draft.candidate
        var rooms = draft.rooms
        rooms[0]["dimensions"] = .object(["width": .number(4), "height": .number(3), "depth": .number(4)])
        rooms[0]["transform"] = NativeExhibitionDraft.identity
        candidate["rooms"] = .array(rooms)
        let q = sqrt(0.5)
        candidate["surfaces"] = .array([.object(["id": .string(ceiling), "roomId": rooms[0]["id"], "type": .string("ceiling"),
            "dimensions": .object(["width": .number(4), "height": .number(4)]),
            "transform": .object(["position": .vector([0,3,0]), "rotation": .vector([q,0,0,q]), "scale": .vector([1,1,1])])])])
        candidate["extensions"] = .object(["example": .object(["keep": .bool(true)])])
        try draft.replaceCandidate(candidate); return draft
    }
    private var attachment: WorkspaceCeilingRailAttachment {
        .init(placementId: placement, surfaceId: ceiling, anchor: [0,0], yaw: 0, gap: 0.01)
    }
    private func added() throws -> NativeExhibitionDraft {
        try XCTUnwrap(WorkspaceCeilingRail.applying(attachment, furnishing: furnishing(), to: draft(), supportedExtensionVersion: 11))
    }
    func testSourceBoundFitRoundTripAndDeclarationNeedsNoRequestedFit() throws {
        let item = try furnishing()
        XCTAssertEqual(item.details.kind, .ceilingRail)
        XCTAssertEqual(item.fit?.profile, .passiveCeilingRail)
        let wire = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(item))
        XCTAssertEqual(wire, descriptor())
        let request = WorkspaceFurnishingRequest(requestId: placement, details: item.details)
        let declaration = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request))
        guard case .object(let fields) = declaration else { return XCTFail("Expected declaration") }
        XCTAssertNil(fields["fitProfile"])
        var details = descriptor()
        guard case .object(var body) = details else { return XCTFail("Expected descriptor") }
        for key in ["id", "version", "fit"] { body.removeValue(forKey: key) }
        details = .object(body)
        let intakeWire: DocumentValue = .object(["intakeId": .string(rail), "requestId": .string(placement),
            "phase": .string("registered"), "declaration": details,
            "received": .object(["bytes": .integer(23), "sha256": .string(modelHash)]),
            "retryable": .bool(false), "error": .null, "furnishing": descriptor()])
        let intake = try JSONDecoder().decode(WorkspaceFurnishingIntake.self, from: JSONEncoder().encode(intakeWire))
        XCTAssertNoThrow(try intake.check(requestId: placement, details: item.details, supportedExtensionVersion: 11))
        XCTAssertThrowsError(try intake.check(requestId: placement, details: item.details, supportedExtensionVersion: 10))
        for key in ["sourceSha256", "sourceBytes", "extra"] {
            var changed = descriptor(); changed["fit"][key] = key == "sourceSha256" ? .string(String(repeating: "b", count: 64)) : .integer(24)
            XCTAssertThrowsError(try furnishing(changed))
        }
        var badBounds = descriptor(); badBounds["fit"]["geometry"]["max"] = .vector([0.5,0.1,0.0175])
        XCTAssertThrowsError(try furnishing(badBounds))
    }
    func testDerivedPlacementAndRoundTripPreserveOtherExtensions() throws {
        let base = try draft(), item = try furnishing()
        XCTAssertThrowsError(try WorkspaceCeilingRail.applying(attachment, furnishing: item, to: base, supportedExtensionVersion: 10))
        let result = try XCTUnwrap(WorkspaceCeilingRail.applying(attachment, furnishing: item, to: base, supportedExtensionVersion: 11))
        try WorkspaceCeilingRail.validate(candidate: result.candidate)
        let ns = WorkspaceFurnishingPlacement.namespace, value = result.candidate["extensions"][ns]
        XCTAssertEqual(value["version"], .integer(11)); XCTAssertEqual(value["ceilingAttachments"].items?.count, 1)
        XCTAssertEqual(value["frameAttachments"], .array([])); XCTAssertEqual(value["openingAttachments"], .array([]))
        XCTAssertEqual(result.candidate["extensions"]["example"], base.candidate["extensions"]["example"])
        XCTAssertNil(try WorkspaceCeilingRail.applying(attachment, furnishing: item, to: result, supportedExtensionVersion: 11))
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(result))
        try WorkspaceCeilingRail.validate(candidate: reopened.candidate)
    }
    func testInvalidGeometryPoseBoundsBijectionAndShapeAreRefused() throws {
        let good = try added().candidate, ns = WorkspaceFurnishingPlacement.namespace
        for variant in 0..<7 {
            var bad = good
            var ceilings = bad["extensions"][ns]["ceilingAttachments"].items!
            var placements = bad["extensions"][ns]["placements"].items!
            switch variant {
            case 0: ceilings[0]["extra"] = .bool(true)
            case 1: ceilings[0]["anchor"] = .vector([2,0])
            case 2: ceilings[0]["surfaceId"] = .string(rail)
            case 3: placements[0]["scale"] = .number(1.1)
            case 4: placements[0]["position"] = .vector([0,2,0])
            case 5: ceilings.append(ceilings[0])
            default: ceilings.removeAll()
            }
            bad["extensions"][ns]["ceilingAttachments"] = .array(ceilings)
            bad["extensions"][ns]["placements"] = .array(placements)
            XCTAssertThrowsError(try WorkspaceCeilingRail.validate(candidate: bad), "variant \(variant)")
        }
        let edge = WorkspaceCeilingRailAttachment(placementId: placement, surfaceId: ceiling, anchor: [1.7,0], yaw: 0, gap: 0.01)
        XCTAssertThrowsError(try WorkspaceCeilingRail.applying(edge, furnishing: furnishing(), to: draft(), supportedExtensionVersion: 11))
        let overGap = WorkspaceCeilingRailAttachment(placementId: placement, surfaceId: ceiling, anchor: [0,0], yaw: 0, gap: 0.11)
        XCTAssertThrowsError(try WorkspaceCeilingRail.applying(overGap, furnishing: furnishing(), to: draft(), supportedExtensionVersion: 11))
    }
    func testExtensionIdentityCollisionsRejectRailDescriptorAndPlacement() throws {
        let good = try added().candidate
        for id in [rail, placement] {
            for source in 0..<4 {
                var bad = good
                let row: DocumentValue = .object(["id": .string(id)])
                switch source {
                case 0: bad["extensions"]["org.exhibitos.viewer/space-still"] = .object(["asset": row])
                case 1: bad["extensions"]["org.exhibitos.studio/furnishing-visuals"] = .object(["assets": .array([row])])
                case 2: bad["extensions"]["org.exhibitos.studio/benches"] = .object(["benches": .array([row])])
                default: bad["extensions"]["org.exhibitos.studio/workspace-materials"] = .object(["materials": .array([row])])
                }
                XCTAssertThrowsError(try WorkspaceCeilingRail.validate(candidate: bad), "identity \(id), extension \(source)")
            }
        }
    }

    func testLegacyFramePortalAndUnrelatedPlacementPreserveRailRows() throws {
        var base = try FrameFitSynthetic.draft(), candidate = base.candidate
        let roomTemplate = try draft(), roomId = try XCTUnwrap(base.rooms[0]["id"].string)
        var rooms = base.rooms; rooms[0]["dimensions"] = roomTemplate.rooms[0]["dimensions"]
        rooms[0]["transform"] = roomTemplate.rooms[0]["transform"]; candidate["rooms"] = .array(rooms)
        var surface = try XCTUnwrap(roomTemplate.candidate["surfaces"].items?.first)
        surface["roomId"] = .string(roomId); candidate["surfaces"] = .array([surface])
        try base.replaceCandidate(candidate)
        let portalModel = try FrameFitSynthetic.furnishing(id: FrameFitSynthetic.portalId, kind: "portal-finish", profile: "rectangular-portal-v1")
        base = try XCTUnwrap(WorkspacePortalFinish.applying(to: base, openingId: FrameFitSynthetic.opening, furnishing: portalModel,
            admitsRectangularFit: true, admitsMiterFrames: true, admitsFrameImageFit: true))
        let ns = WorkspaceFurnishingPlacement.namespace
        let frames = base.candidate["extensions"][ns]["frameAttachments"]
        let portals = base.candidate["extensions"][ns]["openingAttachments"]
        let withRail = try XCTUnwrap(WorkspaceCeilingRail.applying(attachment, furnishing: furnishing(), to: base, supportedExtensionVersion: 11))
        XCTAssertEqual(withRail.candidate["extensions"][ns]["frameAttachments"], frames)
        XCTAssertEqual(withRail.candidate["extensions"][ns]["openingAttachments"], portals)
        let originalRailRows = withRail.candidate["extensions"][ns]["ceilingAttachments"]
        let frameModel = try FrameFitSynthetic.furnishing(id: FrameFitSynthetic.frameId, kind: "artwork-frame", profile: "straight-miter-ring-v1")
        XCTAssertNil(try FrameFitSynthetic.attachment().applying(to: withRail, furnishing: frameModel, supportedExtensionVersion: 11))
        let clear = try NativeProceduralFrameClear.Proposal(command: .init(.object(["placementId": .string(FrameFitSynthetic.otherPlacement)])), draft: withRail)
        XCTAssertEqual(try clear.apply(to: withRail).candidate["extensions"][ns]["ceilingAttachments"], originalRailRows)
        let viewport = ViewportFrame(epoch: "10000000-0000-4000-8000-000000000007", sequence: 1,
            tenantId: FrameFitSynthetic.owner, principalId: FrameFitSynthetic.owner, draft: withRail,
            selection: .init(kind: "placement", id: FrameFitSynthetic.placement), mode: "select", view: "front", disabled: false)
        let request = try FrameImageFitReconciliationRequest(placementId: FrameFitSynthetic.placement, frame: viewport)
        var reply: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "placementId"] { reply[key] = request.value[key] }
        reply["candidate"] = withRail.candidate
        let preview = try request.checkedReply(.object(reply))
        XCTAssertFalse(preview.changed); XCTAssertNil(try preview.apply(to: withRail))
        let room = try XCTUnwrap(withRail.rooms[0]["id"].string)
        let generic = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000005", roomId: room,
            furnishingId: rail, furnishingVersion: 1, position: [0,0,0], yaw: 0, scale: 1)
        XCTAssertThrowsError(try generic.applying(furnishing(), to: withRail, supportedExtensionVersion: 11))
        var wire = descriptor()
        guard case .object(var values) = wire else { return XCTFail("Expected descriptor") }
        values["id"] = .string("10000000-0000-4000-8000-000000000006"); values["kind"] = .string("pedestal"); values.removeValue(forKey: "fit")
        wire = .object(values)
        let pedestal = try furnishing(wire)
        let other = WorkspaceFurnishingPlacement(id: generic.id, roomId: room, furnishingId: pedestal.id,
            furnishingVersion: 1, position: [0,0,0], yaw: 0, scale: 1)
        XCTAssertThrowsError(try other.applying(pedestal, to: withRail, supportedExtensionVersion: 10))
        let updated = try XCTUnwrap(other.applying(pedestal, to: withRail, supportedExtensionVersion: 11))
        XCTAssertEqual(updated.candidate["extensions"][ns]["ceilingAttachments"], withRail.candidate["extensions"][ns]["ceilingAttachments"])
        XCTAssertEqual(updated.candidate["extensions"][ns]["frameAttachments"], frames)
        XCTAssertEqual(updated.candidate["extensions"][ns]["openingAttachments"], portals)
    }
}
