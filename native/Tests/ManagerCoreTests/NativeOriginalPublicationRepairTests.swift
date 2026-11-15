// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeOriginalPublicationRepairTests: XCTestCase {
    private let source = "7af55e9ecddb2da8a76e5985fbb184e59150ca5b7c799a058cbc7d334bd48a29"
    private let repair = "09eacf46cabcc7564d0ad9dee50bf5c40f398a69667d8886e9558d6db67fdbd1"
    private func value<T: Encodable>(_ item: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(item))
    }
    private func decode<T: Decodable>(_ item: DocumentValue, as: T.Type) throws -> T {
        try JSONDecoder().decode(T.self, from: JSONEncoder().encode(item))
    }
    private func digest(_ request: DocumentValue) throws -> String {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return nativeImportSHA256(try encoder.encode(request))
    }
    private func fixture() throws -> (NativeDocumentState, NativePublicationState, DocumentValue) {
        let owner = UUID().uuidString.lowercased(), id = UUID().uuidString.lowercased()
        let draft = try NativeExhibitionDraft.new(title: "Synthetic repair source")
        let etag = "\"studio-r7-" + String(repeating: "a", count: 64) + "\""
        let document = NativeDocumentState(format: 1, revision: 31, workspaceId: owner, tenantId: owner,
            principalId: owner, draft: draft, etag: etag, savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let request: DocumentValue = .object(["id": .string(id), "operation": .string("export-static-native-original"),
            "expectedRevision": .integer(31), "expectedEditVersion": draft.document["editVersion"],
            "outputPath": .string("/private/synthetic/retained-output"), "command": .object(["etag": .string(etag)])])
        var state = NativePublicationState(format: 1, revision: 3, workspaceId: owner, tenantId: owner,
            principalId: owner, pending: nil, completed: nil)
        let intent = try state.nativeOriginalIntent(in: document, runtimeManifest: source,
            requestId: id, requestSHA256: digest(request))
        try state.stageNativeOriginal(intent)
        return (document, state, request)
    }
    private func stage(_ state: NativePublicationState, _ document: NativeDocumentState,
                       _ request: DocumentValue, manifest: String? = nil) throws -> NativePublicationState {
        var next = state
        let intent = try next.nativeOriginalIntent(in: document, runtimeManifest: manifest ?? repair,
            requestId: request["id"].string!, requestSHA256: digest(request))
        try next.stageNativeOriginal(intent)
        return next
    }
    func testExactRepairRestagesTheUnchangedColdIntentAndImmutableAttempt() throws {
        let (document, state, request) = try fixture()
        let cold = try decode(value(state), as: NativePublicationState.self)
        #if DEBUG
        let next = try stage(cold, document, request)
        XCTAssertEqual(next, state)
        XCTAssertEqual(next.pending, state.pending)
        XCTAssertFalse(next.nativeOriginalAttempts![request["id"].string!]!.exportClaimed)
        XCTAssertEqual(next.nativeOriginalAttempts, state.nativeOriginalAttempts)
        XCTAssertEqual(try stage(cold, document, request, manifest: source), state)
        // The ordinary current-runtime plan is different; recovery retained the original.
        let currentPlan = try nativePublicationPlanBinding(sourceExhibitionId: document.draft!.id, etag: document.etag!,
            runtimeManifestSha256: repair, watermark: document.draft!.publicationWatermark(),
            publicationProfile: "native-original-v1", nativeOriginalRequestSHA256: digest(request))
        XCTAssertNotEqual(next.pending!.planBindingSha256, currentPlan)
        #else
        XCTAssertThrowsError(try stage(cold, document, request))
        #endif
    }
    func testChangedWireRequestVersionsPathIdAndETagCannotReuseTheOldPlan() throws {
        let (document, state, request) = try fixture()
        for (key, replacement) in [("outputPath", .string("/private/synthetic/different")),
            ("id", .string(UUID().uuidString.lowercased())), ("expectedRevision", .integer(32)),
            ("expectedEditVersion", .integer(999)), ("operation", .string("export-static"))] as [(String, DocumentValue)] {
            var wrong = request; wrong[key] = replacement
            XCTAssertThrowsError(try stage(state, document, wrong), key)
        }
        var wrong = request; wrong["command"]["etag"] = .string("changed")
        XCTAssertThrowsError(try stage(state, document, wrong))
        wrong = request; wrong["command"]["extraAuthority"] = .bool(true)
        XCTAssertThrowsError(try stage(state, document, wrong))
        for manifest in [String(repeating: "f", count: 64), repair.uppercased(),
                         "41e741a15bedf72750de1033c3afd462503fc5901100a1660caf14ce25a980a1"] {
            XCTAssertThrowsError(try stage(state, document, request, manifest: manifest))
        }
    }
    func testOwnerSourceAndRetainedPurposeFencesRemainMandatory() throws {
        let (document, state, request) = try fixture()
        for key in ["workspaceId", "tenantId", "principalId", "etag", "savedEditVersion"] {
            var wrong = try value(document)
            wrong[key] = key == "savedEditVersion" ? .integer(999) : .string(UUID().uuidString.lowercased())
            XCTAssertThrowsError(try stage(state, decode(wrong, as: NativeDocumentState.self), request), key)
        }
        var changedSource = try value(document); changedSource["draft"]["candidate"]["title"] = .string("Different source")
        XCTAssertThrowsError(try stage(state, decode(changedSource, as: NativeDocumentState.self), request))
        var changedUUID = try value(document); changedUUID["draft"]["exhibitionId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try stage(state, decode(changedUUID, as: NativeDocumentState.self), request))
        var changedSnapshot = try value(state); changedSnapshot["pending"]["snapshot"]["exhibitionId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try stage(decode(changedSnapshot, as: NativePublicationState.self), document, request))
        for key in ["mode", "sourceExhibitionId", "snapshotEtag", "planBindingSha256"] {
            var wrong = try value(state); wrong["pending"][key] = .string(key == "mode" ? "reusable-draft-v1" : UUID().uuidString.lowercased())
            XCTAssertThrowsError(try stage(decode(wrong, as: NativePublicationState.self), document, request), key)
        }
        let id = request["id"].string!
        for key in ["exportClaimed", "planBindingSha256"] {
            var wrong = try value(state); wrong["nativeOriginalAttempts"][id][key] = key == "exportClaimed" ? .bool(true) : .string(String(repeating: "b", count: 64))
            XCTAssertThrowsError(try stage(decode(wrong, as: NativePublicationState.self), document, request), key)
        }
        var missing = try value(state); missing["nativeOriginalAttempts"] = .object([:])
        XCTAssertThrowsError(try stage(decode(missing, as: NativePublicationState.self), document, request))
    }
    func testFreshRepairIntentUsesCurrentPlanAndClaimedOrCompletedReplayStillFails() throws {
        let (document, state, request) = try fixture()
        var fresh = state; fresh.pending = nil; fresh.nativeOriginalAttempts = nil
        let next = try stage(fresh, document, request)
        XCTAssertNotEqual(next.pending!.planBindingSha256, state.pending!.planBindingSha256)
        var completed = state; let intent = completed.pending!; completed.pending = nil
        let pubID = UUID().uuidString.lowercased()
        completed.completed = NativePublicationCompleted(pending: intent,
            publication: NativePublication(publicationId: pubID, publicUrl: "/p/" + pubID,
                revisionSha256: String(repeating: "c", count: 64), publishedAt: "2026-11-15T00:00:00Z", status: "published"))
        try completed.claimNativeOriginalExport(intent)
        XCTAssertThrowsError(try stage(completed, document, request))
        XCTAssertThrowsError(try completed.completed!.refuseNativeOriginalExportReplay(intent))
    }
}
