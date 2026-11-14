// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import WebKit
import CryptoKit
import ImageIO
import ManagerCore

struct NativeEntrancePreviewInput {
    let action: EntrancePreviewAction
    let device: EntrancePreviewDevice
    let scenario: EntrancePreviewScenario
    let reducedMotion: Bool
    let theme: CoverEntryTheme?
    let locale: String?
    let draft: NativeExhibitionDraft
}
struct NativeEntrancePreviewOutput {
    let request: ViewportEntrancePreviewRequest
    let png: Data?
}

struct NativeDisplayReadoutInput {
    let draft: NativeExhibitionDraft
    let selection: ViewportSelection?
    let view: String
    let cameraSequence: Int
    let transparent: Bool
    let presentation: ViewportPresentation
}

enum ViewportSceneProgress: Equatable {
    case preparing
    case artworks(processed: Int, total: Int)

    static func reported(ready: Bool, failed: Bool, loadedCount: Int, failedCount: Int, total: Int) -> Self? {
        guard !failed else { return nil }
        guard ready, total >= 0, loadedCount >= 0, loadedCount <= total,
              failedCount >= 0, failedCount <= total - loadedCount else { return .preparing }
        let processed = loadedCount + failedCount
        return processed < total ? .artworks(processed: processed, total: total) : nil
    }
}

