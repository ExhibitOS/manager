// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class FrameImageFitReconciliationTests: XCTestCase {
    private let p1 = "10000000-0000-4000-8000-000000000001"
    private let p2 = "10000000-0000-4000-8000-000000000002"
    private let revision = "10000000-0000-4000-8000-000000000003"
    private let asset = "10000000-0000-4000-8000-000000000004"
    private let furnishing = "10000000-0000-4000-8000-000000000005"
    private let ns = WorkspaceFurnishingPlacement.namespace
    private var fit: DocumentValue { .object(["version": .integer(1), "profile": .string("contain-contact-v1"),
        "sourceSha256": .string(String(repeating: "a", count: 64)), "pixelWidth": .integer(1000), "pixelHeight": .integer(1000)]) }

    private func draft(_ version: Int64 = 7, fitted: Bool = false) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(now: Date(timeIntervalSince1970: 1000)), candidate = draft.candidate
        let modelHash = String(repeating: "b", count: 64)
        candidate["placements"] = .array([p1, p2].map { id in .object(["id": .string(id), "artworkRevisionId": .string(revision), "assetId": .string(asset), "transform": NativeExhibitionDraft.identity]) })
        candidate["artworks"] = .array([.object(["revisionId": .string(revision), "artworkType": .string("image"), "primaryAssetId": .string(asset),
            "dimensions": .object(["width": .number(1), "height": .number(1)]),
            "assets": .array([.object(["id": .string(asset), "mime": .string("image/png"), "sha256": .string(String(repeating: "a", count: 64))])])])])
        let descriptor: DocumentValue = .object(["id": .string(furnishing), "version": .integer(1), "kind": .string("artwork-frame"),
            "model": .object(["sha256": .string(modelHash), "bytes": .integer(123)]),
            "fit": .object(["version": .integer(1), "sourceSha256": .string(modelHash), "sourceBytes": .integer(123),
                "geometry": .object(["profile": .string(version == 9 ? "straight-miter-ring-v1" : "straight-ring-v1"),
                    "openingWidth": .number(1), "openingHeight": .number(1), "imagePlaneZ": .number(-0.012)])])])
        var rows: [DocumentValue] = [p1, p2].map { id in .object(["placementId": .string(id), "artworkRevisionId": .string(revision),
            "assetId": .string(asset), "furnishingId": .string(furnishing), "furnishingVersion": .integer(1), "modelSha256": .string(modelHash)]) }
        if fitted { rows[0]["imageFit"] = fit }
        candidate["extensions"] = .object([ns: .object(["version": .integer(version), "furnishings": .array([descriptor]),
            "placements": .array([]), "frameAttachments": .array(rows), "openingAttachments": .array([])]),
            "other": .object(["opaque": .string("preserve")])])
        candidate["rights"] = .object(["opaque": .string("preserve")])
        try draft.replaceCandidate(candidate)
        return draft
    }
    private func frame(_ draft: NativeExhibitionDraft, selected: String? = nil, disabled: Bool = false, sequence: Int = 2) -> ViewportFrame {
        ViewportFrame(epoch: "10000000-0000-4000-8000-000000000006", sequence: sequence,
            tenantId: "10000000-0000-4000-8000-000000000007", principalId: "10000000-0000-4000-8000-000000000008",
            draft: draft, selection: .init(kind: "placement", id: selected ?? p1), mode: "select", view: "front", disabled: disabled)
    }
    private func candidate(_ draft: NativeExhibitionDraft, fit: DocumentValue? = nil) -> DocumentValue {
        var result = draft.candidate, rows = result["extensions"][ns]["frameAttachments"].items!
        if let fit { rows[0]["imageFit"] = fit }
        result["extensions"][ns]["frameAttachments"] = .array(rows)
        result["extensions"][ns]["version"] = .integer(10)
        return result
    }
    private func reply(_ request: FrameImageFitReconciliationRequest, candidate: DocumentValue) -> DocumentValue {
        var result: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "placementId"] { result[key] = request.value[key] }
        result["candidate"] = candidate
        return .object(result)
    }

    func testLegacyVersionsUpgradeOnlySelectedAttachment() throws {
        for version in [Int64(7), 9] {
            let base = try draft(version), request = try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base))
            let next = candidate(base, fit: fit), preview = try request.checkedReply(reply(request, candidate: next))
            XCTAssertTrue(preview.changed); XCTAssertEqual(preview.placementId, p1)
            let applied = try XCTUnwrap(preview.apply(to: base))
            XCTAssertEqual(applied.candidate, next)
            XCTAssertEqual(applied.document["editVersion"].integer, try XCTUnwrap(base.document["editVersion"].integer) + 1)
            XCTAssertEqual(next["extensions"][ns]["frameAttachments"].items?[1], base.candidate["extensions"][ns]["frameAttachments"].items?[1])
            XCTAssertEqual(next["placements"], base.candidate["placements"]); XCTAssertEqual(next["artworks"], base.candidate["artworks"])
            XCTAssertEqual(next["rights"], base.candidate["rights"])
        }
    }
    func testAvailabilityRequiresSupportedMeasuredGeometry() throws {
        let base = try draft()
        XCTAssertTrue(FrameImageFitReconciliationRequest.supports(placementId: p1, in: base))
        var candidate = base.candidate, furnishings = candidate["extensions"][ns]["furnishings"].items!
        furnishings[0]["fit"]["geometry"]["profile"] = .string("unsupported")
        candidate["extensions"][ns]["furnishings"] = .array(furnishings)
        var unsupported = base; try unsupported.replaceCandidate(candidate)
        XCTAssertFalse(FrameImageFitReconciliationRequest.supports(placementId: p1, in: unsupported))
    }
    func testExistingExactRecordIsNoopAndConflictRefused() throws {
        let base = try draft(10, fitted: true), request = try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base))
        let preview = try request.checkedReply(reply(request, candidate: base.candidate))
        XCTAssertFalse(preview.changed); XCTAssertNil(try preview.apply(to: base))
        var changed = fit; changed["pixelWidth"] = .integer(999)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: candidate(base, fit: changed))))
    }
    func testBindingsSelectionAndCandidateAreClosed() throws {
        let base = try draft(), request = try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base))
        XCTAssertThrowsError(try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base, selected: p2)))
        XCTAssertThrowsError(try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base, disabled: true)))
        for sequence in [9_007_199_254_740_992, Int.max] {
            XCTAssertThrowsError(try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base, sequence: sequence)))
        }
        XCTAssertNil(Int(exactly: UInt64.max)) // ViewportFrame.sequence is Int; this value cannot be constructed.
        XCTAssertNoThrow(try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base, sequence: 9_007_199_254_740_991)))
        let valid = reply(request, candidate: candidate(base, fit: fit))
        for key in ["epoch", "tenantId", "principalId", "requestId", "placementId"] {
            var bad = valid; bad[key] = .string("10000000-0000-4000-8000-000000000099")
            XCTAssertThrowsError(try request.checkedReply(bad))
        }
        var bad = valid; bad["sequence"] = .integer(3); XCTAssertThrowsError(try request.checkedReply(bad))
        bad = valid; bad["extra"] = .bool(true); XCTAssertThrowsError(try request.checkedReply(bad))
        var tampered = candidate(base, fit: fit); tampered["rights"]["opaque"] = .string("changed")
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: tampered)))
        tampered = candidate(base, fit: fit); tampered["extensions"][ns]["furnishings"] = .array([])
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: tampered)))
    }
    func testMalformedFitBoundsAndStalePrimaryRefused() throws {
        let base = try draft(), request = try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base))
        let invalid: [(String, DocumentValue)] = [("unexpected", .bool(true)), ("pixelWidth", .integer(0)), ("pixelWidth", .integer(8193)),
            ("pixelWidth", .integer(8192)), ("sourceSha256", .string(String(repeating: "c", count: 64)))]
        for (key, value) in invalid {
            var malformed = fit; malformed[key] = value
            if key == "pixelWidth" && value == .integer(8192) { malformed["pixelHeight"] = .integer(8192) }
            XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: candidate(base, fit: malformed))))
        }
        var stale = base.candidate, artworks = stale["artworks"].items!
        artworks[0]["primaryAssetId"] = .string(p2); stale["artworks"] = .array(artworks)
        var altered = base; try altered.replaceCandidate(stale)
        XCTAssertThrowsError(try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(altered)))
    }
    func testSameArtworkConflictsIndependentOfArrayOrderAndStaleApply() throws {
        let base = try draft(), request = try FrameImageFitReconciliationRequest(placementId: p1, frame: frame(base))
        var next = candidate(base, fit: fit), rows = next["extensions"][ns]["frameAttachments"].items!
        var conflicting = fit; conflicting["pixelWidth"] = .integer(999); rows[1]["imageFit"] = conflicting
        for order in [rows, rows.reversed().map { $0 }] {
            next["extensions"][ns]["frameAttachments"] = .array(order)
            XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: next)))
        }
        let preview = try request.checkedReply(reply(request, candidate: candidate(base, fit: fit)))
        var changed = base, changedCandidate = base.candidate
        changedCandidate["title"] = .string("another edit"); try changed.replaceCandidate(changedCandidate)
        XCTAssertThrowsError(try preview.apply(to: changed))
    }
}
