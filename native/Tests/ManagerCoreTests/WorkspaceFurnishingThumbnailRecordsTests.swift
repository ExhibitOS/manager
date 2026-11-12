// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceFurnishingThumbnailRecordsTests: XCTestCase {
    // Synthetic PNG and issued-hash placeholders; no external fixtures or authority.
    private let png = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="
    private let candidateId = "58830074-8ca6-8af4-a506-f6976ad8ebb8" // UUID v8, not v4.
    private func value<T: Encodable>(_ input: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(input))
    }
    private struct Fixture {
        let document: NativeDocumentState
        let furnishing: WorkspaceFurnishing
        let candidate: DocumentValue
        let submission: WorkspaceFurnishingThumbnailSubmission
        var source: DocumentValue { candidate["source"] }
        var thumbnail: DocumentValue { candidate["thumbnail"] }
    }
    private func fixture(display: Bool = true) throws -> Fixture {
        let draft = try NativeExhibitionDraft.new()
        let document = NativeDocumentState(format: 1, revision: 4, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            etag: "synthetic-etag", savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        var rights = ArtworkMetadata.Rights(); rights.permissions.display = display
        let descriptor: DocumentValue = .object(["id": .string(UUID().uuidString.lowercased()), "version": .integer(1),
            "kind": .string("bench"), "name": .string("Synthetic"), "provenance": .string("Synthetic"),
            "rights": try value(rights), "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(128), "sha256": .string(String(repeating: "a", count: 64))])])
        let furnishing = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: document.tenantId,
            principalId: document.principalId, draft: draft, selection: nil, mode: "translate", view: "isometric",
            disabled: false, workspaceId: document.workspaceId, baseETag: document.etag)
        let request = try value(ViewportFurnishingThumbnailRequest(frame: frame, furnishing: furnishing))
        let bytes = try XCTUnwrap(Data(base64Encoded: png))
        let thumbnail: DocumentValue = .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
            "sha256": .string(nativeImportSHA256(bytes)), "bytes": .integer(Int64(bytes.count))])
        let source: DocumentValue = .object(["furnishingId": .string(furnishing.id), "furnishingVersion": .integer(1),
            "modelSha256": .string(furnishing.details.model.sha256), "modelBytes": .integer(128),
            "descriptorSha256": .string(String(repeating: "b", count: 64)), "rightsSha256": .string(String(repeating: "c", count: 64))])
        let receipt: DocumentValue = .object(["version": .integer(1), "purpose": .string("registered-furnishing-thumbnail"),
            "profile": .string("workspace-furnishing-thumbnail-v1"), "workspaceId": .string(document.workspaceId),
            "candidateId": .string(candidateId), "requestId": request["requestId"], "source": source, "renderRequest": request, "png": thumbnail])
        let candidate: DocumentValue = .object(["candidateId": .string(candidateId), "state": .string("ready"), "source": source,
            "thumbnail": thumbnail, "receipt": receipt, "receiptSha256": .string(String(repeating: "d", count: 64))])
        var capturePNG = thumbnail; capturePNG["base64"] = .string(png)
        let capture: DocumentValue = .object(["workspaceId": .string(document.workspaceId), "nativeRevision": .integer(4),
            "editVersion": draft.document["editVersion"], "baseETag": .string("synthetic-etag"), "request": request, "png": capturePNG])
        let submission = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: furnishing)
        return Fixture(document: document, furnishing: furnishing, candidate: candidate, submission: submission)
    }
    private func selection(_ fixture: Fixture, version: Int64, selected: Bool) -> DocumentValue {
        .object(["source": fixture.source, "selectionVersion": .integer(version), "selected": selected ? .object([
            "candidateId": fixture.candidate["candidateId"], "receiptSha256": fixture.candidate["receiptSha256"], "png": fixture.thumbnail]) : .null])
    }

    func testCandidateBindsIssuedReceiptAndOptionalSubmissionWithoutSelecting() throws {
        let f = try fixture()
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing, submission: f.submission)
        XCTAssertEqual(candidate.candidateId, candidateId)
        XCTAssertEqual(candidate.value, f.candidate); XCTAssertEqual(candidate.receipt, f.candidate["receipt"])
        XCTAssertEqual(candidate.request, f.submission.request)
        XCTAssertEqual(try candidate.checkedPNG(f.submission.pngData), f.submission.pngData)
        let current = try WorkspaceFurnishingThumbnailSelection(value: selection(f, version: 0, selected: false), document: f.document, furnishing: f.furnishing)
        XCTAssertNil(current.selected); XCTAssertFalse(current.matches(candidate))
        XCTAssertEqual(current.selectionVersion, 0)
    }

    func testRejectsActorModelReceiptAndSubmittedCaptureMismatch() throws {
        let f = try fixture()
        let mutations: [(String, DocumentValue)] = [("tenantId", .string(UUID().uuidString.lowercased())),
            ("principalId", .string(UUID().uuidString.lowercased())), ("modelBytes", .integer(129)),
            ("modelSha256", .string(String(repeating: "e", count: 64))), ("requestId", .string(UUID().uuidString.lowercased()))]
        for (key, value) in mutations {
            var changed = f.candidate; changed["receipt"]["renderRequest"][key] = value
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing), key)
        }
        for (key, value) in [("workspaceId", DocumentValue.string(UUID().uuidString.lowercased())),
                              ("candidateId", .string(UUID().uuidString.lowercased())), ("profile", .string("other")),
                              ("purpose", .string("artwork-thumbnail")), ("version", .integer(2))] {
            var changed = f.candidate; changed["receipt"][key] = value
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing), key)
        }
        var changed = f.candidate; changed["receipt"]["source"]["rightsSha256"] = .string(String(repeating: "e", count: 64))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing))
        changed = f.candidate; changed["receipt"]["png"]["bytes"] = .integer(34)
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing))
        changed = f.candidate
        let fresh = UUID().uuidString.lowercased()
        changed["receipt"]["requestId"] = .string(fresh); changed["receipt"]["renderRequest"]["requestId"] = .string(fresh)
        XCTAssertNoThrow(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing, submission: f.submission))
    }

    func testClosedNestedShapesAndIssuedHashSyntax() throws {
        let f = try fixture()
        for path in [[], ["source"], ["thumbnail"], ["receipt"], ["receipt", "renderRequest"]] as [[String]] {
            var changed = f.candidate
            switch path.count {
            case 0: changed["unexpected"] = .bool(true)
            case 1: changed[path[0]]["unexpected"] = .bool(true)
            default: changed[path[0]][path[1]]["unexpected"] = .bool(true)
            }
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing))
        }
        for key in ["modelSha256", "descriptorSha256", "rightsSha256"] {
            var changed = f.source; changed[key] = .string("invalid")
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSource(value: changed, furnishing: f.furnishing))
        }
        var changed = f.candidate; changed["receiptSha256"] = .string("invalid")
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing))
        changed = f.candidate; changed["state"] = .string("selected")
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: changed, document: f.document, furnishing: f.furnishing))
    }

    func testSelectionCASMetadataAndExactCandidateMatch() throws {
        let f = try fixture()
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing)
        let raw = selection(f, version: 7, selected: true)
        let selected = try WorkspaceFurnishingThumbnailSelection(value: raw, document: f.document, furnishing: f.furnishing)
        XCTAssertTrue(selected.matches(candidate)); XCTAssertEqual(selected.selectionVersion, 7)
        XCTAssertEqual(selected.selected?.receiptSha256, candidate.receiptSha256)
        XCTAssertEqual(selected.value, raw)
        let historical = try WorkspaceFurnishingThumbnailSelection(value: selection(f, version: 1, selected: true), document: f.document, furnishing: f.furnishing)
        XCTAssertTrue(historical.matches(candidate)); XCTAssertNotEqual(historical, selected)
        XCTAssertNoThrow(try WorkspaceFurnishingThumbnailSelection(value: selection(f, version: 8, selected: false), document: f.document, furnishing: f.furnishing))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSelection(value: selection(f, version: -1, selected: false), document: f.document, furnishing: f.furnishing))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSelection(value: selection(f, version: 0, selected: true), document: f.document, furnishing: f.furnishing))
        for (key, value) in [("candidateId", DocumentValue.string(UUID().uuidString.lowercased())),
                              ("receiptSha256", .string(String(repeating: "e", count: 64)))] {
            var changed = raw; changed["selected"][key] = value
            XCTAssertFalse(try WorkspaceFurnishingThumbnailSelection(value: changed, document: f.document, furnishing: f.furnishing).matches(candidate))
        }
        var changed = raw; changed["selected"]["png"]["sha256"] = .string(String(repeating: "e", count: 64))
        XCTAssertFalse(try WorkspaceFurnishingThumbnailSelection(value: changed, document: f.document, furnishing: f.furnishing).matches(candidate))
        changed = raw; changed["source"]["descriptorSha256"] = .string(String(repeating: "e", count: 64))
        XCTAssertFalse(try WorkspaceFurnishingThumbnailSelection(value: changed, document: f.document, furnishing: f.furnishing).matches(candidate))
        changed = raw; changed["extra"] = .bool(true)
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSelection(value: changed, document: f.document, furnishing: f.furnishing))
        changed = raw; changed["selected"]["extra"] = .bool(true)
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSelection(value: changed, document: f.document, furnishing: f.furnishing))
    }

    func testSelectionLocallyScopesActorAndWorkspaceWithoutSceneCAS() throws {
        let f = try fixture()
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing)
        let raw = selection(f, version: 1, selected: true)
        let other = NativeDocumentState(format: 1, revision: 99, workspaceId: UUID().uuidString.lowercased(),
            tenantId: f.document.tenantId, principalId: UUID().uuidString.lowercased(), draft: nil, etag: nil, savedEditVersion: nil, pending: nil)
        let differentScope = try WorkspaceFurnishingThumbnailSelection(value: raw, document: other, furnishing: f.furnishing)
        XCTAssertFalse(differentScope.matches(candidate))
        var later = f.document; later.revision += 100; later.etag = "later-etag"; later.draft = nil
        XCTAssertTrue(try WorkspaceFurnishingThumbnailSelection(value: raw, document: later, furnishing: f.furnishing).matches(candidate))
    }

    func testPNGBytesAndCurrentRightsAreRechecked() throws {
        let f = try fixture()
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing)
        var broken = f.submission.pngData; broken[broken.count - 1] ^= 1
        XCTAssertThrowsError(try candidate.checkedPNG(broken))
        var metadata = f.thumbnail; metadata["sha256"] = .string(nativeImportSHA256(broken))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailPNG(value: metadata).checkedData(broken))
        var descriptor = try value(f.furnishing)
        descriptor["rights"]["permissions"]["display"] = .bool(false)
        let denied = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: denied))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSelection(value: selection(f, version: 0, selected: false), document: f.document, furnishing: denied))
    }
}
