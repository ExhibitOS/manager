// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class SceneSnapshotFurnishingsTests: XCTestCase {
    private let host = "11111111-1111-4111-8111-111111111111"
    private let asset = "22222222-22ab-42cd-82ef-2222222222ab"
    private let furnishing = "33333333-3333-4333-8333-333333333333"
    private let placement = "44444444-4444-4444-8444-444444444444"
    private let frame = "55555555-5555-4555-8555-555555555555"
    private let opening = "66666666-6666-4666-8666-666666666666"
    private let digest = String(repeating: "a", count: 64)

    private func fixture() throws -> (NativeExhibitionDraft, DocumentValue) {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["extensions"] = .object([:])
        candidate["extensions"]["org.exhibitos.studio/furnishing-visuals"] = .object([
            "version": .integer(1), "assets": .array([.object(["id": .string(asset), "sha256": .string(digest)])]),
            "bindings": .array([.object(["hostKind": .string("light"), "hostId": .string(host), "assetId": .string(asset)])])])
        let identity: [String: DocumentValue] = ["furnishingId": .string(furnishing), "furnishingVersion": .integer(1)]
        var placed = identity, attached = identity, doorway = identity
        placed["id"] = .string(placement); attached["placementId"] = .string(frame); doorway["openingId"] = .string(opening)
        candidate["extensions"][WorkspaceFurnishingPlacement.namespace] = .object([
            "version": .integer(11), "furnishings": .array([.object(["id": .string(furnishing), "version": .integer(1), "model": .object(["sha256": .string(digest)])])]),
            "placements": .array([.object(placed)]), "frameAttachments": .array([.object(attached)]), "openingAttachments": .array([.object(doorway)]),
            // Ceiling attachment adds no duplicate renderer host row.
            "ceilingAttachments": .array([.object(["placementId": .string(placement)])])])
        try draft.replaceCandidate(candidate)
        let catalog: DocumentValue = .object(["kind": .string("light"), "id": .string(host), "assetId": .string(asset), "modelSha256": .string(digest), "status": .string("ready")])
        let rows = [placement, frame, opening].map { id -> DocumentValue in
            var row = identity; row["placementId"] = .string(id); row["modelSha256"] = .string(digest); row["status"] = .string("ready")
            return .object(row)
        }
        return (draft, .object(["catalog": .array([catalog]), "workspace": .array(rows)]))
    }
    private func rejected(_ value: DocumentValue, _ draft: NativeExhibitionDraft, file: StaticString = #filePath, line: UInt = #line) {
        do { _ = try SceneSnapshotFurnishings.checked(value, draft: draft); XCTFail("Invalid furnishing evidence accepted", file: file, line: line) }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "SCENE_SNAPSHOT_INVALID", file: file, line: line) }
        catch { XCTFail("Unexpected error", file: file, line: line) }
    }
    private func replacing(_ value: DocumentValue, _ collection: String, _ index: Int, _ key: String, _ replacement: DocumentValue) -> DocumentValue {
        var result = value, rows = value[collection].items!
        rows[index][key] = replacement; result[collection] = .array(rows)
        return result
    }
    func testCompleteCatalogPlacementFrameAndOpeningEvidenceIsRetained() throws {
        let (draft, value) = try fixture()
        XCTAssertEqual(try SceneSnapshotFurnishings.checked(value, draft: draft), value)
        var fallback = replacing(value, "catalog", 0, "status", .string("fallback"))
        fallback = replacing(fallback, "workspace", 0, "status", .string("pending"))
        fallback = replacing(fallback, "workspace", 1, "status", .string("failed"))
        XCTAssertEqual(try SceneSnapshotFurnishings.checked(fallback, draft: draft), fallback)
    }
    func testMissingExtraDuplicateAndOpenRowsFailClosed() throws {
        let (draft, good) = try fixture()
        rejected(.null, draft)
        var bad = good; bad["ignored"] = .bool(true); rejected(bad, draft)
        for collection in ["catalog", "workspace"] {
            bad = good; bad[collection] = .array([]); rejected(bad, draft)
            bad = good; bad[collection] = .array(good[collection].items! + [good[collection].items![0]]); rejected(bad, draft)
            bad = replacing(good, collection, 0, "extra", .bool(true)); rejected(bad, draft)
        }
        bad = good; var rows = good["workspace"].items!; rows[1] = rows[0]; bad["workspace"] = .array(rows); rejected(bad, draft)
    }
    func testWrongHostAssetVersionDigestAndStatusCannotClaimReadiness() throws {
        let (draft, good) = try fixture()
        for (collection, key, value) in [
            ("catalog", "kind", DocumentValue.string("bench")), ("catalog", "id", .string(placement)),
            ("catalog", "assetId", .string(furnishing)), ("catalog", "modelSha256", .string(String(repeating: "b", count: 64))),
            ("catalog", "status", .string("pending")), ("catalog", "status", .bool(true)),
            ("workspace", "placementId", .string(host)), ("workspace", "furnishingId", .string(asset)),
            ("workspace", "furnishingVersion", .integer(2)), ("workspace", "furnishingVersion", .bool(true)),
            ("workspace", "modelSha256", .string(String(repeating: "b", count: 64))), ("workspace", "status", .string("fallback"))] {
            let bad = replacing(good, collection, 0, key, value); rejected(bad, draft)
        }
    }
    func testMalformedSavedDescriptorAndOversizeEvidenceAreRejected() throws {
        let (draft, good) = try fixture()
        var changed = draft, candidate = draft.candidate
        var descriptors = candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"].items!
        descriptors[0]["model"]["sha256"] = .string("invalid")
        candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"] = .array(descriptors)
        try changed.replaceCandidate(candidate); rejected(good, changed)
        let huge = replacing(good, "catalog", 0, "status", .string(String(repeating: "a", count: SceneSnapshotFurnishings.maximumBytes)))
        rejected(huge, draft)
    }
    func testMixedCaseAssetReferenceResolvesToActualDeclaredIdentity() throws {
        let (draft, good) = try fixture()
        var changed = draft, candidate = draft.candidate
        let ns = "org.exhibitos.studio/furnishing-visuals"
        var bindings = candidate["extensions"][ns]["bindings"].items!
        bindings[0]["assetId"] = .string(asset.uppercased())
        candidate["extensions"][ns]["bindings"] = .array(bindings)
        try changed.replaceCandidate(candidate)
        XCTAssertEqual(try SceneSnapshotFurnishings.checked(good, draft: changed), good)
        rejected(replacing(good, "catalog", 0, "assetId", .string(asset.uppercased())), changed)
    }
}
