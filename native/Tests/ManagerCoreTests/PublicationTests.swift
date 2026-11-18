// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore
final class PublicationTests: XCTestCase {
    func testApprovedReleaseSelectionRequiresTheExactOriginalOperationAndAdmittedRuntime() throws {
        let selection = NativePublicationReleaseSelection.r125ApprovedR201
        func request(_ operation: String, selection: DocumentValue?) -> DocumentValue {
            var command: [String: DocumentValue] = [:]
            command["releaseSelection"] = selection
            return .object(["operation": .string(operation), "command": .object(command)])
        }
        let exact = request("export-static-native-original", selection: .string(selection.rawValue))
        XCTAssertEqual(try NativePublicationReleaseSelection.checked(in: exact, runtimeAdmitted: true), selection)
        XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(in: exact, runtimeAdmitted: false))
        for manifest in [nil,
                         "ff09c9e7f7ca92575f4b307c59fa42418a4195c03319e5d2a6ff2754489d0cb6",
                         "75895a172d843d6839309d0cc20febb7c210db0a35959e838cc8aa9dc507240a"] {
            XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest))
        }
        for invalid in [DocumentValue.null, .string("r125-approved-r201-v2")] {
            XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(
                in: request("export-static-native-original", selection: invalid), runtimeAdmitted: true))
        }
        for operation in ["export-static", "export-static-saved-head", "export-static-native-original-publication",
                          "private-exhibition-export-static", "artwork-original-image-static-approve"] {
            XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(
                in: request(operation, selection: .string(selection.rawValue)), runtimeAdmitted: true))
        }
        let noSelection = request("export-static-native-original", selection: nil)
        XCTAssertNil(try NativePublicationReleaseSelection.checked(in: noSelection, runtimeAdmitted: false))
        XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(in: noSelection, runtimeAdmitted: true))
    }

    func testApprovedReleaseSelectionBindsTheCurrentHeadAndOriginalImageOnTheWire() throws {
        let selection = NativePublicationReleaseSelection.r125ApprovedR201
        let etag = "\"studio-r201-2fac6adaf0ca67ae1b36d7924f2f916a4eed7e314a53e5b1262f204a54d002bc\""
        let requestID = "11111111-1111-4111-8111-111111111111"
        let fields: DocumentValue = .object([
            "artworkId": .string("d31d7f57-27a2-471a-a17a-eb2754f2e56b"), "artworkRevision": .integer(1),
            "importJobId": .string("41eb3e2d-06d1-48d2-b82c-557a6af58212")
        ])
        let image = try NativePublicationOriginalImage(fields)
        XCTAssertNoThrow(try selection.checkSource(etag: etag, originalImage: image))
        XCTAssertThrowsError(try selection.checkSource(etag: etag + " ", originalImage: image))
        XCTAssertThrowsError(try selection.checkSource(etag: etag, originalImage: nil))
        var otherImage = fields
        otherImage["artworkRevision"] = .integer(2)
        XCTAssertThrowsError(try selection.checkSource(etag: etag, originalImage: NativePublicationOriginalImage(otherImage)))

        func wire(_ selection: NativePublicationReleaseSelection?) throws -> DocumentValue {
            let input = NativePublicationPrepareRequest(requestId: requestID, originalImage: image, releaseSelection: selection)
            return try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(input))
        }
        XCTAssertEqual(try wire(selection), .object(["requestId": .string(requestID), "originalImage": fields,
                                                    "releaseSelection": .string("r125-approved-r201-v1")]))
        XCTAssertEqual(try wire(nil), .object(["requestId": .string(requestID), "originalImage": fields]))
    }

    func testNativeOriginalIntentBindsTheSavedSourceAndReplayID() throws {
        let draft = try NativeExhibitionDraft.new()
        let etag = "\"studio-r6-" + String(repeating: "a", count: 64) + "\""
        let manifest = String(repeating: "b", count: 64)
        let requestId = UUID().uuidString.lowercased()
        func originalPlan(etag: String, request: String) throws -> String {
            try nativePublicationPlanBinding(sourceExhibitionId: draft.id, etag: etag,
                runtimeManifestSha256: manifest, watermark: nil, publicationProfile: "native-original-v1",
                nativeOriginalRequestSHA256: request)
        }
        let plan = try originalPlan(etag: etag, request: String(repeating: "c", count: 64))
        let ordinary = try nativePublicationPlanBinding(sourceExhibitionId: draft.id, etag: etag,
            runtimeManifestSha256: manifest, watermark: nil, publicationProfile: "bounded-local-v1")
        XCTAssertNotEqual(plan, ordinary)
        let intent = try NativePublicationPending(nativeOriginalSource: draft, etag: etag, requestId: requestId, planBindingSha256: plan)
        XCTAssertEqual(intent, try NativePublicationPending(nativeOriginalSource: draft, etag: etag, requestId: requestId, planBindingSha256: plan))
        XCTAssertEqual(intent.snapshot, draft)
        XCTAssertEqual(intent.snapshotEtag, etag)
        XCTAssertEqual(intent.publishRequestId, requestId)
        XCTAssertNil(intent.capabilityPublicationId)
        XCTAssertThrowsError(try intent.requirePlanBinding(ordinary))
        let publicationID = UUID().uuidString.lowercased()
        let publication = NativePublication(publicationId: publicationID, publicUrl: "/p/" + publicationID,
            revisionSha256: String(repeating: "e", count: 64), publishedAt: "2026-11-08T00:00:00Z", status: "published")
        let completed = NativePublicationCompleted(pending: intent, publication: publication)
        XCTAssertThrowsError(try completed.refuseNativeOriginalExportReplay(intent)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "NATIVE_DETAIL_EXPORT_REPLAY_UNAVAILABLE")
        }
        let newer = etag.replacingOccurrences(of: "studio-r6-", with: "studio-r7-")
        let otherRequest = String(repeating: "d", count: 64)
        for otherEtag in [etag, newer] {
            let changed = try NativePublicationPending(nativeOriginalSource: draft, etag: otherEtag, requestId: requestId,
                planBindingSha256: originalPlan(etag: otherEtag, request: otherRequest))
            XCTAssertThrowsError(try completed.refuseNativeOriginalExportReplay(changed)) { error in
                XCTAssertEqual((error as? AuthoringFailure)?.code, "PUBLICATION_PLAN_CHANGED")
            }
        }
    }

    func testDetailTerminalRejectsInterruptedOrDifferentJob() {
        func status(_ state: String) -> NativeStaticExportStatus {
            .init(format: 1, jobId: "job", workspaceId: "workspace", tenantId: "tenant", principalId: "owner",
                publicationId: "publication", revisionSha256: "revision", output: "/candidate",
                state: state, receipt: nil, errorCode: nil, privateEditor: true)
        }
        XCTAssertFalse(status("interrupted").confirmsDetailTerminal(jobId: "job", publicationId: "publication", output: "/candidate"))
        XCTAssertFalse(status("completed").confirmsDetailTerminal(jobId: "other", publicationId: "publication", output: "/candidate"))
        XCTAssertFalse(status("completed").confirmsDetailTerminal(jobId: "job", publicationId: "publication", output: "/missing"))
        XCTAssertTrue(status("cancelled").confirmsDetailTerminal(jobId: "job", publicationId: "publication", output: "/candidate"))
    }

    func testNativeOriginalClaimSurvivesLaterPublicationAndRejectsColdReplay() throws {
        let draft = try NativeExhibitionDraft.new()
        let etag = "\"studio-r6-" + String(repeating: "a", count: 64) + "\""
        func intent(id: String = UUID().uuidString, plan: Character) throws -> NativePublicationPending {
            try .init(nativeOriginalSource: draft, etag: etag, requestId: id,
                      planBindingSha256: String(repeating: plan, count: 64))
        }
        let a = try intent(plan: "a"), b = try intent(plan: "b")
        let publicId = UUID().uuidString
        let publication = NativePublication(publicationId: publicId, publicUrl: "/p/" + publicId,
            revisionSha256: String(repeating: "c", count: 64), publishedAt: "2026-11-08T00:00:00Z", status: "published")
        var state = NativePublicationState(format: 1, revision: 1, workspaceId: UUID().uuidString,
            tenantId: UUID().uuidString, principalId: UUID().uuidString, pending: nil, completed: nil)
        for operation in [a, b] {
            try state.stageNativeOriginal(operation)
            state.pending = nil
            state.completed = NativePublicationCompleted(pending: operation, publication: publication)
            try state.claimNativeOriginalExport(operation)
        }
        var cold = try JSONDecoder().decode(NativePublicationState.self, from: JSONEncoder().encode(state))
        XCTAssertEqual(cold.completed?.publishRequestId, b.publishRequestId)
        let changedDestination = try intent(id: a.publishRequestId, plan: "d")
        XCTAssertThrowsError(try cold.stageNativeOriginal(changedDestination)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "PUBLICATION_PLAN_CHANGED")
        }
        XCTAssertThrowsError(try cold.stageNativeOriginal(a)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "NATIVE_DETAIL_EXPORT_REPLAY_UNAVAILABLE")
        }
        XCTAssertEqual(cold, state)
    }

    func testPublicationCopyRetainsGeometryRightsAndOpaqueDataWhileChangingOnlyDocumentIdentity() throws {
        let authoredTitle = String(repeating: "가", count: 500) + " · 공개용 저장본"
        var source = try NativeExhibitionDraft.new(title: authoredTitle)
        var candidate = source.candidate
        candidate["extensions"] = .object(["com.example/retained":.object(["note":.string("원문 보존")])])
        try source.replaceCandidate(candidate)
        let original = source
        let pending = try NativePublicationPending(source: source, etag: "\"studio-r2-" + String(repeating: "a", count: 64) + "\"")
        let copy = pending.snapshot
        XCTAssertNotEqual(copy.id, source.id); XCTAssertNotEqual(copy.document["id"], source.document["id"])
        XCTAssertNotEqual(copy.candidate["revisionId"], source.candidate["revisionId"])
        XCTAssertEqual(copy.document["editVersion"].integer, 1)
        for key in ["rooms","surfaces","openings","placements","artworks","lights","accessibility","extensions","mediaAssets","audioZones","annotations","scripts"] { XCTAssertEqual(copy.candidate[key],source.candidate[key]) }
        XCTAssertEqual(copy.title, authoredTitle); XCTAssertEqual(source, original)
        XCTAssertNil(pending.snapshotEtag)
        let decoded = try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(pending)); XCTAssertEqual(decoded, pending)
        XCTAssertTrue(try source.copyForEditing().title.hasSuffix("편집 사본"))
        XCTAssertThrowsError(try NativePublicationPending(source: source, etag: "untrusted"))
    }
    func testPreparedPublicationNeverAcceptsAnExternalOrChangedLocalURL() throws {
        let id = UUID().uuidString.lowercased(), hash = String(repeating: "a", count: 64)
        let valid = NativePublication(publicationId: id, publicUrl: "/p/" + id, revisionSha256: hash, publishedAt: "2026-10-23T00:00:00.000Z", status: "published")
        XCTAssertEqual(try valid.validated(), valid)
        for url in ["https://outside.invalid/p/" + id, "/p/" + UUID().uuidString, "/p/" + id + "?token=untrusted"] {
            XCTAssertThrowsError(try NativePublication(publicationId: id, publicUrl: url, revisionSha256: hash, publishedAt: valid.publishedAt, status: valid.status).validated())
        }
    }
    func testPendingComparisonAcceptsOnlyExactInitialSnapshotWithoutReplacingRequestIdentity() throws {
        var source = try NativeExhibitionDraft.new(title: "비교 원본"), candidate = source.candidate
        candidate["extensions"] = .object(["com.example/precise": .number(0.9559733532492861)])
        try source.replaceCandidate(candidate)
        let pending = try NativePublicationPending(source: source, etag: "\"studio-r7-" + String(repeating: "a", count: 64) + "\"", planBindingSha256: String(repeating: "c", count: 64))
        let etag = "\"studio-r1-" + String(repeating: "b", count: 64) + "\""
        let saved = NativeStudioResponse(revision: 1, etag: etag, draft: pending.snapshot)
        let accepted = try pending.confirmingSavedSnapshot(saved)
        var expected = pending; expected.snapshotEtag = etag
        XCTAssertEqual(accepted, expected, "Only the acknowledged ETag changes; no new draft/request/plan identity")
        XCTAssertEqual(try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(accepted)), expected)
        XCTAssertNil(pending.snapshotEtag, "The original uncertain request remains untouched")
        for invalid in [NativeStudioResponse(revision: 2, etag: etag, draft: pending.snapshot),
                        NativeStudioResponse(revision: 1, etag: "untrusted", draft: pending.snapshot),
                        NativeStudioResponse(revision: 1, etag: "\"studio-r2-" + String(repeating: "b", count: 64) + "\"", draft: pending.snapshot),
                        NativeStudioResponse(revision: 1, etag: etag, draft: source)] {
            XCTAssertThrowsError(try pending.confirmingSavedSnapshot(invalid))
        }
        var changed = pending.snapshot.document
        changed["candidate"]["extensions"]["com.example/precise"] = .number(0.955973353249286)
        XCTAssertThrowsError(try pending.confirmingSavedSnapshot(NativeStudioResponse(revision: 1, etag: etag, draft: NativeExhibitionDraft(document: changed))))
        XCTAssertThrowsError(try accepted.confirmingSavedSnapshot(saved), "An existing acknowledgement cannot be overwritten")
    }
    func testLegacySavedSnapshotCanAcquireOnlyItsFirstBinding() throws {
        let source = try NativeExhibitionDraft.new()
        var pending = try NativePublicationPending(source: source, etag: "\"studio-r2-" + String(repeating: "a", count: 64) + "\"")
        let etag = "\"studio-r1-" + String(repeating: "b", count: 64) + "\""
        let saved = NativeStudioResponse(revision: 1, etag: etag, draft: pending.snapshot)
        let binding = String(repeating: "c", count: 64)
        XCTAssertThrowsError(try pending.bindingSavedSnapshot(saved, to: binding))
        pending.snapshotEtag = etag
        let bound = try pending.bindingSavedSnapshot(saved, to: binding)
        XCTAssertEqual(bound.planBindingSha256, binding)
        XCTAssertEqual(bound.snapshot, pending.snapshot)
        XCTAssertEqual(bound.snapshotEtag, pending.snapshotEtag)
        XCTAssertEqual(bound.saveRequestId, pending.saveRequestId)
        XCTAssertEqual(bound.publishRequestId, pending.publishRequestId)
        XCTAssertThrowsError(try bound.bindingSavedSnapshot(saved, to: binding))
        XCTAssertThrowsError(try pending.bindingSavedSnapshot(
            NativeStudioResponse(revision: 1, etag: etag, draft: source), to: binding))
    }
    func testPendingSnapshotBindingRejectsLateGenerationScopeRevisionAndRequestChanges() throws {
        let pending = try NativePublicationPending(source: NativeExhibitionDraft.new(), etag: "\"studio-r7-" + String(repeating: "a", count: 64) + "\"")
        let state = NativePublicationState(format: 1, revision: 7, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), pending: pending, completed: nil)
        let generation = UUID(), binding = try NativePendingPublicationSnapshotBinding(state, generation: generation)
        let etag = "\"studio-r1-" + String(repeating: "b", count: 64) + "\""
        let response = NativeStudioResponse(revision: 1, etag: etag, draft: pending.snapshot)
        var expected = state; expected.pending?.snapshotEtag = etag
        XCTAssertEqual(try binding.accepting(response, in: state, generation: generation), expected)
        XCTAssertThrowsError(try binding.accepting(response, in: state, generation: UUID()))
        var later = state; later.revision += 1
        XCTAssertThrowsError(try binding.accepting(response, in: later, generation: generation))
        later = state; later.pending = nil
        XCTAssertThrowsError(try binding.accepting(response, in: later, generation: generation))
        for key in ["workspaceId", "tenantId", "principalId"] {
            var changed = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(state))
            changed[key] = .string(UUID().uuidString.lowercased())
            let foreign = try JSONDecoder().decode(NativePublicationState.self, from: JSONEncoder().encode(changed))
            XCTAssertThrowsError(try binding.accepting(response, in: foreign, generation: generation))
        }
        for key in ["saveRequestId", "publishRequestId"] {
            var changed = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(state))
            changed["pending"][key] = .string(UUID().uuidString.lowercased())
            let foreign = try JSONDecoder().decode(NativePublicationState.self, from: JSONEncoder().encode(changed))
            XCTAssertThrowsError(try binding.accepting(response, in: foreign, generation: generation))
        }
    }
    func testReusableDraftIntentRetainsExactIdentityAndRequestAcrossRestart() throws {
        let draft = try NativeExhibitionDraft.new(), etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
        let pending = try NativePublicationPending(reusableSource: draft, etag: etag, capabilityPublicationId: UUID().uuidString.lowercased())
        XCTAssertEqual(pending.mode, "reusable-draft-v1")
        XCTAssertEqual(pending.snapshot, draft)
        XCTAssertEqual(pending.sourceExhibitionId, draft.id)
        XCTAssertEqual(pending.snapshotEtag, etag)
        let round = try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(pending))
        XCTAssertEqual(round, pending)
        let id = UUID().uuidString.lowercased()
        let withdrawn = NativePublication(publicationId: id, publicUrl: "/p/" + id, revisionSha256: String(repeating: "c", count: 64), publishedAt: "2026-10-30T00:00:00Z", status: "unpublished")
        let completed = NativePublicationCompleted(pending: round, publication: withdrawn)
        XCTAssertEqual(completed.mode, "reusable-draft-v1")
        XCTAssertEqual(completed.sourceExhibitionId, completed.snapshotExhibitionId)
        XCTAssertFalse(completed.isReusable(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: nil))
        XCTAssertThrowsError(try pending.confirmingSavedSnapshot(NativeStudioResponse(revision: 1, etag: etag, draft: draft)))
    }

}
