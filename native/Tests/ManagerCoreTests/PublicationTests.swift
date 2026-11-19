// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore
final class PublicationTests: XCTestCase {
    func testR203OrdinarySuccessorKeepsVoiceAndSelectedExportSeparateFromFloorAndFrames() throws {
        let manifest = "efd30f7b66ee0babed897af289eecc2df6a1a16275ce4373ec69db8516180e29"
        let predecessor = AuthoringAPI.r203CorrectedReleaseManifest
        #if DEBUG && os(macOS)
        XCTAssertTrue(AuthoringAPI.supportsR203OrdinarySuccessor(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
        XCTAssertTrue(NativeFloorRuntime.supports(manifestSHA256: manifest, ready: true))
        XCTAssertTrue(NativeFloorAudioRuntime.supportsQuietOak(manifestSHA256: manifest, ready: true))
        XCTAssertFalse(NativeFloorAudioRuntime.supportsQuietOak(manifestSHA256: manifest, ready: false))
        XCTAssertFalse(NativeFloorAudioRuntime.supports(manifestSHA256: manifest, ready: true))
        for operation in ["read", "save", "reopen-saved", "floor-sound-status", "floor-quiet-oak-footsteps-apply",
                          "floor-entrance-stone-footsteps-apply", "workspace-furnishing-list", "workspace-furnishing-read", "workspace-frame-lineage-status",
                          "workspace-frame-attach", "workspace-frame-image-fit-prepare", "workspace-frame-image-fit-apply",
                          "workspace-frame-image-fit-cancel", "saved-head-static-readiness", "export-oex", "export-static-saved-head"] {
            XCTAssertNoThrow(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest), operation)
        }
        for operation in ["placement-voice-apply", "applied-draft-save-resume", "room-audio-approve", "exhibition-new",
                          "floor-appearance-apply", "workspace-frame-detach",
                          "workspace-frame-lineage-prepare", "workspace-furnishing-update", "workspace-material-list",
                          "private-exhibition-export-static", "export-static", "export-static-native-original",
                          "export-static-native-original-publication"] {
            XCTAssertThrowsError(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest), operation)
        }
        #endif
        XCTAssertFalse(AuthoringAPI.supportsR203OrdinarySuccessor(manifest: predecessor))
        XCTAssertFalse(AuthoringAPI.supportsR203OrdinarySuccessor(manifest: manifest.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsR201OrdinarySuccessor(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsOrdinaryNativePublicationRuntime(manifest: manifest))
    }

    func testEntranceStoneRequiresTheExactOrdinaryRuntime() {
        let operation = "floor-entrance-stone-footsteps-apply"
        let manifest = "efd30f7b66ee0babed897af289eecc2df6a1a16275ce4373ec69db8516180e29"
        let otherRuntimes: [String?] = [
            nil,
            AuthoringAPI.r203CorrectedReleaseManifest,
            AuthoringAPI.r125ApprovedReleaseManifest,
            "ae894e6fe31a5e751406610e35e0e6a1e67033a9cf7527c588aaf26019b9564a",
            manifest.uppercased(),
            "0" + manifest.dropFirst(),
        ]
        for other in otherRuntimes {
            XCTAssertThrowsError(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: other))
        }
        #if DEBUG && os(macOS)
        XCTAssertNoThrow(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest))
        #else
        XCTAssertThrowsError(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest))
        #endif
    }

    func testOrdinarySuccessorVoiceScopeRejectsOtherEditsAndSelectedOutput() throws {
        let manifest = "ae894e6fe31a5e751406610e35e0e6a1e67033a9cf7527c588aaf26019b9564a"
        let predecessor = "b109a50804cd358951f44a1f0bb0ec78ea11c15bc67bb3e2f8f7404701e66739"
        #if DEBUG && os(macOS)
        XCTAssertTrue(AuthoringAPI.supportsR201OrdinarySuccessor(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        // v11 is required to retain the existing ceiling rail rows while editing frames.
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
        XCTAssertTrue(NativeFloorRuntime.supports(manifestSHA256: manifest, ready: true))
        XCTAssertTrue(NativeFloorAudioRuntime.supportsQuietOak(manifestSHA256: manifest, ready: true))
        XCTAssertFalse(NativeFloorAudioRuntime.supports(manifestSHA256: manifest, ready: true))
        XCTAssertFalse(NativeFloorAudioRuntime.supportsQuietOak(manifestSHA256: manifest, ready: false))
        for operation in ["read", "reopen-saved", "room-audio-status", "placement-voice-apply", "applied-draft-save-resume"] {
            XCTAssertNoThrow(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest))
        }
        for operation in ["save", "export-oex", "export-static-saved-head", "private-exhibition-export-static", "export-static",
                          "export-static-native-original", "export-static-native-original-publication", "room-audio-approve",
                          "room-audio-intake", "room-audio-apply", "workspace-frame-attach", "workspace-frame-image-fit-apply",
                          "floor-quiet-oak-footsteps-apply", "floor-entrance-stone-footsteps-apply", "exhibition-new"] {
            XCTAssertThrowsError(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest))
        }
        #endif
        XCTAssertFalse(AuthoringAPI.supportsR201OrdinarySuccessor(manifest: predecessor))
        XCTAssertFalse(AuthoringAPI.supportsR201OrdinarySuccessor(manifest: manifest.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest))
        XCTAssertFalse(NativeFloorAudioRuntime.supportsQuietOak(manifestSHA256: predecessor, ready: true))
        let request: DocumentValue = .object([
            "operation": .string("export-static-native-original"),
            "command": .object(["releaseSelection": .string("r125-approved-r201-v1")]),
        ])
        XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(in: request,
            runtimeAdmitted: AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest)))
    }

    func testVoiceCorrectionRequiresTheExactPriorBindingExistingWAVAndUnchangedScript() throws {
        let place = "10000000-0000-4000-8000-000000000021"
        let old = "10000000-0000-4000-8000-000000000022"
        let correction = "10000000-0000-4000-8000-000000000023"
        let sha = String(repeating: "a", count: 64)
        let script = "Synthetic Korean narration"
        let media: DocumentValue = .object(["id": .string(correction), "mime": .string("audio/wav"), "sha256": .string(sha)])
        let binding: DocumentValue = .object(["placementId": .string(place), "assetId": .string(old),
            "locale": .string("ko"), "transcript": .string(script)])
        let candidate: DocumentValue = .object(["mediaAssets": .array([media]), "extensions": .object([
            ConcreteFootsteps.experienceNamespace: .object(["voices": .array([binding])])
        ])])
        let command: DocumentValue = .object(["placementId": .string(place), "audioId": .string(correction),
            "locale": .string("ko"), "transcript": .string(script), "expectedMediaAsset": media])
        let pin = try NativeAutomationSession.voiceCorrectionCommitment(placementId: place, priorAudioId: old,
            audioId: correction, audioSHA256: sha, transcript: script)
        XCTAssertNoThrow(try NativeAutomationSession.checkVoiceCorrection(command, candidate: candidate, allowedCommitments: [pin]))
        XCTAssertThrowsError(try NativeAutomationSession.checkVoiceCorrection(command, candidate: candidate))
        for (key, wrong) in [
            ("placementId", DocumentValue.string(old)), ("audioId", .string(old)), ("locale", .string("en")),
            ("transcript", .string(script + " changed")), ("expectedMediaAsset", .null)
        ] {
            var changed = command
            changed[key] = wrong
            XCTAssertThrowsError(try NativeAutomationSession.checkVoiceCorrection(changed, candidate: candidate, allowedCommitments: [pin]))
        }
        var next = candidate
        var edited = binding
        edited["assetId"] = .string(correction)
        next["extensions"][ConcreteFootsteps.experienceNamespace]["voices"] = .array([edited])
        XCTAssertThrowsError(try NativeAutomationSession.checkVoiceCorrection(command, candidate: next, allowedCommitments: [pin]))
    }

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
        let admitted = "b109a50804cd358951f44a1f0bb0ec78ea11c15bc67bb3e2f8f7404701e66739"
        #if DEBUG && os(macOS)
        XCTAssertTrue(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: admitted))
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: admitted))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: admitted))
        XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: admitted))
        #endif
        for manifest in [nil, admitted.uppercased(), "0" + String(admitted.dropFirst()),
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
        for operation in ["export-static", "export-static-saved-head", "export-static-native-original-publication",
                          "private-exhibition-export-static", "save", "room-audio-approve", "export-oex"] {
            XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(
                in: request(operation, selection: nil), runtimeAdmitted: true))
        }
        for operation in ["read", "room-audio-status"] {
            XCTAssertNil(try NativePublicationReleaseSelection.checked(
                in: request(operation, selection: nil), runtimeAdmitted: true))
        }
        let noSelection = request("export-static-native-original", selection: nil)
        XCTAssertNil(try NativePublicationReleaseSelection.checked(in: noSelection, runtimeAdmitted: false))
        XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(in: noSelection, runtimeAdmitted: true))
    }

    func testR209WheatOrdinarySuccessorKeepsSelectedAndIntakeOperationsClosed() throws {
        let manifest = AuthoringAPI.r209WheatOrdinaryManifest
        let predecessor = AuthoringAPI.r209PointReleaseManifest
        #if DEBUG && os(macOS)
        XCTAssertTrue(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
        let api = try AuthoringAPI(port: 58000, tenantId: UUID().uuidString, nativeRuntimeManifestSHA256: manifest)
        XCTAssertTrue(api.workspaceFurnishingRuntimeQualified)
        for operation in ["read", "reopen-saved", "save", "workspace-furnishing-list", "workspace-furnishing-read",
                          "workspace-frame-attach", "workspace-frame-image-fit-prepare", "workspace-frame-image-fit-apply",
                          "workspace-frame-image-fit-cancel", "saved-head-static-readiness", "export-oex", "export-static-saved-head"] {
            XCTAssertNoThrow(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest))
        }
        for operation in ["workspace-furnishing-intake-create", "workspace-furnishing-intake-upload", "workspace-frame-detach",
                          "light-prepare", "floor-entrance-stone-footsteps-apply", "export-static-native-original", "export-static"] {
            XCTAssertThrowsError(try NativeAutomationSession.checkOrdinarySuccessorOperation(operation, manifest: manifest))
        }
        let placement = "89ec602d-79ff-4851-9ead-85bdd4a382cf"
        let oak = "a96b4942c2cd10b25fe1b33001443e1824fc125bc258f28a68547d38d8c54362"
        XCTAssertNoThrow(try NativeAutomationSession.checkR209WheatFrameTarget(placementId: placement, modelSHA256: oak, manifest: manifest))
        XCTAssertThrowsError(try NativeAutomationSession.checkR209WheatFrameTarget(placementId: UUID().uuidString, modelSHA256: oak, manifest: manifest))
        XCTAssertThrowsError(try NativeAutomationSession.checkR209WheatFrameTarget(placementId: placement, modelSHA256: nil, manifest: manifest))
        #endif
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: predecessor))
        XCTAssertFalse(AuthoringAPI.supportsR209WheatOrdinarySuccessor(manifest: manifest.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest))
    }

    func testR209SelectedSuccessorsOnlyAdmitTheExistingSelectedCommand() throws {
        try checkSelectedSuccessor(manifest: AuthoringAPI.r209FloorReleaseManifest)
        try checkSelectedSuccessor(manifest: AuthoringAPI.r209PointReleaseManifest)
        try checkSelectedSuccessor(manifest: AuthoringAPI.r209ArtworkSpotsReleaseManifest)
        try checkSelectedSuccessor(manifest: AuthoringAPI.r209GreekSpotReleaseManifest)
        try checkSelectedSuccessor(manifest: AuthoringAPI.r209TransitionSpotsReleaseManifest)
        try checkSelectedSuccessor(manifest: AuthoringAPI.r209GreekCurrentReleaseManifest)
    }

    private func checkSelectedSuccessor(manifest: String) throws {
        let selection = NativePublicationReleaseSelection.r125ApprovedR201
        let admitted = AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest)
        let selected: DocumentValue = .object(["operation": .string("export-static-native-original"),
            "command": .object(["releaseSelection": .string(selection.rawValue)])])
        #if DEBUG && os(macOS)
        XCTAssertTrue(admitted)
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        XCTAssertEqual(try NativePublicationReleaseSelection.checked(in: selected, runtimeAdmitted: admitted), selection)
        for operation in ["save", "workspace-frame-attach", "floor-entrance-stone-footsteps-apply",
                          "export-oex", "export-static-saved-head", "export-static-native-original",
                          "export-static-native-original-publication"] {
            XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(
                in: .object(["operation": .string(operation)]), runtimeAdmitted: admitted), operation)
        }
        #else
        XCTAssertFalse(admitted)
        XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(in: selected, runtimeAdmitted: admitted))
        #endif
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        let differentFirstDigit = manifest.first == "0" ? "1" : "0"
        for invalid in [nil, manifest.uppercased(), differentFirstDigit + manifest.dropFirst()] {
            XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: invalid))
        }
        let image = try NativePublicationOriginalImage(.object([
            "artworkId": .string("10000000-0000-4000-8000-000000000016"), "artworkRevision": .integer(1),
            "importJobId": .string("10000000-0000-4000-8000-000000000017")
        ]))
        XCTAssertNotEqual(try selection.sourceCommitment(etag: "synthetic", originalImage: image),
            try selection.sourceCommitment(etag: "synthetic", originalImage: image, runtimeManifest: manifest))
        XCTAssertThrowsError(try selection.checkSource(etag: "synthetic", originalImage: image, manifest: manifest))
        XCTAssertThrowsError(try selection.checkSource(etag: "synthetic", originalImage: nil, manifest: manifest))
    }

    func testSameHeadSelectedSuccessorsRejectCompletedPredecessorRequestAndKeepItsReceipt() throws {
        try checkSelectedSuccessorReplay(previousManifest: AuthoringAPI.r209BrassReleaseManifest,
                                         nextManifest: AuthoringAPI.r209FloorReleaseManifest)
        try checkSelectedSuccessorReplay(previousManifest: AuthoringAPI.r209FloorReleaseManifest,
                                         nextManifest: AuthoringAPI.r209PointReleaseManifest)
        try checkSelectedSuccessorReplay(previousManifest: AuthoringAPI.r209PointReleaseManifest,
                                         nextManifest: AuthoringAPI.r209ArtworkSpotsReleaseManifest)
        try checkSelectedSuccessorReplay(previousManifest: AuthoringAPI.r209ArtworkSpotsReleaseManifest,
                                         nextManifest: AuthoringAPI.r209GreekSpotReleaseManifest)
        try checkSelectedSuccessorReplay(previousManifest: AuthoringAPI.r209GreekSpotReleaseManifest,
                                         nextManifest: AuthoringAPI.r209TransitionSpotsReleaseManifest)
        try checkSelectedSuccessorReplay(previousManifest: AuthoringAPI.r209TransitionSpotsReleaseManifest,
                                         nextManifest: AuthoringAPI.r209GreekCurrentReleaseManifest)
    }

    private func checkSelectedSuccessorReplay(previousManifest: String, nextManifest: String) throws {
        let owner = UUID().uuidString.lowercased()
        let draft = try NativeExhibitionDraft.new(title: "Saved source")
        let etag = "\"studio-r209-" + String(repeating: "a", count: 64) + "\""
        let source = NativeDocumentState(format: 1, revision: 584, workspaceId: owner, tenantId: owner, principalId: owner,
            draft: draft, etag: etag, savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        var state = NativePublicationState(format: 1, revision: 1, workspaceId: owner, tenantId: owner, principalId: owner,
            pending: nil, completed: nil)
        let request = UUID().uuidString.lowercased()
        let digest = String(repeating: "b", count: 64)
        let prior = try state.nativeOriginalIntent(in: source, runtimeManifest: previousManifest,
            requestId: request, requestSHA256: digest)
        try state.stageNativeOriginal(prior)
        let publicationID = UUID().uuidString.lowercased()
        let published = NativePublication(publicationId: publicationID, publicUrl: "/p/" + publicationID,
            revisionSha256: String(repeating: "c", count: 64), publishedAt: "2026-11-19T00:00:00Z", status: "published")
        state.pending = nil
        state.completed = NativePublicationCompleted(pending: prior, publication: published)
        try state.claimNativeOriginalExport(prior)
        let retained = state
        let replay = try state.nativeOriginalIntent(in: source, runtimeManifest: nextManifest,
            requestId: request, requestSHA256: digest)
        XCTAssertNotEqual(prior.planBindingSha256, replay.planBindingSha256)
        XCTAssertThrowsError(try XCTUnwrap(state.completed).refuseNativeOriginalExportReplay(replay)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "PUBLICATION_PLAN_CHANGED")
        }
        XCTAssertThrowsError(try state.stageNativeOriginal(replay)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "PUBLICATION_PLAN_CHANGED")
        }
        XCTAssertEqual(state, retained)
        let fresh = try state.nativeOriginalIntent(in: source, runtimeManifest: nextManifest,
            requestId: UUID().uuidString.lowercased(), requestSHA256: digest)
        try state.stageNativeOriginal(fresh)
        XCTAssertEqual(state.pending, fresh)
        XCTAssertEqual(state.completed, retained.completed)
        XCTAssertEqual(state.nativeOriginalAttempts?[request], retained.nativeOriginalAttempts?[request])
    }

    func testBrassReleaseUsesTheExistingSelectorAndKeepsOrdinaryEditingClosed() throws {
        let manifest = AuthoringAPI.r209BrassReleaseManifest
        let selection = NativePublicationReleaseSelection.r125ApprovedR201
        let admitted = AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest)
        let selected: DocumentValue = .object(["operation": .string("export-static-native-original"),
            "command": .object(["releaseSelection": .string(selection.rawValue)])])
        #if DEBUG && os(macOS)
        XCTAssertTrue(admitted)
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        XCTAssertEqual(try NativePublicationReleaseSelection.checked(in: selected, runtimeAdmitted: admitted), selection)
        for operation in ["save", "placement-voice-apply", "floor-entrance-stone-footsteps-apply",
                          "workspace-frame-attach", "export-oex", "export-static-saved-head",
                          "private-exhibition-export-static", "export-static-native-original-publication",
                          "export-static-native-original"] {
            XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(
                in: .object(["operation": .string(operation)]), runtimeAdmitted: admitted), operation)
        }
        #else
        XCTAssertFalse(admitted)
        XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(in: selected, runtimeAdmitted: admitted))
        #endif
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsR203OrdinarySuccessor(manifest: manifest))
        for denied in [nil, manifest.uppercased(), "0" + manifest.dropFirst()] {
            XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: denied))
        }
        let image = try NativePublicationOriginalImage(.object([
            "artworkId": .string("10000000-0000-4000-8000-000000000016"), "artworkRevision": .integer(1),
            "importJobId": .string("10000000-0000-4000-8000-000000000017")
        ]))
        XCTAssertThrowsError(try selection.checkSource(etag: "synthetic", originalImage: image, manifest: manifest))
        XCTAssertThrowsError(try selection.checkSource(etag: "synthetic", originalImage: nil, manifest: manifest))
    }

    func testCorrectedReleaseKeepsTheExistingSelectorAndOnlySelectedExport() throws {
        let manifest = AuthoringAPI.r203CorrectedReleaseManifest
        let selection = NativePublicationReleaseSelection.r125ApprovedR201
        func request(_ operation: String, selected: Bool) -> DocumentValue {
            let fields: [String: DocumentValue] = selected ? ["releaseSelection": .string(selection.rawValue)] : [:]
            return .object(["operation": .string(operation), "command": .object(fields)])
        }
        #if DEBUG && os(macOS)
        let admitted = AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: manifest)
        XCTAssertTrue(admitted)
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsR201OrdinarySuccessor(manifest: manifest))
        XCTAssertEqual(try NativePublicationReleaseSelection.checked(
            in: request("export-static-native-original", selected: true), runtimeAdmitted: admitted), selection)
        for operation in ["save", "placement-voice-apply", "export-oex", "export-static-saved-head",
                          "private-exhibition-export-static", "export-static-native-original-publication"] {
            XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(
                in: request(operation, selected: false), runtimeAdmitted: admitted))
        }
        XCTAssertThrowsError(try NativePublicationReleaseSelection.checked(
            in: request("export-static-native-original", selected: false), runtimeAdmitted: admitted))
        #endif
        for denied in [nil, manifest.uppercased(), "a" + String(manifest.dropFirst())] {
            XCTAssertFalse(AuthoringAPI.supportsR125ApprovedReleaseRuntime(manifest: denied))
        }
        let image = try NativePublicationOriginalImage(.object([
            "artworkId": .string("10000000-0000-4000-8000-000000000016"), "artworkRevision": .integer(1),
            "importJobId": .string("10000000-0000-4000-8000-000000000017")
        ]))
        XCTAssertThrowsError(try selection.checkSource(etag: "synthetic", originalImage: image, manifest: manifest))
    }

    func testApprovedReleaseSelectionBindsTheCurrentHeadAndOriginalImageOnTheWire() throws {
        let selection = NativePublicationReleaseSelection.r125ApprovedR201
        let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\""
        let requestID = "10000000-0000-4000-8000-000000000013"
        let fields: DocumentValue = .object([
            "artworkId": .string("10000000-0000-4000-8000-000000000011"), "artworkRevision": .integer(1),
            "importJobId": .string("10000000-0000-4000-8000-000000000012")
        ])
        let image = try NativePublicationOriginalImage(fields)
        let commit = try selection.sourceCommitment(etag: etag, originalImage: image)
        XCTAssertEqual(commit, "db173da039940c6486f58f43a23b79830953cd2c6525b31d97166cdb248a3ab7")
        XCTAssertNotEqual(try selection.sourceCommitment(etag: etag + " ", originalImage: image), commit)
        for (field, replacement) in [
            ("artworkId", DocumentValue.string("10000000-0000-4000-8000-000000000014")),
            ("artworkRevision", .integer(2)),
            ("importJobId", .string("10000000-0000-4000-8000-000000000015")),
        ] {
            var other = fields
            other[field] = replacement
            let changedImage = try NativePublicationOriginalImage(other)
            XCTAssertNotEqual(try selection.sourceCommitment(etag: etag, originalImage: changedImage), commit)
        }
        XCTAssertThrowsError(try selection.checkSource(etag: etag, originalImage: image,
            manifest: AuthoringAPI.r125ApprovedReleaseManifest))
        XCTAssertThrowsError(try selection.checkSource(etag: etag, originalImage: nil,
            manifest: AuthoringAPI.r125ApprovedReleaseManifest))

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