@MainActor
final class NativeViewportController: ObservableObject {
    // Synchronously updated by native navigation, before a delayed WK frame or
    // queued candidate callback can act on the former editing purpose.
    private(set) var editingContextGeneration = UUID()
    @Published var editingContextActive = true {
        didSet { if oldValue != editingContextActive { editingContextGeneration = UUID(); displayReadout = "" } }
    }
    @Published var connected = false
    /// Restriction from an acknowledged frame; retained until a newer exact
    /// frame explicitly clears it. It never grants server authority.
    @Published fileprivate(set) var boundedReadOnly = false
    fileprivate var acknowledgedBoundedReadOnly: Bool?
    fileprivate var acknowledgedBoundedEdit: NativeBoundedEditCapability?
    @Published fileprivate(set) var planSupported = false
    @Published fileprivate(set) var thumbnailSupported = false
    @Published fileprivate(set) var renderedThumbnailSupported = false
    @Published fileprivate(set) var furnishingThumbnailSupported = false
    @Published fileprivate(set) var sceneSnapshotSupported = false
    @Published fileprivate(set) var sceneSnapshotInspectionSupported = false
    @Published fileprivate(set) var findSelectionSupported = false
    @Published fileprivate(set) var catalogDropSupported = false
    @Published fileprivate(set) var placementSupported = false
    @Published fileprivate(set) var furnishingVisualsSupported = false
    @Published fileprivate(set) var sharedFurnishingVisualsSupported = false
    @Published fileprivate(set) var plinthEnvelopeAlignSupported = false
    @Published fileprivate(set) var spaceSettingsSupported = false
    @Published fileprivate(set) var viewingPlanSupported = false
    @Published fileprivate(set) var lightInspectorSupported = false
    @Published fileprivate(set) var lightFixtureSupported = false
    @Published fileprivate(set) var lightObjectSupported = false
    @Published fileprivate(set) var roomBaseLightSupported = false
    @Published fileprivate(set) var wallFinishSupported = false
    @Published fileprivate(set) var benchInspectorSupported = false
    @Published fileprivate(set) var plaqueInspectorSupported = false
    @Published fileprivate(set) var surfaceMaterialTexturesSupported = false
    @Published fileprivate(set) var entrancePreviewSupported = false
    fileprivate var acknowledgedFrame: ViewportFrame?
    @Published fileprivate(set) var displayReadout = ""
    @Published private(set) var readingDisplay = false
    fileprivate var displayReadoutAction: ((NativeDisplayReadoutInput) async throws -> String)?
    func readDisplayStatus(_ input: NativeDisplayReadoutInput) async {
        guard connected, editingContextActive, !readingDisplay, let displayReadoutAction else { return }
        let context = editingContextGeneration, epoch = owner, accepted = acceptedSequence
        readingDisplay = true; displayReadout = ""
        defer { readingDisplay = false }
        do {
            let value = try await displayReadoutAction(input)
            guard connected, editingContextActive, editingContextGeneration == context, owner == epoch,
                  acceptedSequence == accepted, acknowledgedFrame?.draft == input.draft,
                  acknowledgedFrame?.selection == input.selection else { return }
            displayReadout = value
        } catch {
            guard editingContextActive, editingContextGeneration == context, owner == epoch else { return }
            diagnose("DISPLAY_READ", error)
        }
    }
    /// An older or pending frame cannot clear a saved draft's restriction.
    func acknowledges(draft: NativeExhibitionDraft?, tenantId: String?, principalId: String?, boundedEdit: NativeBoundedEditCapability? = nil) -> Bool {
        guard connected, let frame = acknowledgedFrame, let owner, let draft,
              let tenantId, let principalId else { return false }
        return acknowledgedBoundedReadOnly == false && (boundedEdit == nil || acknowledgedBoundedEdit == boundedEdit) && frame.epoch == owner && frame.sequence == acceptedSequence && frame.draft == draft &&
            frame.tenantId == tenantId && frame.principalId == principalId
    }
    func acknowledgesPlacement(draft: NativeExhibitionDraft?, tenantId: String?, principalId: String?,
                               selection: ViewportSelection, capability: NativeBoundedEditCapability) -> Bool {
        acknowledges(draft: draft, tenantId: tenantId, principalId: principalId, boundedEdit: capability)
            && acknowledgedFrame?.selection == selection && acknowledgedFrame?.disabled == false
            && editingContextActive
    }
    var hostEpoch: String? { owner }
    func confirmsFloorPreview(_ draft: NativeExhibitionDraft, tenantId: String, principalId: String) -> Bool {
        confirmsMetadataPreview(draft, tenantId: tenantId, principalId: principalId)
    }
    // The existing immutable disabled-frame identity check also fences metadata previews.
    func confirmsMetadataPreview(_ draft: NativeExhibitionDraft, tenantId: String, principalId: String) -> Bool {
        connected && acknowledgedFrame?.confirmsFloorPreview(ownerEpoch: owner, acceptedSequence: acceptedSequence,
            expectedDraft: draft, tenantId: tenantId, principalId: principalId) == true
    }
    func ordinaryRecoveryReady(replacing previousEpoch: String, draft: NativeExhibitionDraft, tenantId: String, principalId: String) -> Bool {
        guard connected, let acknowledgedFrame else { return false }
        return acknowledgedFrame.confirmsHostRecovery(replacing: previousEpoch, ownerEpoch: owner, acceptedSequence: acceptedSequence,
                                                     expectedDraft: draft, tenantId: tenantId, principalId: principalId)
    }
    func waitForOrdinaryRecovery(replacing previousEpoch: String, draft: NativeExhibitionDraft, tenantId: String, principalId: String) async throws {
        for _ in 0..<100 {
            try Task.checkCancellation()
            if ordinaryRecoveryReady(replacing: previousEpoch, draft: draft, tenantId: tenantId, principalId: principalId) { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw AuthoringFailure("VIEWPORT_FRAME_STALE")
    }
    fileprivate var entrancePreviewAction: ((NativeEntrancePreviewInput) async throws -> NativeEntrancePreviewOutput)?
    func entrancePreview(_ input: NativeEntrancePreviewInput) async throws -> NativeEntrancePreviewOutput {
        // The explicitly staged display draft must pass the ordinary frame
        // handshake before any preview request can name that frame.
        for _ in 0..<100 {
            try Task.checkCancellation()
            if connected, acknowledgedFrame?.disabled == true, acknowledgedFrame?.draft == input.draft {
                guard entrancePreviewSupported, let entrancePreviewAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                return try await entrancePreviewAction(input)
            }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw AuthoringFailure("VIEWPORT_FRAME_STALE")
    }
    @Published fileprivate(set) var acceptedSequence = 0
    fileprivate var registeredFurnishingThumbnailAction: ((WorkspaceFurnishing, NativeExhibitionDraft) async throws -> (ViewportFurnishingThumbnailRequest, Data))?
    fileprivate var thumbnailAction: ((String, Int) async throws -> (ViewportThumbnailReply, ViewportThumbnailRequest))?
    fileprivate var sceneSnapshotAction: ((NativeStudioResponse, SceneSnapshotCamera, String) async throws -> SceneSnapshotResult)?
    #if DEBUG
    // Read-only synchronization for the opt-in live host integration. This
    // observes the coordinator's existing policy; it grants no call authority.
    fileprivate var snapshotPolicyObservation: (() -> Bool)?
    var sceneSnapshotPolicyActiveForTesting: Bool { snapshotPolicyObservation?() == true }
    #endif
    fileprivate var findSelectionAction: (() async throws -> Void)?
    fileprivate var catalogDropAction: ((CatalogDropQuery) async throws -> CatalogDropHit?)?
    fileprivate var placementAction: ((ArtworkPlacementIntent, ViewportPlacementPhase, ViewportPlacementTarget?) async throws -> DocumentValue?)?
    fileprivate var furnishingVisualsAction: ((FurnishingCommand) async throws -> FurnishingPreview)?
    fileprivate var sharedFurnishingVisualsAction: ((SharedFurnishingCommand, String?) async throws -> SharedFurnishingPreview)?
    fileprivate var furnishingThumbnailAction: (() async throws -> Data)?
    fileprivate var spaceSettingsAction: ((SpaceLightingCommand) async throws -> SpaceLightingPreview)?
    fileprivate var viewingPlanAction: ((ViewingPlanCommand) async throws -> ViewingPlanPreview)?
    fileprivate var lightInspectorAction: ((LightCommand, String) async throws -> LightPreview)?
    fileprivate var lightObjectAction: ((DocumentValue) async throws -> LightObjectPreview)?
    fileprivate var wallFinishAction: ((WallFinishCommand) async throws -> WallFinishPreview)?
    func wallFinish(_ command: WallFinishCommand) async throws -> WallFinishPreview {
        guard editingContextActive, connected, wallFinishSupported, let wallFinishAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        let context = editingContextGeneration
        let preview = try await wallFinishAction(command)
        guard editingContextActive, editingContextGeneration == context else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return preview
    }
    fileprivate var benchInspectorAction: ((BenchCommand, String) async throws -> BenchPreview)?
    fileprivate var plaqueInspectorAction: ((PlaqueCommand, String) async throws -> PlaquePreview)?
    func plaqueInspector(_ command: PlaqueCommand, selectedPlaqueId: String) async throws -> PlaquePreview {
        guard connected, plaqueInspectorSupported, let plaqueInspectorAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await plaqueInspectorAction(command, selectedPlaqueId)
    }
    func benchInspector(_ command: BenchCommand, selectedBenchId: String) async throws -> BenchPreview {
        guard connected, benchInspectorSupported, let benchInspectorAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await benchInspectorAction(command, selectedBenchId)
    }
    func furnishingVisuals(_ command: FurnishingCommand) async throws -> FurnishingPreview {
        guard connected, furnishingVisualsSupported, let furnishingVisualsAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await furnishingVisualsAction(command)
    }
    func sharedFurnishingVisuals(_ command: SharedFurnishingCommand, selectedInspectorId: String?) async throws -> SharedFurnishingPreview {
        guard editingContextActive, connected, sharedFurnishingVisualsSupported, let sharedFurnishingVisualsAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await sharedFurnishingVisualsAction(command, selectedInspectorId)
    }
    func furnishingThumbnail() async throws -> Data {
        guard connected, furnishingVisualsSupported, let furnishingThumbnailAction else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        return try await furnishingThumbnailAction()
    }
    func spaceSettings(_ command: SpaceLightingCommand) async throws -> SpaceLightingPreview {
        guard connected, spaceSettingsSupported, let spaceSettingsAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await spaceSettingsAction(command)
    }
    func viewingPlan(_ command: ViewingPlanCommand) async throws -> ViewingPlanPreview {
        guard editingContextActive, connected, viewingPlanSupported, let viewingPlanAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        let context = editingContextGeneration
        let preview = try await viewingPlanAction(command)
        guard editingContextActive, editingContextGeneration == context else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return preview
    }
    func lightInspector(_ command: LightCommand, selectedLightId: String) async throws -> LightPreview {
        guard connected, lightInspectorSupported, !command.usesLightDetails || lightFixtureSupported,
              let lightInspectorAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await lightInspectorAction(command, selectedLightId)
    }
    func lightObject(_ command: DocumentValue) async throws -> LightObjectPreview {
        guard editingContextActive, connected, lightObjectSupported, let lightObjectAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        let context = editingContextGeneration
        let preview = try await lightObjectAction(command)
        guard editingContextActive, connected, lightObjectSupported, editingContextGeneration == context else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return preview
    }
    /// Native saved-head proposals use the already negotiated API transport;
    /// they do not claim that the Web page implements lightObject.
    func nativeLightObjectFrame(in document: NativeDocumentState) throws -> ViewportFrame {
        guard editingContextActive, connected, let frame = acknowledgedFrame else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return try LightObjectRequest.nativeFrame(acknowledged: frame, document: document,
            ownerEpoch: owner, acceptedSequence: acceptedSequence,
            readOnly: acknowledgedBoundedReadOnly, boundedEdit: acknowledgedBoundedEdit)
    }
    func confirmsNativeLightObjectFrame(_ frame: ViewportFrame, in document: NativeDocumentState, context: UUID) -> Bool {
        guard editingContextGeneration == context, let current = try? nativeLightObjectFrame(in: document) else { return false }
        return current.epoch == frame.epoch && current.sequence == frame.sequence
            && current.workspaceId == frame.workspaceId && current.baseETag == frame.baseETag
            && current.draft == frame.draft
    }
    func lightMountCommand(lightId: String, attachment: DocumentValue) throws -> DocumentValue {
        guard editingContextActive, connected, lightObjectSupported, let frame = acknowledgedFrame,
              !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return try LightObjectRequest.mountCommand(lightId: lightId, attachment: attachment, frame: frame)
    }
    func catalogDropFrameMatches(_ draft: NativeExhibitionDraft, selection: ViewportSelection?) -> Bool {
        connected && editingContextActive && catalogDropSupported && acknowledgedFrame?.draft == draft
            && acknowledgedFrame?.selection == selection && acknowledgedFrame?.disabled == false
    }
    func catalogDrop(_ query: CatalogDropQuery) async throws -> CatalogDropHit? {
        guard connected, catalogDropSupported, let catalogDropAction else { throw AuthoringFailure("CATALOG_DROP_UNAVAILABLE") }
        return try await catalogDropAction(query)
    }
    func placement(_ intent: ArtworkPlacementIntent, phase: ViewportPlacementPhase, target: ViewportPlacementTarget? = nil) async throws -> DocumentValue? {
        guard connected, placementSupported, let placementAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        return try await placementAction(intent, phase, target)
    }
    func thumbnail(artworkId: String, revision: Int) async throws -> Data {
        guard connected, thumbnailSupported, let thumbnailAction else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let (reply, request) = try await thumbnailAction(artworkId, revision)
        return try reply.pngData(matching: request)
    }
    func acknowledgesFurnishingThumbnail(draft: NativeExhibitionDraft?, tenantId: String?, principalId: String?) -> Bool {
        editingContextActive && acknowledgedFrame?.disabled == false && acknowledgedBoundedEdit == nil
            && acknowledges(draft: draft, tenantId: tenantId, principalId: principalId)
    }

    func furnishingThumbnail(_ item: WorkspaceFurnishing, draft: NativeExhibitionDraft) async throws -> Data {
        return try await furnishingThumbnailCapture(item, draft: draft).1
    }
    func furnishingThumbnailCapture(_ item: WorkspaceFurnishing, draft: NativeExhibitionDraft) async throws -> (ViewportFurnishingThumbnailRequest, Data) {
        guard furnishingThumbnailSupported, let registeredFurnishingThumbnailAction else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        return try await registeredFurnishingThumbnailAction(item, draft)
    }

    func renderedThumbnail(artworkId: String, revision: Int) async throws -> ViewportRenderedThumbnail {
        guard connected, renderedThumbnailSupported, let thumbnailAction else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let (reply, request) = try await thumbnailAction(artworkId, revision)
        guard renderedThumbnailSupported else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        return try reply.renderedThumbnail(matching: request)
    }
    func sceneSnapshot(saved: NativeStudioResponse, viewpointId: String, placementId: String) async throws -> SceneSnapshotResult {
        return try await sceneSnapshot(saved: saved, camera: .savedViewpoint(viewpointId), placementId: placementId)
    }
    func sceneSnapshot(saved: NativeStudioResponse, camera: SceneSnapshotCamera, placementId: String) async throws -> SceneSnapshotResult {
        guard editingContextActive, connected, sceneSnapshotSupported,
              !camera.isInspection || sceneSnapshotInspectionSupported, let sceneSnapshotAction else { throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY") }
        let context = editingContextGeneration
        let image = try await sceneSnapshotAction(saved, camera, placementId)
        guard editingContextActive, editingContextGeneration == context else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
        return image
    }
    func findSelection() async {
        guard connected, findSelectionSupported, findSelectionAction != nil else { return }
        do { try await findSelectionChecked(); message = "선택한 배치가 보이도록 시점을 맞췄습니다." }
        catch { message = "선택한 배치를 찾지 못했습니다. 선택과 기존 시점은 유지됩니다."; diagnose("FIND", error) }
    }
    func acknowledgesOrdinarySelection(draft: NativeExhibitionDraft?, tenantId: String?, principalId: String?, selection: ViewportSelection) -> Bool {
        acknowledges(draft: draft, tenantId: tenantId, principalId: principalId)
            && acknowledgedFrame?.selection == selection && acknowledgedFrame?.disabled == false && editingContextActive
    }
    /// Reuses the same private camera request and guards, exposing completion to
    /// a typed prospective preview without turning a disabled frame editable.
    func findSelectionChecked() async throws {
        guard connected, findSelectionSupported, let findSelectionAction else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        try await findSelectionAction()
    }
    @Published fileprivate(set) var sceneProgress: ViewportSceneProgress? = .preparing
    @Published var rendering = false
    @Published var loaded = 0
    @Published var failed = 0
    @Published var message = "3D 화면을 준비하고 있습니다."
    @Published var diagnostic = ""
    func diagnose(_ stage: String, _ error: Error) {
        let e = error as NSError
        let domain = e.domain.range(of: "^[A-Za-z0-9_.-]{1,80}$", options: .regularExpression) == nil ? "WebKit" : e.domain
        let rawDetail = (error as? AuthoringFailure)?.code ?? (e.userInfo["WKJavaScriptExceptionMessage"] as? String) ?? ""
        let detail = rawDetail.hasPrefix("Error: ") ? String(rawDetail.dropFirst(7)) : rawDetail
        let known = ["crypto.randomUUID", "structuredClone", "INVALID_GEOMETRY", "VIEWPORT_COMMAND_DENIED", "VIEWPORT_NOT_READY", "func2"].filter { detail.contains($0) }.joined(separator: ",")
        let snapshotCodes = ["SCENE_SNAPSHOT_NOT_READY", "SCENE_SNAPSHOT_INVALID", "SCENE_SNAPSHOT_STALE", "SCENE_SNAPSHOT_UNAVAILABLE", "SCENE_SNAPSHOT_OVERLAY_UNSUPPORTED", "SCENE_SNAPSHOT_TARGET_INVALID", "SCENE_SNAPSHOT_TARGET_NOT_READY", "SCENE_SNAPSHOT_SAVED_CHANGED", "SCENE_SNAPSHOT_SAVED_LIMIT", "SCENE_SNAPSHOT_RIGHTS_UNVERIFIED", "SCENE_SNAPSHOT_PNG_LIMIT", "SCENE_SNAPSHOT_ENCODE", "SCENE_SNAPSHOT_CANCELLED"]
        let allowedCode = stage == "SCENE_SNAPSHOT" ? snapshotCodes.contains(detail) : detail.range(of: "^[A-Z0-9_]{1,180}$", options: .regularExpression) != nil
        let code = allowedCode ? " · " + detail : ""
        var snapshotDetail = ""
        if stage == "SCENE_SNAPSHOT" {
            // Fixed tokens and bounded source positions identify a script failure
            // without showing arbitrary exception text, URLs, tokens or document data.
            let kinds = ["TypeError", "ReferenceError", "RangeError", "SyntaxError"].filter { rawDetail.hasPrefix($0 + ":") }
            let methods = ["crypto.subtle", "getActiveCubeFace", "getActiveMipmapLevel", "readRenderTargetPixels", "ImageData", "throwIfAborted"].filter { rawDetail.contains($0) }
            let positions = [("line", "WKJavaScriptExceptionLineNumber"), ("column", "WKJavaScriptExceptionColumnNumber")].compactMap { name, key -> String? in
                guard let number = e.userInfo[key] as? NSNumber, number.doubleValue.isFinite,
                      number.doubleValue.rounded() == number.doubleValue, (0...1_000_000).contains(number.doubleValue) else { return nil }
                return name + " " + number.stringValue
            }
            let fields = kinds + methods + positions
            if !fields.isEmpty { snapshotDetail = " · " + fields.joined(separator: " · ") }
        }
        diagnostic = "\(stage) · \(domain) · \(e.code)" + code + (known.isEmpty ? "" : " · " + known) + snapshotDetail
    }
    fileprivate var owner: String?
    fileprivate var action: ((NativeGeometryAction) async throws -> (DocumentValue, NativeExhibitionDraft))?
    func perform(_ command: NativeGeometryAction, model: ExhibitionModel) async {
        guard editingContextActive, !model.placingArtwork, let action, connected else { return }
        if case .installation(.alignOnVerifiedPlinth(_)) = command, !plinthEnvelopeAlignSupported { return }
        if case .roomBaseLight = command, !model.canUseRoomBaseLight { return }
        let contextGeneration = editingContextGeneration, access = model.viewportAccessID
        do {
            let (candidate, base) = try await action(command)
            guard editingContextActive, editingContextGeneration == contextGeneration else { return }
            if case .roomBaseLight(let roomBaseLight) = command {
                await model.acceptRoomBaseLightCandidate(candidate, base: base, action: roomBaseLight, access: access)
            } else if case .installation(let installation) = command { await model.acceptInstallationCandidate(candidate, base: base, command: installation) }
            else if case .architecture(let architecture) = command { await model.acceptArchitectureCandidate(candidate, base: base, command: architecture) }
            else if case .surfaceMaterial(let material) = command { await model.acceptSurfaceMaterialPreview(candidate, base: base, command: material) }
            else if case .surfaceMaterialTexture(let material) = command { await model.acceptSurfaceMaterialTexturePreview(candidate, base: base, command: material) }
            else { await model.acceptViewportCandidate(candidate, base: base) }
        }
        catch {
            message = "이 조작을 적용하지 못했습니다. 현재 전시와 선택을 확인하세요."
            if case .architecture = command { message = "이 공간 편집을 적용하지 못했습니다. 입력은 유지되며 기존 전시는 바뀌지 않았습니다." }
            if case .surfaceMaterial = command { message = "재질을 미리 보지 못했습니다. 입력과 원래 재질은 유지됩니다." }
            if case .surfaceMaterialTexture = command { message = "재질을 미리 보지 못했습니다. 입력과 원래 재질은 유지됩니다." }
            if case .installation = command,
               let detail = (error as NSError).userInfo["WKJavaScriptExceptionMessage"] as? String,
               let reason = InstallationCommandReason.fromJavaScriptMessage(detail) { message = reason.explanation }
            diagnose("COMMAND", error)
        }
    }
}

struct NativeViewport: NSViewRepresentable {
    let credentials: LocalAuthoringSession
    let privateFloor: Bool
    let workspaceMaterials: Bool
    let workspaceMaterialMultimap: Bool
    let workspaceFurnishings: Bool
    let draft: NativeExhibitionDraft
    let selection: ViewportSelection?
    let presentation: ViewportPresentation
    let mode: String
    let view: String
    let cameraSequence: Int
    let transparent: Bool
    let disabled: Bool
    let interactionDisabled: Bool
    let selectionDisabled: Bool
    var boundedEdit: NativeBoundedEditCapability? = nil
    var acceptBoundedAck: (ViewportFrame, ViewportAcceptance) -> Bool = { _, _ in false }
    @ObservedObject var controller: NativeViewportController
    let onSelect: (ViewportSelection) -> Void
    let onCandidate: (DocumentValue, NativeExhibitionDraft) -> Void
    let onPlacementTarget: (DocumentValue) -> Void
    let onPlacementCancel: (String) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: NSViewRepresentableContext<NativeViewport>) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.userContentController.add(context.coordinator, name: "exhibitosViewport")
        let web = WKWebView(frame: .zero, configuration: configuration)
        web.pageZoom = 1 // Snapshot CSS pixels map one-to-one to native points.
        web.navigationDelegate = context.coordinator; web.uiDelegate = context.coordinator
        web.allowsBackForwardNavigationGestures = false; web.allowsLinkPreview = false
        web.setAccessibilityLabel("전시 편집 화면")
        context.coordinator.attach(web)
        context.coordinator.update(self)
        return web
    }
    func updateNSView(_ nsView: WKWebView, context: NSViewRepresentableContext<NativeViewport>) {
        nsView.setAccessibilityLabel(presentation == .plan && controller.planSupported ? "전시 2D 평면 편집 화면" : "전시 3D 편집 화면")
        context.coordinator.update(self)
    }
    static func dismantleNSView(_ web: WKWebView, coordinator: Coordinator) {
        coordinator.invalidate()
        web.stopLoading(); web.navigationDelegate = nil; web.uiDelegate = nil
        web.configuration.userContentController.removeScriptMessageHandler(forName: "exhibitosViewport")
    }
    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
        weak var web: WKWebView?
        var parent: NativeViewport
        var controller: NativeViewportController { parent.controller }
        var generation = UUID()
        private var active = true
        private var epoch = UUID().uuidString.lowercased()
        private var sequence = 0
        private var sent = 0
        private var fingerprint: Data?
        private var editingContextGeneration: UUID?
        private var installedToken: String?
        private var resourceRule: WKContentRuleList?
        private var snapshotPolicy: (id: UUID, content: WKUserContentController, base: WKContentRuleList, rule: WKContentRuleList)?
        private var prepared = false
        private var latest: ViewportFrame?
        private var placementIntent: ArtworkPlacementIntent?
        private var inspectorPending = false
        private var entranceOperation: UUID?
        var sendTask: Task<Void, Never>?
        init(_ parent: NativeViewport) { self.parent = parent; super.init() }
        func attach(_ web: WKWebView) {
            // A coordinator owns the controller only once its actual view exists.
            self.web = web
            controller.owner = epoch
            #if DEBUG
            controller.snapshotPolicyObservation = { [weak self] in self?.snapshotPolicy != nil }
            #endif
        }
        func invalidate() {
            guard active else { return }
            restoreSnapshotPolicy()
            endPlacement()
            entranceOperation = nil
            if controller.owner == epoch { controller.sceneSnapshotAction = nil; controller.sceneSnapshotSupported = false; controller.sceneSnapshotInspectionSupported = false }
            #if DEBUG
            if controller.owner == epoch { controller.snapshotPolicyObservation = nil }
            #endif
            if controller.owner == epoch { controller.entrancePreviewAction = nil; controller.entrancePreviewSupported = false; controller.acknowledgedFrame = nil; controller.acknowledgedBoundedEdit = nil; controller.displayReadoutAction = nil; controller.displayReadout = "" }
            active = false; generation = UUID(); sendTask?.cancel()
            if controller.owner == epoch { controller.sharedFurnishingVisualsAction = nil; controller.sharedFurnishingVisualsSupported = false; controller.plinthEnvelopeAlignSupported = false }
            if controller.owner == epoch { controller.furnishingVisualsAction = nil; controller.furnishingThumbnailAction = nil; controller.spaceSettingsAction = nil; controller.viewingPlanAction = nil; controller.lightInspectorAction = nil; controller.lightObjectAction = nil; controller.wallFinishAction = nil; controller.furnishingVisualsSupported = false; controller.spaceSettingsSupported = false; controller.viewingPlanSupported = false; controller.lightInspectorSupported = false; controller.lightFixtureSupported = false; controller.lightObjectSupported = false; controller.roomBaseLightSupported = false; controller.wallFinishSupported = false; controller.benchInspectorAction = nil; controller.benchInspectorSupported = false; controller.plaqueInspectorAction = nil; controller.plaqueInspectorSupported = false; controller.surfaceMaterialTexturesSupported = false }
            if controller.owner == epoch { controller.action = nil; controller.thumbnailAction = nil; controller.registeredFurnishingThumbnailAction = nil; controller.findSelectionAction = nil; controller.placementAction = nil; controller.catalogDropAction = nil; controller.catalogDropSupported = false; controller.placementSupported = false; controller.connected = false; controller.planSupported = false; controller.thumbnailSupported = false; controller.renderedThumbnailSupported = false; controller.furnishingThumbnailSupported = false; controller.findSelectionSupported = false; controller.acceptedSequence = 0 }
        }
        private func restoreSnapshotPolicy(_ id: UUID? = nil) {
            guard let held = snapshotPolicy, id == nil || held.id == id else { return }
            snapshotPolicy = nil
            // Keep a blocker installed throughout the transition and preserve
            // a newer base configuration if this web view was reconfigured.
            held.content.add(resourceRule ?? held.base)
            held.content.remove(held.rule)
        }
        private func endPlacement() {
            guard let intent = placementIntent else { return }
            placementIntent = nil
            let notify = parent.onPlacementCancel
            Task { @MainActor in notify(intent.requestId) }
        }
        private var page: URL? { URL(string: parent.credentials.origin + "/native-viewport.html") }
        private func isPage(_ url: URL?) -> Bool { url == page && url?.user == nil && url?.password == nil }
        func update(_ value: NativeViewport) {
            guard active else { return }
            parent = value
            guard let page, page.scheme == "http", page.host == "127.0.0.1", let port = page.port, port >= 1024, UUID(uuidString: value.credentials.tenantId) != nil, UUID(uuidString: value.credentials.principalId) != nil else { return }
            if let installedToken, installedToken != value.credentials.token {
                // A renewed session requires the existing new-coordinator identity.
                // Revoke this page once; repeated published resets during a SwiftUI
                // update would prevent that replacement from being mounted.
                invalidate()
                web?.stopLoading()
                return
            }
            if installedToken == nil {
                controller.sceneSnapshotSupported = false; controller.sceneSnapshotInspectionSupported = false; controller.sceneSnapshotAction = nil
                controller.sharedFurnishingVisualsSupported = false; controller.plinthEnvelopeAlignSupported = false
                entranceOperation = nil; controller.entrancePreviewSupported = false; controller.acknowledgedFrame = nil; controller.acknowledgedBoundedEdit = nil; controller.displayReadoutAction = nil; controller.displayReadout = ""
                controller.furnishingVisualsSupported = false; controller.spaceSettingsSupported = false; controller.viewingPlanSupported = false; controller.lightInspectorSupported = false; controller.lightFixtureSupported = false; controller.lightObjectSupported = false; controller.roomBaseLightSupported = false; controller.wallFinishSupported = false; controller.benchInspectorSupported = false; controller.plaqueInspectorSupported = false; controller.surfaceMaterialTexturesSupported = false
                installedToken = value.credentials.token
                generation = UUID(); sequence = 0; sent = 0; fingerprint = nil; prepared = false
                sendTask?.cancel(); sendTask = nil
                controller.connected = false; controller.rendering = false; controller.planSupported = false; controller.thumbnailSupported = false; controller.renderedThumbnailSupported = false; controller.furnishingThumbnailSupported = false; controller.findSelectionSupported = false; controller.acceptedSequence = 0
                let installGeneration = generation
                Task { await install(page: page, generation: installGeneration) }
            }
            let selection = value.selection.flatMap {
                $0.belongs(to: value.draft) && ($0.kind != "light" || controller.lightObjectSupported) ? $0 : nil
            }
            let probe = ViewportFrame(epoch: epoch, sequence: 1, tenantId: value.credentials.tenantId, principalId: value.credentials.principalId, draft: value.draft, selection: selection, mode: value.mode, view: value.view, disabled: value.disabled, cameraSequence: value.cameraSequence, transparent: value.transparent, presentation: controller.planSupported ? value.presentation : .threeD, workspaceId: value.boundedEdit?.workspaceId, baseETag: value.boundedEdit?.etag)
            let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
            guard let bytes = try? encoder.encode(probe) else { return }
            let hash = Data(SHA256.hash(data: bytes))
            let contextChanged = editingContextGeneration != controller.editingContextGeneration
            editingContextGeneration = controller.editingContextGeneration
            guard hash != fingerprint || contextChanged else { return }
            restoreSnapshotPolicy()
            // Status-only rerenders dedupe above. A genuinely different frame
            // invalidates the transient operation before any late callback.
            endPlacement()
            entranceOperation = nil
            fingerprint = hash; sequence += 1
            if latest?.draft != value.draft { controller.sceneProgress = .preparing }
            if !controller.displayReadout.isEmpty { controller.displayReadout = "" }
            controller.connected = false
            controller.acknowledgedBoundedEdit = nil
            latest = ViewportFrame(epoch: epoch, sequence: sequence, tenantId: value.credentials.tenantId, principalId: value.credentials.principalId, draft: value.draft, selection: selection, mode: value.mode, view: value.view, disabled: value.disabled, cameraSequence: value.cameraSequence, transparent: value.transparent, presentation: controller.planSupported ? value.presentation : .threeD, workspaceId: value.boundedEdit?.workspaceId, baseETag: value.boundedEdit?.etag)
            schedule()
        }
        private func install(page: URL, generation: UUID) async {
            guard active, self.generation == generation, let web, parent.credentials.token.range(of: "^[A-Za-z0-9_-]{43}$", options: .regularExpression) != nil else { return }
            var stage = "COOKIE"
            do {
                // The token never enters JS, a URL, an argument, log or disk store.
                let headers = ["Set-Cookie":"exhibitos_local_session=\(parent.credentials.token); Path=/; HttpOnly; SameSite=Strict"]
                guard let cookie = HTTPCookie.cookies(withResponseHeaderFields: headers, for: page).first, cookie.isHTTPOnly else { throw BridgeFailure("VIEWPORT_COOKIE_INVALID") }
                let store = web.configuration.websiteDataStore.httpCookieStore
                for cookie in await store.allCookies() { await store.deleteCookie(cookie) }
                await store.setCookie(cookie)
                stage = "RESOURCE_POLICY"
                let encodedRules = try ViewportResourcePolicy.encodedRules(origin: parent.credentials.origin, tenantId: parent.credentials.tenantId, privateFloor: parent.privateFloor, workspaceMaterials: parent.workspaceMaterials, workspaceMaterialMultimap: parent.workspaceMaterialMultimap, workspaceFurnishings: parent.workspaceFurnishings)
                let identifier = "ExhibitOSViewport-" + UUID().uuidString
                let rule = try await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: encodedRules)
                try? await WKContentRuleListStore.default().removeContentRuleList(forIdentifier: identifier)
                guard active, self.generation == generation else { return }
                guard let rule else { throw BridgeFailure("VIEWPORT_POLICY_UNAVAILABLE") }
                web.configuration.userContentController.removeAllContentRuleLists(); web.configuration.userContentController.add(rule)
                resourceRule = rule
                web.load(URLRequest(url: page, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 15))
            } catch { if self.generation == generation { controller.sceneProgress = nil; controller.message = "3D 화면 연결을 준비하지 못했습니다. 작업 공간을 다시 연결하세요."; controller.diagnose(stage, error) } }
        }
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            guard active, controller.owner == epoch, webView === web, isPage(webView.url) else { return }
            prepared = true; schedule()
        }
        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
            navigationAction.targetFrame?.isMainFrame == true && isPage(navigationAction.request.url) ? .allow : .cancel
        }
        func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse) async -> WKNavigationResponsePolicy {
            guard navigationResponse.isForMainFrame, isPage(navigationResponse.response.url), navigationResponse.response.mimeType == "text/html", (navigationResponse.response as? HTTPURLResponse)?.statusCode == 200 else { return .cancel }
            return .allow
        }
        func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? { nil }
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { guard active, controller.owner == epoch, webView === web else { return }; restoreSnapshotPolicy(); controller.sceneSnapshotSupported = false; controller.sceneSnapshotInspectionSupported = false; controller.sceneSnapshotAction = nil; controller.plinthEnvelopeAlignSupported = false; controller.sharedFurnishingVisualsSupported = false; entranceOperation = nil; if controller.owner == epoch { controller.entrancePreviewSupported = false; controller.acknowledgedFrame = nil; controller.acknowledgedBoundedEdit = nil; controller.displayReadoutAction = nil; controller.displayReadout = "" }; endPlacement(); prepared = false; controller.connected = false; controller.planSupported = false; controller.thumbnailSupported = false; controller.renderedThumbnailSupported = false; controller.furnishingThumbnailSupported = false; controller.findSelectionSupported = false; controller.catalogDropSupported = false; controller.placementSupported = false; controller.furnishingVisualsSupported = false; controller.spaceSettingsSupported = false; controller.viewingPlanSupported = false; controller.lightInspectorSupported = false; controller.lightFixtureSupported = false; controller.lightObjectSupported = false; controller.roomBaseLightSupported = false; controller.wallFinishSupported = false; controller.acceptedSequence = 0; controller.rendering = false; controller.sceneProgress = nil; controller.message = "3D 화면이 닫혔습니다. 입력은 유지됩니다. 편집 화면 다시 열기를 선택하세요." }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            guard active, controller.owner == epoch, webView === web else { return }
            controller.sceneProgress = nil
            controller.message = "3D 화면을 열지 못했습니다. 로컬 제작 상태를 확인하세요."
            controller.diagnose("PAGE_LOAD", error)
        }
        private func schedule() {
            guard active, controller.owner == epoch, prepared, sendTask == nil, let web else { return }
            let generation = generation
            sendTask = Task { [weak self] in
                guard let self else { return }
                defer { self.sendTask = nil }
                while !Task.isCancelled, self.generation == generation, let frame = self.latest, frame.sequence > self.sent {
                    do {
                        let bytes = try JSONEncoder().encode(frame)
                        guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                        // Foundation can intern empty NSArray containers. Passing a JSON string
                        // gives every document collection its own JS array, as the wire format requires.
                        let value = String(decoding: bytes, as: UTF8.self)
                        let response = try await web.callAsyncJavaScript("const deadline = Date.now() + 10000; while (!window.exhibitosViewport && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 25)); if (!window.exhibitosViewport) throw new Error('VIEWPORT_ENTRY_UNAVAILABLE'); return await window.exhibitosViewport.accept(JSON.parse(frameJSON))", arguments: ["frameJSON":value], in: nil, contentWorld: .page)
                        guard self.active, !Task.isCancelled, self.controller.owner == self.epoch, self.generation == generation else { return }
                        // A newer frame owns admission; ignore even a rejected
                        // older response and advance to the current frame.
                        if frame.workspaceId != nil, frame.sequence != self.sequence { self.sent = frame.sequence; continue }
                        guard let response, JSONSerialization.isValidJSONObject(response),
                              let acceptance = try? JSONDecoder().decode(ViewportAcceptance.self, from: JSONSerialization.data(withJSONObject: response)), acceptance.accepted else { throw BridgeFailure("VIEWPORT_ACCEPT_INVALID") }
                        if frame.workspaceId != nil {
                            guard self.parent.acceptBoundedAck(frame, acceptance),
                                  let capability = self.parent.boundedEdit, acceptance.editingCapability == capability else {
                                self.controller.boundedReadOnly = true
                                throw BridgeFailure("VIEWPORT_ACCEPT_INVALID")
                            }
                            self.controller.acknowledgedBoundedEdit = capability
                        } else { self.controller.acknowledgedBoundedEdit = nil }
                        if frame.sequence == self.sequence { self.controller.acknowledgedBoundedReadOnly = acceptance.boundedReadOnly }
                        if acceptance.boundedReadOnly == true {
                            self.controller.boundedReadOnly = true
                        } else if acceptance.boundedReadOnly == false && frame.sequence == self.sequence {
                            self.controller.boundedReadOnly = false
                        }
                        self.controller.planSupported = acceptance.supportsPlan
                        self.controller.thumbnailSupported = acceptance.supportsArtworkThumbnail
                        self.controller.renderedThumbnailSupported = acceptance.supportsRenderedArtworkThumbnail
                        self.controller.furnishingThumbnailSupported = acceptance.supportsFurnishingThumbnail
                        self.controller.sceneSnapshotSupported = acceptance.supportsSceneSnapshot
                        self.controller.sceneSnapshotInspectionSupported = acceptance.supportsSceneSnapshotInspection
                        self.controller.findSelectionSupported = acceptance.supportsFindSelection
                        self.controller.catalogDropSupported = acceptance.supportsCatalogDrop
                        CatalogDropTrace.record("frameAccepted", "catalog=\(acceptance.supportsCatalogDrop) disabled=\(frame.disabled) bounded=\(acceptance.boundedReadOnly == true) editing=\(self.controller.editingContextActive)")
                        self.controller.placementSupported = acceptance.supportsArtworkPlacement
                        self.controller.furnishingVisualsSupported = acceptance.supportsFurnishingVisuals
                        self.controller.sharedFurnishingVisualsSupported = acceptance.supportsSharedFurnishingVisuals
                        self.controller.plinthEnvelopeAlignSupported = acceptance.supportsPlinthEnvelopeAlign
                        self.controller.spaceSettingsSupported = acceptance.supportsSpaceSettings
                        self.controller.viewingPlanSupported = acceptance.supportsViewingPlan
                        self.controller.lightInspectorSupported = acceptance.supportsLightInspector
                        self.controller.lightFixtureSupported = acceptance.supportsLightFixture
                        self.controller.lightObjectSupported = acceptance.supportsLightObject
                        self.controller.roomBaseLightSupported = frame.workspaceId == nil && !frame.disabled && acceptance.supportsRoomBaseLight
                        self.controller.wallFinishSupported = acceptance.supportsWallFinish
                        self.controller.benchInspectorSupported = acceptance.supportsBenchInspector
                        self.controller.plaqueInspectorSupported = acceptance.supportsPlaqueInspector
                        self.controller.surfaceMaterialTexturesSupported = acceptance.supportsSurfaceMaterialTextures
                        self.controller.entrancePreviewSupported = acceptance.supportsEntrancePreview
                        self.controller.acknowledgedFrame = frame
                        self.controller.displayReadout = ""
                        self.controller.acceptedSequence = frame.sequence
                        self.sent = frame.sequence; self.controller.connected = self.sent == self.sequence; self.controller.diagnostic = ""
                        // A later accepted frame supersedes an earlier frame error.
                        if self.controller.connected && self.controller.message == "전시를 3D에 표시하지 못했습니다. 저장한 전시와 연결을 확인하세요." {
                            self.controller.message = "전시 연결을 확인했습니다."
                        }
                    } catch { if self.generation == generation { self.controller.connected = false; self.controller.sceneProgress = nil; self.controller.message = "전시를 3D에 표시하지 못했습니다. 저장한 전시와 연결을 확인하세요."; self.controller.diagnose("FRAME", error) }; break }
                }
            }
            controller.displayReadoutAction = { [weak self] input in
                guard let self, self.active, self.prepared, let web = self.web, let frame = self.latest,
                      frame.draft == input.draft, frame.selection == input.selection,
                      frame.view == input.view, frame.cameraSequence == input.cameraSequence,
                      frame.transparent == input.transparent, frame.presentation == input.presentation,
                      frame.presentation == .threeD else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let generation = self.generation, context = self.controller.editingContextGeneration
                @MainActor func current() throws {
                    guard self.active, self.prepared, self.isPage(web.url), self.generation == generation,
                          self.controller.owner == self.epoch, frame.epoch == self.epoch,
                          self.controller.editingContextActive, self.controller.editingContextGeneration == context,
                          self.controller.connected, self.sent == frame.sequence,
                          self.controller.acceptedSequence == frame.sequence,
                          self.controller.acknowledgedFrame?.epoch == frame.epoch,
                          self.controller.acknowledgedFrame?.selection == input.selection,
                          self.controller.acknowledgedFrame?.draft == input.draft,
                          self.controller.acknowledgedFrame?.view == input.view,
                          self.controller.acknowledgedFrame?.cameraSequence == input.cameraSequence,
                          self.controller.acknowledgedFrame?.transparent == input.transparent,
                          self.controller.acknowledgedFrame?.presentation == input.presentation,
                          self.latest?.sequence == frame.sequence,
                          self.parent.draft == input.draft, self.parent.selection == input.selection,
                          self.parent.view == input.view, self.parent.cameraSequence == input.cameraSequence,
                          (self.controller.planSupported ? self.parent.presentation : .threeD) == input.presentation,
                          self.parent.credentials.tenantId == frame.tenantId, self.parent.credentials.principalId == frame.principalId,
                          self.installedToken == self.parent.credentials.token, self.parent.disabled == frame.disabled,
                          self.parent.transparent == input.transparent, self.entranceOperation == nil,
                          self.placementIntent == nil, !self.inspectorPending else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                try current()
                // One synchronous, read-only sample. These existing datasets
                // report a last request/map admission, never live pose or appearance.
                let script = """
                const canvases = [...document.querySelectorAll('canvas[aria-label="전시 공간 3D 미리보기"]')].filter(c => {
                  const r = c.getBoundingClientRect(); return c.isConnected && r.width > 0 && r.height > 0;
                });
                if (canvases.length !== 1) throw new Error('VIEWPORT_DISPLAY_CANVAS_UNAVAILABLE');
                const c = canvases[0], result = {};
                for (const key of ['floorView', 'cameraProjection', 'floorTextures']) {
                  const value = c.dataset[key] ?? null;
                  if (value !== null && value.length > 4096) throw new Error('VIEWPORT_DISPLAY_LIMIT');
                  result[key] = value;
                }
                // Project only the agreed bounded loader slice. Legacy or
                // malformed records remain unavailable; never forward the
                // unrelated viewer state, raw errors, paths or timing data.
                result.viewerState = null;
                const raw = c.dataset.viewerState;
                if (typeof raw === 'string' && raw.length <= 131072) {
                    try {
                        const stages = JSON.parse(raw).preparationStages;
                        if (Array.isArray(stages) && stages.length <= 32) {
                            const uuid = value => typeof value === 'string' && value.length === 36 &&
                                /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i.test(value);
                            const token = (value, pattern) => typeof value === 'string' && pattern.exec(value)?.[0] === value;
                            const rows = [];
                            for (const row of stages) {
                                if (!row || typeof row !== 'object' ||
                                    !['assetId','artworkRevisionId','stage','errorCode'].every(key => Object.hasOwn(row,key)) ||
                                    !uuid(row.assetId) || !(row.artworkRevisionId === null || uuid(row.artworkRevisionId)) ||
                                    !token(row.stage,/^[a-z][a-z0-9-]{0,63}$/)) throw new Error('UNAVAILABLE');
                                const errorCode = token(row.errorCode,/^[A-Z][A-Z0-9_]{0,79}$/) ? row.errorCode : null;
                                if (errorCode !== null && !['failed','deferred-budget'].includes(row.stage)) throw new Error('UNAVAILABLE');
                                rows.push({assetId:row.assetId,artworkRevisionId:row.artworkRevisionId,stage:row.stage,errorCode});
                            }
                            const value = JSON.stringify({preparationStages: rows});
                            if (value.length <= 16384) result.viewerState = value;
                        }
                    } catch { /* No exact attribution is available. */ }
                }
                \(WorkspaceFurnishingFailureReadout.samplingScript)
                return result;
                """
                let response = try await web.callAsyncJavaScript(script, arguments: [:], in: nil, contentWorld: .page)
                try Task.checkCancellation(); try current()
                guard let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 65_536, case .object(let fields) = try JSONDecoder().decode(DocumentValue.self, from: bytes),
                      Set(fields.keys) == Set(["floorView", "cameraProjection", "floorTextures", "viewerState", "workspaceFurnishingFailures"]) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                var datasets: [String: DocumentValue] = [:]
                for (key, value) in fields {
                    if key == "workspaceFurnishingFailures" {
                        datasets[key] = WorkspaceFurnishingFailureReadout.projected(value.string, in: frame.draft)
                        continue
                    }
                    if key == "viewerState" {
                        datasets[key] = AssetPreparationReadout.projected(value.string, in: frame.draft)
                        continue
                    }
                    if value == .null { datasets[key] = .null; continue }
                    guard let string = value.string, string.utf8.count <= 4096 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    let parsed = try JSONDecoder().decode(DocumentValue.self, from: Data(string.utf8))
                    guard case .object = parsed else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    datasets[key] = parsed
                }
                let selectedFloor = input.selection.flatMap { selection in
                    selection.kind == "surface" && input.draft.candidate["surfaces"].items?.contains(where: { $0["id"].string == selection.id && $0["type"].string == "floor" }) == true ? selection.id : nil
                }
                let lastFloor = datasets["floorView"]?["surfaceId"].string
                let match = selectedFloor != nil && lastFloor?.lowercased() == selectedFloor?.lowercased()
                let selection: DocumentValue = frame.selection.map { .object(["kind": .string($0.kind), "id": .string($0.id)]) } ?? .null
                let ack: DocumentValue = .object(["epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)),
                    "selection": selection, "view": .string(frame.view), "cameraSequence": .integer(Int64(frame.cameraSequence)),
                    "transparent": .bool(frame.transparent), "presentation": .string(frame.presentation.rawValue)])
                let value: DocumentValue = .object(["ack": ack, "canvas": .object(datasets),
                    "lastFloorRequestMatchesSelection": .bool(match)])
                let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
                return String(decoding: try encoder.encode(value), as: UTF8.self)
            }
            controller.entrancePreviewAction = { [weak self] input in
                guard let self, self.active, self.prepared, self.controller.owner == self.epoch,
                      self.controller.entrancePreviewSupported, self.placementIntent == nil, !self.inspectorPending,
                      self.entranceOperation == nil || input.action == .close,
                      let web = self.web, let frame = self.latest, self.sent == frame.sequence,
                      frame.draft == input.draft, frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let operation = UUID(), generation = self.generation
                self.entranceOperation = operation
                defer { if self.entranceOperation == operation { self.entranceOperation = nil } }
                let request = try ViewportEntrancePreviewRequest(frame: frame, action: input.action, device: input.device, scenario: input.scenario,
                                                               reducedMotion: input.reducedMotion, theme: input.theme, locale: input.locale)
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                var acceptedViewport: CGSize?
                var acceptedBackingScale: CGFloat?
                @MainActor func current() throws {
                    try Task.checkCancellation()
                    guard self.active, self.prepared, self.controller.connected, self.controller.owner == self.epoch,
                          self.controller.entrancePreviewSupported, self.generation == generation,
                          self.entranceOperation == operation, self.latest?.sequence == frame.sequence,
                          self.latest?.draft == input.draft, web.pageZoom == 1 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    if let acceptedViewport, web.bounds.size != acceptedViewport { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    if let acceptedBackingScale, web.window?.backingScaleFactor != acceptedBackingScale { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                func decode(_ value: Any?) throws -> DocumentValue {
                    guard let value, JSONSerialization.isValidJSONObject(value) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    let data = try JSONSerialization.data(withJSONObject: value)
                    guard data.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                    return try JSONDecoder().decode(DocumentValue.self, from: data)
                }
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.entrancePreview !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.entrancePreview(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try current()
                let reply = try request.checkedReply(decode(response))
                guard let width = reply.value["viewport"]["width"].double, let height = reply.value["viewport"]["height"].double,
                      abs(width - Double(web.bounds.width)) <= 0.5, abs(height - Double(web.bounds.height)) <= 0.5 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                acceptedViewport = web.bounds.size
                guard input.action == .thumbnail else { return NativeEntrancePreviewOutput(request: request, png: nil) }
                // A fallback layout is not proof of an actual space image.
                if input.theme == .threshold, reply.value["imageStatus"].string != "ready" { return NativeEntrancePreviewOutput(request: request, png: nil) }
                guard reply.snapshotRect.height / reply.snapshotRect.width <= 4 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                let scale = web.window?.backingScaleFactor ?? 1
                guard scale.isFinite, (1...4).contains(scale) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                acceptedBackingScale = web.window?.backingScaleFactor
                let config = WKSnapshotConfiguration(); config.rect = reply.snapshotRect
                config.snapshotWidth = NSNumber(value: 256 / Double(scale)); config.afterScreenUpdates = true
                let snapshot: NSImage? = try await web.takeSnapshot(configuration: config)
                try current()
                let state = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.entrancePreviewState !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.entrancePreviewState(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try current(); try reply.recheck(decode(state), request: request)
                guard let image = snapshot, let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff), bitmap.pixelsWide <= 258,
                      bitmap.pixelsHigh <= 1032, let png = bitmap.representation(using: .png, properties: [:]), png.count <= 1_048_576 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                return NativeEntrancePreviewOutput(request: request, png: png)
            }
            controller.furnishingVisualsAction = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.furnishingVisualsSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try FurnishingViewportRequest(frame: frame, command: command)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.furnishingVisuals !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.furnishingVisuals(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.furnishingVisualsSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.sharedFurnishingVisualsAction = { [weak self] command, selectedInspectorId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.editingContextActive,
                      self.controller.sharedFurnishingVisualsSupported, self.placementIntent == nil, !self.inspectorPending,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try SharedFurnishingViewportRequest(frame: frame, command: command, selectedInspectorId: selectedInspectorId)
                self.inspectorPending = true; defer { self.inspectorPending = false }
                let generation = self.generation, context = self.controller.editingContextGeneration
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.furnishingVisualsV2 !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.furnishingVisualsV2(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.sharedFurnishingVisualsSupported, self.controller.connected,
                      self.controller.editingContextActive, self.controller.editingContextGeneration == context,
                      self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence,
                      self.placementIntent == nil, let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.viewingPlanAction = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.viewingPlanSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try ViewingPlanViewportRequest(frame: frame, command: command)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.viewingPlan !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.viewingPlan(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.viewingPlanSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.spaceSettingsAction = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.spaceSettingsSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try SpaceLightingViewportRequest(frame: frame, command: command)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.spaceSettings !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.spaceSettings(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.spaceSettingsSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.lightObjectAction = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.lightObjectSupported,
                      self.controller.editingContextActive, self.placementIntent == nil, !self.inspectorPending,
                      let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try LightObjectRequest(command: command, frame: frame)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.lightObject !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.lightObject(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.lightObjectSupported, self.controller.connected,
                      self.controller.editingContextActive, self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.lightInspectorAction = { [weak self] command, selectedLightId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.lightInspectorSupported,
                      !command.usesLightDetails || self.controller.lightFixtureSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try LightViewportRequest(frame: frame, command: command, selectedLightId: selectedLightId)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let script = !command.usesLightDetails
                    ? "if (typeof window.exhibitosViewport?.lightInspector !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.lightInspector(JSON.parse(requestJSON));"
                    : "if (typeof window.exhibitosViewport?.lightDetails !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.lightDetails(JSON.parse(requestJSON));"
                let response = try await web.callAsyncJavaScript(script, arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.lightInspectorSupported, self.controller.connected,
                      !command.usesLightDetails || self.controller.lightFixtureSupported,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes), selectedLightId: selectedLightId)
            }
            controller.wallFinishAction = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.wallFinishSupported,
                      self.controller.editingContextActive, self.placementIntent == nil, !self.inspectorPending,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try WallFinishViewportRequest(frame: frame, command: command)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let context = self.controller.editingContextGeneration
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.architecturalFinishes !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.architecturalFinishes(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.wallFinishSupported, self.controller.connected,
                      self.controller.editingContextActive, self.controller.editingContextGeneration == context,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.latest?.selection == frame.selection,
                      self.placementIntent == nil, let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.benchInspectorAction = { [weak self] command, selectedBenchId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.benchInspectorSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try BenchViewportRequest(frame: frame, command: command, selectedBenchId: selectedBenchId)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.benchInspector !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.benchInspector(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.benchInspectorSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes), selectedBenchId: selectedBenchId)
            }
            controller.plaqueInspectorAction = { [weak self] command, selectedPlaqueId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.plaqueInspectorSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest,
                      self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try PlaqueViewportRequest(frame: frame, command: command, selectedPlaqueId: selectedPlaqueId)
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.plaqueInspector !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.plaqueInspector(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.plaqueInspectorSupported, self.controller.connected,
                      self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.placementIntent == nil,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 1_100_000 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes), selectedPlaqueId: selectedPlaqueId)
            }
            controller.catalogDropAction = { [weak self] query in
                guard let self, self.active, self.prepared, self.controller.owner == self.epoch,
                      self.controller.editingContextActive, !self.controller.boundedReadOnly, !self.inspectorPending,
                      let web = self.web, let frame = self.latest, self.sent == frame.sequence,
                      self.controller.acceptedSequence == frame.sequence, frame.presentation == .threeD,
                      query.phase == .clear || !frame.disabled && frame.draft == query.draft,
                      query.phase == .clear || self.placementIntent?.requestId == query.placementRequestID else {
                    throw AuthoringFailure("VIEWPORT_FRAME_STALE")
                }
                let generation = self.generation, context = self.controller.editingContextGeneration
                let requestID = UUID().uuidString
                let json = String(decoding: try JSONEncoder().encode(query.value(frame: frame, requestID: requestID)), as: UTF8.self)
                CatalogDropTrace.record("webKitDispatch", "phase=\(query.phase.rawValue) kind=\(query.kind.rawValue)")
                let response = try await web.callAsyncJavaScript("return await window.exhibitosViewport.catalogDropTarget(JSON.parse(requestJSON));",
                    arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.controller.owner == self.epoch, self.generation == generation,
                      self.controller.editingContextActive, self.controller.editingContextGeneration == context,
                      self.latest?.sequence == frame.sequence, self.controller.acceptedSequence == frame.sequence,
                      let response, JSONSerialization.isValidJSONObject(response) else {
                    throw AuthoringFailure("VIEWPORT_FRAME_STALE")
                }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 4096 else { throw AuthoringFailure("CATALOG_DROP_INVALID") }
                let hit = try query.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes), frame: frame, requestID: requestID)
                CatalogDropTrace.record("webKitReply", "phase=\(query.phase.rawValue) hit=\(hit != nil)")
                return hit
            }
            controller.placementAction = { [weak self] intent, phase, target in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.placementSupported, !self.inspectorPending,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, frame.draft == intent.base else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let request = try ViewportPlacementRequest(frame: frame, intent: intent, phase: phase, target: target)
                if phase == .begin {
                    guard self.placementIntent == nil else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                    self.placementIntent = intent
                } else {
                    guard self.placementIntent?.requestId == intent.requestId else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                let generation = self.generation
                if phase == .cancel { self.placementIntent = nil }
                do {
                    let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                    let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.placement !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.placement(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                    try Task.checkCancellation()
                    guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence,
                          phase == .cancel || self.placementIntent?.requestId == intent.requestId,
                          let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                    let bytes = try JSONSerialization.data(withJSONObject: response)
                    guard bytes.count <= 16384 else { throw AuthoringFailure("PLACEMENT_TARGET_INVALID") }
                    let placement = try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
                    if phase == .commit { self.placementIntent = nil }
                    return placement
                } catch {
                    if phase != .cancel, self.placementIntent?.requestId == intent.requestId {
                        // An invalid/lost acknowledgement must not leave a ghost
                        // after native refuses the operation. A changed frame
                        // already invalidates its own web operation.
                        if self.generation == generation, self.latest?.sequence == frame.sequence,
                           let cancel = try? ViewportPlacementRequest(frame: frame, intent: intent, phase: .cancel),
                           let bytes = try? JSONEncoder().encode(cancel.value) {
                            _ = try? await web.callAsyncJavaScript("return await window.exhibitosViewport?.placement(JSON.parse(requestJSON));", arguments: ["requestJSON": String(decoding: bytes, as: UTF8.self)], in: nil, contentWorld: .page)
                        }
                        self.endPlacement()
                    }
                    throw error
                }
            }
            controller.furnishingThumbnailAction = { [weak self] in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.furnishingVisualsSupported,
                      self.placementIntent == nil, !self.inspectorPending, let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, !self.parent.interactionDisabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                self.inspectorPending = true
                defer { self.inspectorPending = false }
                let generation = self.generation
                let request = try FurnishingThumbnailRequest(frame: frame)
                let value = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.furnishingThumbnail !== 'function') throw new Error('CATALOG_PREVIEW_UNAVAILABLE'); return await window.exhibitosViewport.furnishingThumbnail(JSON.parse(requestJSON));", arguments: ["requestJSON": value], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.prepared, self.controller.connected, self.controller.furnishingVisualsSupported, self.placementIntent == nil, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, !self.parent.interactionDisabled,
                      let fields = response as? [String: Any], let png = fields["pngBase64"] as? String, png.utf8.count <= 699_052,
                      JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: fields)
                guard bytes.count <= 710_000 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                return try request.pngData(JSONDecoder().decode(DocumentValue.self, from: bytes))
            }
            controller.sceneSnapshotAction = { [weak self] saved, camera, placementId in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.sceneSnapshotSupported,
                      !camera.isInspection || self.controller.sceneSnapshotInspectionSupported,
                      !self.inspectorPending, self.placementIntent == nil, self.entranceOperation == nil,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, !self.parent.interactionDisabled else { throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY") }
                self.inspectorPending = true; defer { self.inspectorPending = false }
                let generation = self.generation
                let request = try SceneSnapshotViewportRequest(frame: frame, saved: saved, camera: camera, placementId: placementId)
                let json = String(decoding: try JSONEncoder().encode(request.value), as: UTF8.self)
                guard let baseRule = self.resourceRule else { throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY") }
                let rules = try ViewportResourcePolicy.encodedRules(origin: self.parent.credentials.origin, tenantId: frame.tenantId, savedExhibitionId: saved.draft.id, privateFloor: self.parent.privateFloor, workspaceMaterials: self.parent.workspaceMaterials, workspaceMaterialMultimap: self.parent.workspaceMaterialMultimap, workspaceFurnishings: self.parent.workspaceFurnishings)
                let identifier = "ExhibitOSSnapshot-" + UUID().uuidString
                let store = WKContentRuleListStore.default()!
                let snapshotRule = try await store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: rules)
                try? await store.removeContentRuleList(forIdentifier: identifier)
                guard let snapshotRule, self.active, self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.latest?.draft == saved.draft,
                      self.resourceRule === baseRule, !self.parent.interactionDisabled else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
                let content = web.configuration.userContentController
                try Task.checkCancellation()
                let policyId = UUID()
                self.snapshotPolicy = (policyId, content, baseRule, snapshotRule)
                defer {
                    // Restore even after cancellation or host invalidation. This
                    // retained web view must never be left without its blockers.
                    self.restoreSnapshotPolicy(policyId)
                }
                content.add(snapshotRule)
                guard self.snapshotPolicy?.id == policyId, self.active, self.generation == generation,
                      self.latest?.sequence == frame.sequence else {
                    content.remove(snapshotRule)
                    throw AuthoringFailure("SCENE_SNAPSHOT_STALE")
                }
                content.remove(baseRule)
                let response: Any?
                do {
                    response = try await withTaskCancellationHandler {
                        try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.sceneSnapshot !== 'function') throw new Error('SCENE_SNAPSHOT_NOT_READY'); return await window.exhibitosViewport.sceneSnapshot(JSON.parse(requestJSON));", arguments: ["requestJSON": json], in: nil, contentWorld: .page)
                    } onCancel: { [weak self] in
                        Task { @MainActor in self?.restoreSnapshotPolicy(policyId) }
                    }
                } catch {
                    if SceneSnapshotViewportRequest.isNotReadyJavaScriptError(error) { throw AuthoringFailure("SCENE_SNAPSHOT_NOT_READY") }
                    throw error
                }
                try Task.checkCancellation()
                guard self.active, self.controller.owner == self.epoch, self.generation == generation,
                      self.latest?.sequence == frame.sequence, self.latest?.draft == saved.draft, !self.parent.interactionDisabled,
                      let fields = response as? [String: Any], let png = fields["pngBase64"] as? String,
                      png.utf8.count <= SceneSnapshotViewportRequest.maximumBase64Bytes,
                      JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("SCENE_SNAPSHOT_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: fields)
                guard bytes.count <= SceneSnapshotViewportRequest.maximumReplyBytes else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
                let result = try request.checkedReply(JSONDecoder().decode(DocumentValue.self, from: bytes))
                guard let image = CGImageSourceCreateWithData(result.png as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
                      CGImageSourceGetCount(image) == 1, let decoded = CGImageSourceCreateImageAtIndex(image, 0, nil),
                      decoded.width == SceneSnapshotViewportRequest.width, decoded.height == SceneSnapshotViewportRequest.height else { throw AuthoringFailure("SCENE_SNAPSHOT_INVALID") }
                return result
            }
            controller.registeredFurnishingThumbnailAction = { [weak self] item, draft in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.furnishingThumbnailSupported,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !self.parent.interactionDisabled,
                      self.controller.acknowledgesFurnishingThumbnail(draft: draft, tenantId: frame.tenantId, principalId: frame.principalId) else {
                    throw AuthoringFailure("VIEWPORT_FRAME_STALE")
                }
                let generation = self.generation, context = self.controller.editingContextGeneration
                let request = try ViewportFurnishingThumbnailRequest(frame: frame, furnishing: item)
                let value = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("return await window.exhibitosViewport.furnishingThumbnailV1(JSON.parse(requestJSON));",
                    arguments: ["requestJSON": value], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.generation == generation, self.controller.editingContextGeneration == context,
                      self.controller.owner == self.epoch, self.latest?.sequence == frame.sequence,
                      self.controller.acceptedSequence == frame.sequence, !self.parent.interactionDisabled,
                      self.controller.acknowledgesFurnishingThumbnail(draft: draft, tenantId: frame.tenantId, principalId: frame.principalId) else {
                    throw AuthoringFailure("VIEWPORT_FRAME_STALE")
                }
                let fields: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId",
                    "furnishingId", "furnishingVersion", "modelSha256", "modelBytes", "recipeId", "toolId", "pngBase64", "pngSha256", "width", "height"]
                guard let reply = response as? [String: Any], Set(reply.keys) == fields,
                      let png = reply["pngBase64"] as? String, png.utf8.count <= 699_052,
                      JSONSerialization.isValidJSONObject(reply) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                let bytes = try JSONSerialization.data(withJSONObject: reply)
                guard bytes.count <= 710_000 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                return (request, try JSONDecoder().decode(ViewportFurnishingThumbnailReply.self, from: bytes).pngData(matching: request))
            }
            controller.thumbnailAction = { [weak self] artworkId, revision in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.thumbnailSupported,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, !self.parent.interactionDisabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                let generation = self.generation
                let request = try ViewportThumbnailRequest(frame: frame, artworkId: artworkId, expectedRevision: revision)
                let value = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.thumbnail !== 'function') throw new Error('CATALOG_PREVIEW_UNAVAILABLE'); return await window.exhibitosViewport.thumbnail(JSON.parse(requestJSON));", arguments: ["requestJSON": value], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, !self.parent.interactionDisabled,
                      let fields = response as? [String: Any], let png = fields["pngBase64"] as? String, png.utf8.count <= 699_052,
                      JSONSerialization.isValidJSONObject(fields) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: fields)
                guard bytes.count <= 710_000 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
                return (try JSONDecoder().decode(ViewportThumbnailReply.self, from: bytes), request)
            }
            controller.findSelectionAction = { [weak self] in
                guard let self, self.active, self.controller.owner == self.epoch, self.controller.findSelectionSupported,
                      let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence,
                      !frame.disabled, !self.parent.interactionDisabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                let generation = self.generation, request = try ViewportFindSelectionRequest(frame: frame)
                let value = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
                let response = try await web.callAsyncJavaScript("if (typeof window.exhibitosViewport?.findSelection !== 'function') throw new Error('VIEWPORT_NOT_READY'); return await window.exhibitosViewport.findSelection(JSON.parse(requestJSON));", arguments: ["requestJSON": value], in: nil, contentWorld: .page)
                try Task.checkCancellation()
                guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, !self.parent.interactionDisabled,
                      let response, JSONSerialization.isValidJSONObject(response) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                let bytes = try JSONSerialization.data(withJSONObject: response)
                guard bytes.count <= 2048, try JSONDecoder().decode(ViewportFindSelectionReply.self, from: bytes).matches(request) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
            }
            controller.action = { [weak self] command in
                guard let self, self.active, self.controller.owner == self.epoch, !self.inspectorPending, self.placementIntent == nil, let web = self.web, let frame = self.latest, self.prepared, self.sent == frame.sequence, !frame.disabled else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                if case .surfaceMaterialTexture = command {
                    guard self.controller.surfaceMaterialTexturesSupported else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                }
                if case .roomBaseLight = command {
                    guard frame.workspaceId == nil, !self.parent.interactionDisabled, self.controller.roomBaseLightSupported,
                          self.controller.acknowledgedFrame?.sequence == frame.sequence else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                }
                if case .installation(.alignOnVerifiedPlinth(_)) = command {
                    guard self.controller.plinthEnvelopeAlignSupported else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
                }
                let generation = self.generation
                let value = String(decoding: try JSONEncoder().encode(command.encodedValue(in: frame.draft, selected: frame.selection)), as: UTF8.self)
                let candidate = try await web.callAsyncJavaScript("try { return window.exhibitosViewport.action(JSON.parse(actionJSON)); } catch (error) { if (error.code === 'INVALID_GEOMETRY' && Array.isArray(error.issues)) { const codes = error.issues.slice(0, 1).map(issue => issue.code + '_' + String(issue.path).replace(/[^a-zA-Z0-9_]/g, '_').toUpperCase() + '_' + (/^[a-zA-Z]{1,30}$/.test(issue.message) ? issue.message.toUpperCase() : '')).filter(code => /^[A-Z0-9_]{1,150}$/.test(code)); throw new Error('INVALID_GEOMETRY_' + codes.join('_')); } throw error; }", arguments: ["actionJSON":value], in: nil, contentWorld: .page)
                guard self.active, self.controller.owner == self.epoch, self.generation == generation, self.latest?.sequence == frame.sequence, let candidate, JSONSerialization.isValidJSONObject(candidate) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                if case .installation(.alignOnVerifiedPlinth(_)) = command {
                    guard self.controller.connected, self.controller.editingContextActive, self.controller.plinthEnvelopeAlignSupported else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                if case .roomBaseLight = command {
                    guard self.controller.connected, self.controller.roomBaseLightSupported, !self.parent.interactionDisabled,
                          self.controller.acknowledgedFrame?.sequence == frame.sequence else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
                }
                let bytes = try JSONSerialization.data(withJSONObject: candidate)
                guard bytes.count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
                return (try JSONDecoder().decode(DocumentValue.self, from: bytes), frame.draft)
            }
        }
        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
            guard active, controller.owner == epoch, message.webView === web, message.frameInfo.isMainFrame, isPage(web?.url), message.frameInfo.securityOrigin.protocol == "http", message.frameInfo.securityOrigin.host == "127.0.0.1", message.frameInfo.securityOrigin.port == page?.port,
                  let fields = message.body as? [String: Any], Set(fields.keys) == Set(["version", "epoch", "sequence", "type", "body"]),
                  JSONSerialization.isValidJSONObject(message.body), let bytes = try? JSONSerialization.data(withJSONObject: message.body), bytes.count <= 1_100_000,
                  let envelope = try? JSONDecoder().decode(Envelope.self, from: bytes), envelope.version == 1, envelope.epoch == epoch, envelope.sequence == sequence, let frame = latest else { return }
            switch envelope.type {
            case "placement-target":
                guard controller.editingContextActive, editingContextGeneration == controller.editingContextGeneration, !frame.disabled, let intent = placementIntent,
                      (try? ViewportPlacementTarget(value: envelope.body, intent: intent)) != nil else { return }
                parent.onPlacementTarget(envelope.body)
            case "placement-cancel":
                guard let intent = placementIntent, case .object(let body) = envelope.body,
                      Set(body.keys) == Set(["placementRequestId", "reason"]), envelope.body["placementRequestId"].string == intent.requestId,
                      let reason = envelope.body["reason"].string, reason.utf8.count <= 160 else { return }
                endPlacement()
            case "selection":
                guard controller.editingContextActive, editingContextGeneration == controller.editingContextGeneration, placementIntent == nil, !frame.disabled, !parent.selectionDisabled, let selection = try? JSONDecoder().decode(ViewportSelection.self, from: JSONEncoder().encode(envelope.body)), selection.belongs(to: frame.draft) else { return }
                if parent.boundedEdit != nil && selection.kind != "placement" { return }
                guard selection.kind != "light" || controller.lightObjectSupported else { return }
                parent.onSelect(selection)
            case "candidate": guard controller.editingContextActive, editingContextGeneration == controller.editingContextGeneration, placementIntent == nil, !frame.disabled, !parent.interactionDisabled else { return }; parent.onCandidate(envelope.body, frame.draft)
            case "status":
                guard let report = try? JSONDecoder().decode(Report.self, from: JSONEncoder().encode(envelope.body)), report.loaded >= 0, report.failed >= 0, report.message.count <= 2048 else { return }
                let total = frame.draft.placements.count
                controller.sceneProgress = ViewportSceneProgress.reported(ready: report.ready, failed: report.error == true,
                    loadedCount: report.loaded, failedCount: report.failed, total: total)
                controller.rendering = report.ready && report.error != true
                controller.loaded = report.loaded; controller.failed = report.failed
                if report.error == true {
                    controller.message = "편집 화면을 준비하지 못했습니다. 편집 화면을 다시 열어 보세요."
                } else if controller.sceneProgress == .preparing {
                    controller.message = "편집 화면을 준비하고 있습니다."
                } else if controller.sceneProgress != nil {
                    controller.message = "전시 작품을 불러오고 있습니다."
                } else if report.ready,
                          report.message == "장면의 표시 한도에 도달해 작업 공간 비품을 표시하지 못했어요. 전시 편집은 계속할 수 있습니다."
                          || report.message == "일부 작업 공간 비품을 표시하지 못했어요. 등록 상태와 표시 권한을 확인하세요." {
                    controller.message = report.failed > 0
                        ? "일부 작품을 불러오지 못했습니다. " + report.message
                        : report.message
                } else {
                    controller.message = report.failed > 0
                        ? "일부 작품을 불러오지 못했습니다. 작업 공간과 작품 사용 권한을 확인하세요."
                        : "전시 편집 화면을 표시했습니다."
                }
            case "error": controller.message = "3D 조작을 적용하지 못했습니다. 현재 전시와 선택은 유지됩니다."
            default: break
            }
        }
        private struct Envelope: Decodable { let version: Int; let epoch: String; let sequence: Int; let type: String; let body: DocumentValue }
        private struct Report: Decodable { let ready: Bool; let message: String; let loaded: Int; let failed: Int; let error: Bool? }
    }
}

struct NativeViewportArea: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    var editable = true
    @ObservedObject var controller: NativeViewportController
    var workbench = false
    var mode: Mode = .beginner
    var catalogDrag: CatalogDragSession?
    @State private var identity = UUID()
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let credentials = model.viewportCredentials, let draft = model.viewportDraft, ready {
                if workbench { workbenchViewTools } else {
                Picker("편집 화면", selection: Binding(get: { controller.planSupported ? model.viewportPresentation : .threeD }, set: { value in
                    if value == .threeD || controller.planSupported { model.viewportPresentation = value }
                })) {
                    Text("3D 공간").tag(ViewportPresentation.threeD)
                    Text("2D 평면").tag(ViewportPresentation.plan).disabled(!controller.planSupported)
                }.pickerStyle(.segmented)
                    .disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
                    .accessibilityHint("같은 전시와 선택을 유지하며 편집 화면을 전환합니다.")
                if !controller.planSupported { Text("이 제작 서비스는 3D 화면을 제공합니다. 2D 평면 지원 연결을 확인하세요.").foregroundStyle(.secondary) }
                HStack {
                    Button("전체 공간") { model.requestViewportView("isometric") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    Button("위에서 보기") { model.requestViewportView("top") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    Button("정면 보기") { model.requestViewportView("front") }.disabled(controller.planSupported && model.viewportPresentation == .plan)
                    if editable {
                        Picker("대상 편집", selection: $model.viewportMode) { Text("대상 이동").tag("translate"); Text("대상 회전").tag("rotate"); Text("대상 크기").tag("dimensions") }
                    }
                }.disabled(model.placingArtwork || model.entrancePreviewActive)
                Button("편집 화면 다시 열기") {
                    if model.entrancePreviewActive { model.recoverEntrancePreviewHost { identity = UUID() } }
                    else { identity = UUID() }
                }.disabled(model.entrancePreviewActive ? !model.canRecoverEntranceHost : (model.placingArtwork || model.busy || model.pending || model.architectureApplying))
                    .help("이 편집 화면만 다시 만듭니다. 입력·선택·이력과 마지막으로 선택한 보기를 유지합니다.")
                Toggle("공간을 투명하게 보기", isOn: $model.viewportTransparent)
                    .disabled(model.placingArtwork || model.entrancePreviewActive)
                    .help("벽·바닥 너머 작품을 확인합니다. 저장한 재질은 바뀌지 않습니다.")
                }
                GeometryReader { geometry in
                    if let catalogDrag {
                        viewportSurface(credentials: credentials, draft: draft)
                            .onDrop(of: [CatalogDragSession.contentType], delegate: CatalogViewportDrop(
                                model: model, controller: controller, session: catalogDrag,
                                surfaceSize: geometry.size, enabled: editable))
                            .onAppear { CatalogDropTrace.record("receiverAttached", "enabled=\(editable)") }
                    } else {
                        viewportSurface(credentials: credentials, draft: draft)
                    }
                }
                    .frame(height: workbench ? nil : (model.entrancePreviewActive ? 740 : 440))
                    .frame(minHeight: workbench ? 120 : nil, maxHeight: workbench ? .infinity : (model.entrancePreviewActive ? 740 : 440))
                if workbench && model.viewportPresentation == .threeD && !model.entrancePreviewActive {
                    switch controller.sceneProgress {
                    case .preparing:
                        ProgressView("편집 화면 준비 중").controlSize(.small).padding(.horizontal, 12)
                    case .artworks(let processed, let total):
                        ProgressView(value: Double(processed), total: Double(total)) {
                            Text("작품 배치 확인 \(processed)/\(total)")
                        } currentValueLabel: {
                            Text("\(Int(100 * Double(processed) / Double(total)))%")
                        }.controlSize(.small).padding(.horizontal, 12)
                    case nil:
                        EmptyView()
                    }
                }
                if !workbench { Text("불러온 작품 \(controller.loaded)개 · 불러오지 못한 작품 \(controller.failed)개").foregroundStyle(.secondary) }
                Text(workbench ? "작품 \(controller.loaded)개 · 불러오기 실패 \(controller.failed)개 · " + controller.message : controller.message).foregroundStyle(.secondary)
                    .font(workbench ? .caption : .body).lineLimit(workbench ? 2 : nil).padding(.horizontal, workbench ? 12 : 0)
                    .onChange(of: controller.acceptedSequence) { _, _ in model.confirmEntranceHostRecovery() }
                    .onChange(of: controller.connected) { _, _ in model.confirmEntranceHostRecovery() }
                if mode == .expert || !controller.diagnostic.isEmpty {
                    DisclosureGroup("3D 연결 상세") {
                        if !controller.diagnostic.isEmpty { Text(controller.diagnostic).textSelection(.enabled) }
                        if mode == .expert {
                            Button(controller.readingDisplay ? "표시 상태 확인 중…" : "표시 상태 확인") {
                                let input = NativeDisplayReadoutInput(draft: draft, selection: model.viewportSelection,
                                    view: model.viewportView, cameraSequence: model.viewportCameraSequence,
                                    transparent: model.viewportTransparent,
                                    presentation: controller.planSupported ? model.viewportPresentation : .threeD)
                                Task { await controller.readDisplayStatus(input) }
                            }.disabled(!controller.connected || !controller.editingContextActive || controller.readingDisplay || model.entrancePreviewActive)
                            Text("요청·맵 상태 정보입니다. 현재 카메라 좌표나 외관 수락을 뜻하지 않습니다.").font(.caption).foregroundStyle(.secondary)
                            ScrollView {
                                Text(controller.displayReadout.isEmpty ? "현재 표시 상태를 확인하세요." : controller.displayReadout)
                                    .font(.caption.monospaced()).textSelection(.enabled)
                            }.frame(height: 120)
                        }
                    }
                }
                if !workbench {
                if editable {
                    HStack {
                        Button("벽과 바닥 만들기") { Task { await controller.perform(.whiteCube(roomId: model.roomId), model: model) } }
                        roomAddControls
                        Button("조명 추가") { Task { await controller.perform(.addLight(roomId: model.roomId), model: model) } }
                    }.disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
                    Text("숫자 입력은 이 Mac에 보관한 뒤 3D에 반영됩니다. 3D 변경도 이 Mac에 보관하며, 전시 저장과 공개는 별도로 진행합니다.").foregroundStyle(.secondary)
                }
                NativeLightPicker(model: model, ready: ready).disabled(model.entrancePreviewActive)
                NativeBenchPicker(model: model, ready: ready).disabled(model.entrancePreviewActive)
                NativePlaquePicker(model: model, ready: ready).disabled(model.entrancePreviewActive)
                }
            } else { Text("전시를 열고 로컬 제작을 시작하면 3D를 볼 수 있습니다.").foregroundStyle(.secondary) }
        }
    }
    @ViewBuilder private var roomAddControls: some View {
        Button("공간 추가") {
            let action: NativeGeometryAction = model.canUseRoomBaseLight
                ? .roomBaseLight(.addRoom(name: "새 공간"))
                : .addRoom(name: "새 공간")
            Task { await controller.perform(action, model: model) }
        }
        if model.canUseRoomBaseLight {
            Button("기본 조명 적용") {
                let action = RoomBaseLightAction.ensure(roomId: model.roomId)
                Task { await controller.perform(.roomBaseLight(action), model: model) }
            }.disabled(!model.canEnsureRoomBaseLight)
        }
    }

    private func viewportSurface(credentials: LocalAuthoringSession, draft: NativeExhibitionDraft) -> some View {
        GeometryReader { geometry in
            let width = model.entrancePreviewActive ? (model.entrancePreviewDevice == .mobile ? 360.0 : 1024.0) : max(1, geometry.size.width)
            let height = model.entrancePreviewActive ? 740.0 : max(1, workbench ? geometry.size.height : 440)
            let fit = workbench && model.entrancePreviewActive ? min(1, geometry.size.width / width, geometry.size.height / height) : 1
            if workbench {
                ZStack {
                    renderer(credentials: credentials, draft: draft)
                        .frame(width: width, height: height)
                        .scaleEffect(fit, anchor: .topLeading)
                        .frame(width: width * fit, height: height * fit, alignment: .topLeading)
                }.frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            } else {
                ScrollView(.horizontal) { renderer(credentials: credentials, draft: draft).frame(width: width, height: height) }
            }
        }
    }
    private func rendererIdentity(_ credentials: LocalAuthoringSession) -> String {
        // The view and its identity must use the same credential snapshot. The
        // model access UUID fences other tasks but is published separately.
        // Only a one-way digest enters SwiftUI identity; never the bearer token.
        let fields = [credentials.origin, credentials.tenantId.lowercased(), credentials.principalId.lowercased(), credentials.token]
        let digest = SHA256.hash(data: Data(fields.joined(separator: "\0").utf8))
        return identity.uuidString + digest.map { String(format: "%02x", $0) }.joined()
    }
    private func renderer(credentials: LocalAuthoringSession, draft: NativeExhibitionDraft) -> some View {
        let selectionBlocked = !editable || model.entrancePreviewActive || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.surfaceMaterialPreviewing || model.lightPreviewing || model.benchPreviewing || model.plaquePreviewing || model.furnishingPreviewing || model.spaceLightingPreviewing || model.infoStandPreviewing || model.placingArtwork
        let workspaceFurnishings = model.workspaceFurnishingIntakeAvailable
        let workspaceMaterialMultimap = model.workspaceMaterialMultimapAvailable
        return NativeViewport(credentials: credentials, privateFloor: model.privateFloorRuntimeAvailable, workspaceMaterials: model.workspaceMaterialPreviewAvailable, workspaceMaterialMultimap: workspaceMaterialMultimap, workspaceFurnishings: workspaceFurnishings, draft: draft, selection: model.viewportSelection, presentation: model.viewportPresentation, mode: model.viewportMode, view: model.viewportView, cameraSequence: model.viewportCameraSequence, transparent: model.viewportTransparent, disabled: !editable || model.entrancePreviewActive || model.busy || model.pending || model.viewportFormDirty || model.positionDirty || model.wallFinishPreviewing || model.surfaceMaterialPreviewing || model.lightPreviewing || model.benchPreviewing || model.plaquePreviewing || model.furnishingPreviewing || model.spaceLightingPreviewing || model.infoStandPreviewing, interactionDisabled: selectionBlocked || NativeViewportAccess.boundedInteractionBlocked(boundedAdmission: model.boundedPlacementCapability != nil, acknowledged: model.boundedPlacementReady), selectionDisabled: selectionBlocked, boundedEdit: model.boundedPlacementCapability, acceptBoundedAck: { model.acceptBoundedPlacementFrame($0, $1) }, controller: controller, onSelect: { if controller.editingContextActive { model.selectViewport($0) } }, onCandidate: { candidate, base in let context = controller.editingContextGeneration; Task { guard controller.editingContextActive, controller.editingContextGeneration == context else { return }; await model.acceptViewportCandidate(candidate, base: base) } }, onPlacementTarget: { model.receivePlacementTarget($0) }, onPlacementCancel: { model.placementCancelled($0) })
            .id(rendererIdentity(credentials) + String(model.privateFloorRuntimeAvailable) + String(model.workspaceMaterialPreviewAvailable) + String(workspaceMaterialMultimap) + String(workspaceFurnishings))
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .allowsHitTesting(!model.architectureDirty && !model.architectureApplying && !model.placementApplying)
            .disabled(model.architectureDirty || model.architectureApplying)
    }
    private var workbenchViewTools: some View {
        HStack(spacing: 8) {
            Picker("편집 화면", selection: Binding(get: { controller.planSupported ? model.viewportPresentation : .threeD }, set: { value in
                if value == .threeD || controller.planSupported { model.viewportPresentation = value }
            })) {
                Text("3D 공간").tag(ViewportPresentation.threeD)
                Text("2D 평면").tag(ViewportPresentation.plan).disabled(!controller.planSupported)
            }.pickerStyle(.segmented).frame(maxWidth: 220)
                .disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
                .accessibilityHint("같은 전시와 선택을 유지하며 편집 화면을 전환합니다.")
            if editable {
                Picker("대상 편집", selection: $model.viewportMode) { Text("대상 이동").tag("translate"); Text("대상 회전").tag("rotate"); Text("대상 크기").tag("dimensions") }
                    .labelsHidden().fixedSize().disabled(model.placingArtwork || model.entrancePreviewActive)
            }
            Spacer(minLength: 0)
            Menu {
                Button("전체 공간") { model.requestViewportView("isometric") }.disabled(model.placingArtwork || model.entrancePreviewActive || (controller.planSupported && model.viewportPresentation == .plan))
                Button("위에서 보기") { model.requestViewportView("top") }.disabled(model.placingArtwork || model.entrancePreviewActive || (controller.planSupported && model.viewportPresentation == .plan))
                Button("정면 보기") { model.requestViewportView("front") }.disabled(model.placingArtwork || model.entrancePreviewActive || (controller.planSupported && model.viewportPresentation == .plan))
                Toggle("공간을 투명하게 보기", isOn: $model.viewportTransparent).disabled(model.placingArtwork || model.entrancePreviewActive)
                Divider()
                Button("편집 화면 다시 열기") {
                    if model.entrancePreviewActive { model.recoverEntrancePreviewHost { identity = UUID() } }
                    else { identity = UUID() }
                }.disabled(model.entrancePreviewActive ? !model.canRecoverEntranceHost : (model.placingArtwork || model.busy || model.pending || model.architectureApplying))
            } label: { Label("보기", systemImage: "eye") }
                .menuStyle(.borderlessButton).fixedSize()
            if editable {
                Menu {
                    Button("벽과 바닥 만들기") { Task { await controller.perform(.whiteCube(roomId: model.roomId), model: model) } }
                    roomAddControls
                    Button("조명 추가") { Task { await controller.perform(.addLight(roomId: model.roomId), model: model) } }
                    Button("벤치 추가") { model.beginBench() }.disabled(!model.canAddBench)
                    Button("명판 추가") { model.beginPlaque() }.disabled(!model.canAddPlaque)
                } label: { Label("추가", systemImage: "plus") }
                    .menuStyle(.borderlessButton).fixedSize()
                    .disabled(model.entrancePreviewActive || !controller.connected || model.busy || model.pending || model.formDirty || model.positionDirty || model.architectureDirty || model.architectureApplying || model.placingArtwork)
            }
        }.controlSize(.small).padding(.horizontal, 12).padding(.top, 8)
    }

}
