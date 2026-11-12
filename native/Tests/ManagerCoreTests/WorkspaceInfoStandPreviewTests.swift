// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceInfoStandPreviewTests: XCTestCase {
    private let oldId = "10000000-0000-4000-8000-000000000001"
    private let newId = "10000000-0000-4000-8000-000000000002"
    private let standId = "10000000-0000-4000-8000-000000000003"
    private let plaqueId = "10000000-0000-4000-8000-000000000004"
    private let artId = "10000000-0000-4000-8000-000000000005"
    private let ns = WorkspaceFurnishingPlacement.namespace

    private func value<T: Encodable>(_ input: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(input))
    }
    private func descriptor(_ id: String) throws -> WorkspaceFurnishing {
        let wire: DocumentValue = .object(["id": .string(id), "version": .integer(1), "kind": .string("info-stand"),
            "name": .string("Synthetic stand"), "provenance": .string("Synthetic test fixture"),
            "rights": .object(["holder": .string("Fixture"), "ownership": .string("owner"), "licenseId": .string("CC0-1.0"), "creditLine": .string(""),
                "permissions": .object(["display": .bool(true), "download": .bool(false), "export": .bool(false), "commercial": .bool(false)])]),
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(28), "sha256": .string(String(repeating: id == oldId ? "a" : "b", count: 64))])])
        return try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(wire))
    }
    private func state(_ draft: NativeExhibitionDraft) -> NativeDocumentState {
        NativeDocumentState(format: 1, revision: 11, workspaceId: UUID().uuidString.lowercased(), tenantId: UUID().uuidString.lowercased(),
            principalId: UUID().uuidString.lowercased(), draft: draft, etag: "synthetic-saved-head",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
    }
    private func fixture() throws -> NativeDocumentState {
        var draft = try NativeExhibitionDraft.new(title: "Synthetic preview")
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string("10000000-0000-4000-8000-000000000006"),
            "primaryAssetId": .string("10000000-0000-4000-8000-000000000007")])
        try draft.addApprovedArtwork(artwork, roomId: room, placementId: artId, transform: NativeExhibitionDraft.identity)
        for (id, x) in [(standId, -1.0), ("10000000-0000-4000-8000-000000000008", 1.0)] {
            let placement = WorkspaceFurnishingPlacement(id: id, roomId: room, furnishingId: oldId, furnishingVersion: 1,
                position: [x, 0, 0], yaw: 0, scale: 1)
            draft = try XCTUnwrap(placement.applying(descriptor(oldId), to: draft, supportedExtensionVersion: 3))
        }
        let plaque = try PlaqueSettings(id: plaqueId, roomId: room, source: .artwork(placementId: artId),
            position: [-1, 1, 0], rotation: [0, 0, 0, 1], width: 0.15, height: 0.1, visible: true,
            captionTranslations: [.init(locale: "en", artist: "Synthetic author", date: "2026", medium: "Synthetic", dimensions: "1 × 1 cm")])
        var candidate = draft.candidate
        candidate["extensions"][PlaqueSettings.namespace] = .object(["version": .integer(3), "plaques": .array([plaque.value])])
        candidate["extensions"]["example/preserved"] = .object(["value": .string("unchanged")])
        try draft.replaceCandidate(candidate)
        return state(draft)
    }
    private func replacement(_ draft: NativeExhibitionDraft) throws -> WorkspaceInfoStandReplacement {
        try WorkspaceInfoStandReplacement(value: .object([
            "placement": .object(["id": .string(standId), "roomId": draft.rooms[0]["id"], "furnishingId": .string(newId),
                "furnishingVersion": .integer(1), "position": .vector([-0.8, 0, 0.2]), "yaw": .number(0.1), "scale": .number(1)]),
            "plaque": .object(["id": .string(plaqueId), "position": .vector([-0.8, 0.9, 0.2]), "rotation": .vector([0, 0, 0, 1]),
                "dimensions": .object(["width": .number(0.16), "height": .number(0.11)])])]))
    }
    private func preview(_ base: NativeDocumentState, prospective: NativeExhibitionDraft?) throws -> WorkspaceInfoStandPreview {
        try WorkspaceInfoStandPreview(base: base, replacement: replacement(try XCTUnwrap(base.draft)), furnishing: descriptor(newId),
            supportedExtensionVersion: 3, prospectiveDraft: prospective)
    }
    private func prospective(_ base: NativeDocumentState) throws -> NativeExhibitionDraft {
        let draft = try XCTUnwrap(base.draft)
        return try XCTUnwrap(replacement(draft).applying(descriptor(newId), to: draft, supportedExtensionVersion: 3))
    }

    func testCoupledPreviewPreservesCaptionTargetAndUnrelatedGraphAndAppliesStoredDraft() throws {
        let base = try fixture(), next = try prospective(base)
        let checked = try preview(base, prospective: next), before = try XCTUnwrap(base.draft)
        XCTAssertTrue(checked.changed); XCTAssertEqual(checked.displayDraft, next)
        XCTAssertEqual(try checked.apply(to: base), next)
        XCTAssertEqual(try checked.apply(to: base), checked.displayDraft)
        XCTAssertEqual(checked.displayDraft.candidate["extensions"][ns]["placements"].items?[1], before.candidate["extensions"][ns]["placements"].items?[1])
        XCTAssertEqual(checked.displayDraft.candidate["extensions"][ns]["furnishings"], .array([try value(descriptor(oldId)), try value(descriptor(newId))]))
        let oldPlaque = before.candidate["extensions"][PlaqueSettings.namespace]["plaques"].items![0]
        let newPlaque = checked.displayDraft.candidate["extensions"][PlaqueSettings.namespace]["plaques"].items![0]
        for key in ["source", "artworkCaption", "visible", "id", "roomId"] { XCTAssertEqual(newPlaque[key], oldPlaque[key], key) }
        XCTAssertEqual(newPlaque["artworkCaption"]["translations"].items?.first?["artist"], .string("Synthetic author"))
        XCTAssertEqual(newPlaque["source"]["placementId"], .string(artId))
        XCTAssertEqual(newPlaque["position"], .vector([-0.8, 0.9, 0.2]))
        XCTAssertEqual(checked.displayDraft.candidate["extensions"]["example/preserved"], before.candidate["extensions"]["example/preserved"])
        XCTAssertEqual(checked.displayDraft.candidate["artworks"], before.candidate["artworks"])
    }

    func testOnlyNativeUpdatedAtDifferenceIsAllowedAcrossSecondDerivation() throws {
        let base = try fixture(), next = try prospective(base)
        var later = next.document; later["updatedAt"] = .string("2099-01-01T00:00:00Z")
        let differentTick = try NativeExhibitionDraft(document: later)
        let checked = try preview(base, prospective: differentTick)
        XCTAssertEqual(checked.displayDraft, differentTick)
        XCTAssertEqual(try checked.apply(to: base), differentTick)
        for stamp in [DocumentValue.null, .integer(1), .string("invalid"), .string("2099-01-01T00:00:00+00:00")] {
            later["updatedAt"] = stamp
            XCTAssertThrowsError(try preview(base, prospective: NativeExhibitionDraft(document: later)))
        }
    }

    func testRejectsUnrelatedCandidateAndDraftEnvelopeMutations() throws {
        let base = try fixture(), next = try prospective(base)
        var changed = next.document; changed["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try preview(base, prospective: NativeExhibitionDraft(document: changed)))
        changed = next.document; changed["candidate"]["extensions"]["example/preserved"]["value"] = .string("altered")
        XCTAssertThrowsError(try preview(base, prospective: NativeExhibitionDraft(document: changed)))
        changed = next.document
        var plaques = try XCTUnwrap(changed["candidate"]["extensions"][PlaqueSettings.namespace]["plaques"].items)
        plaques[0]["visible"] = .bool(false); changed["candidate"]["extensions"][PlaqueSettings.namespace]["plaques"] = .array(plaques)
        XCTAssertThrowsError(try preview(base, prospective: NativeExhibitionDraft(document: changed)))
        changed = next.document
        plaques = try XCTUnwrap(changed["candidate"]["extensions"][PlaqueSettings.namespace]["plaques"].items)
        var translations = try XCTUnwrap(plaques[0]["artworkCaption"]["translations"].items)
        translations[0]["artist"] = .string("Unexpected caption")
        plaques[0]["artworkCaption"]["translations"] = .array(translations)
        changed["candidate"]["extensions"][PlaqueSettings.namespace]["plaques"] = .array(plaques)
        XCTAssertThrowsError(try preview(base, prospective: NativeExhibitionDraft(document: changed)))
        for (key, mutation) in [("id", DocumentValue.string(UUID().uuidString.lowercased())),
                                ("editVersion", .integer(1000)), ("createdAt", .string("2000-01-01T00:00:00Z")), ("extra", .bool(true))] {
            changed = next.document; changed[key] = mutation
            XCTAssertThrowsError(try preview(base, prospective: NativeExhibitionDraft(document: changed)), key)
        }
        XCTAssertThrowsError(try preview(base, prospective: nil))
    }

    func testExactNoOpPreservesBaseAndRejectsTimestampOnlyOrOtherEdits() throws {
        let initial = try fixture(), next = try prospective(initial), base = state(next)
        let checked = try preview(base, prospective: nil)
        XCTAssertFalse(checked.changed); XCTAssertEqual(checked.displayDraft, next)
        XCTAssertEqual(try checked.apply(to: base), next)
        XCTAssertNoThrow(try preview(base, prospective: next))
        var changed = next.document; changed["updatedAt"] = .string("2099-01-01T00:00:00Z")
        XCTAssertThrowsError(try preview(base, prospective: NativeExhibitionDraft(document: changed)))
    }

    func testRequiresCleanSavedHeadValidScopeAndSupportedRegisteredDescriptor() throws {
        let base = try fixture(), next = try prospective(base)
        for (key, mutation) in [("workspaceId", DocumentValue.string("invalid")), ("tenantId", .string("invalid")),
                                ("principalId", .string("invalid")), ("etag", .null), ("etag", .string("")),
                                ("savedEditVersion", .integer(999))] {
            var raw = try value(base); raw[key] = mutation
            let changed = try JSONDecoder().decode(NativeDocumentState.self, from: JSONEncoder().encode(raw))
            XCTAssertThrowsError(try preview(changed, prospective: next), key)
        }
        var pending = base; pending.pending = .init(etag: base.etag, draft: try XCTUnwrap(base.draft))
        XCTAssertThrowsError(try preview(pending, prospective: next))
        let command = try replacement(XCTUnwrap(base.draft))
        XCTAssertThrowsError(try WorkspaceInfoStandPreview(base: base, replacement: command, furnishing: descriptor(oldId), supportedExtensionVersion: 3, prospectiveDraft: next))
        XCTAssertThrowsError(try WorkspaceInfoStandPreview(base: base, replacement: command, furnishing: descriptor(newId), supportedExtensionVersion: 2, prospectiveDraft: next))
    }

    func testApplyRequiresCompleteCapturedDocumentState() throws {
        let base = try fixture(), next = try prospective(base), checked = try preview(base, prospective: next)
        for (key, mutation) in [("format", DocumentValue.integer(2)), ("revision", .integer(12)),
                                ("workspaceId", .string(UUID().uuidString.lowercased())), ("tenantId", .string(UUID().uuidString.lowercased())),
                                ("principalId", .string(UUID().uuidString.lowercased())), ("etag", .string("new-head")),
                                ("savedEditVersion", .integer(999)), ("draft", .null)] {
            var raw = try value(base); raw[key] = mutation
            let current = try JSONDecoder().decode(NativeDocumentState.self, from: JSONEncoder().encode(raw))
            XCTAssertThrowsError(try checked.apply(to: current), key)
        }
        var current = base; current.pending = .init(etag: base.etag, draft: next)
        XCTAssertThrowsError(try checked.apply(to: current))
        var rawDraft = try XCTUnwrap(base.draft).document; rawDraft["updatedAt"] = .string("2099-01-01T00:00:00Z")
        current = base; current.draft = try NativeExhibitionDraft(document: rawDraft)
        XCTAssertThrowsError(try checked.apply(to: current))
    }
}
