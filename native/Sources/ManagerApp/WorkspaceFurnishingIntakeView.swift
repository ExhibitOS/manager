// SPDX-License-Identifier: Apache-2.0
import ManagerCore
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct WorkspaceFurnishingIntakeView: View {
    @ObservedObject var model: ExhibitionModel
    let access: UUID
    let initialWorkspaceFurnishingID: String?
    let initialBundledID: String?
    let onRegistered: (WorkspaceFurnishing) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var kind = WorkspaceFurnishingKind.pedestal
    @State private var name = ""
    @State private var provenance = ""
    @State private var reusablePortal = false
    @State private var rights = {
        var value = ArtworkMetadata().rights
        value.licenseText = nil
        value.permissions.display = false
        return value
    }()
    @State private var snapshot: ImportFileSnapshot?
    @State private var bundledCatalog: BundledWorkspaceFurnishings.Catalog?
    @State private var choosing = false
    @State private var busyActivity: String?
    @State private var message: String?
    struct PendingRequest {
        let access: UUID
        let id: String
        let details: WorkspaceFurnishingDetails
        let policy: WorkspaceArtworkFramePreparationPolicy?
        let fitProfile: WorkspaceFurnishingFitProfile?
    }
    @Binding private var pendingRequest: PendingRequest?
    private var request: PendingRequest? {
        get { pendingRequest?.access == access ? pendingRequest : nil }
        nonmutating set { pendingRequest = newValue }
    }
    @State private var selected: WorkspaceFurnishingIntake?
    @State private var intakes: [WorkspaceFurnishingIntake] = []
    @State private var intakeCursor: String?
    @State private var target: WorkspaceFurnishingSummary?
    @State private var reviewedInfoStand: (furnishing: WorkspaceFurnishing, intakeId: String)?
    @State private var room = ""
    @State private var portalOpening = ""
    @State private var x = "0"
    @State private var y = "0"
    @State private var z = "0"
    @State private var yaw = "0"
    @State private var scale = "1"
    @State private var placementId = UUID().uuidString.lowercased()
    @State private var placementStaged = false
    @State private var seatPolicyETag: String?
    private var busy: Bool { busyActivity != nil }

    init(model: ExhibitionModel, access: UUID, initialKind: WorkspaceFurnishingKind? = nil,
         initialWorkspaceFurnishingID: String? = nil, initialBundledID: String? = nil,
         pendingRequest: Binding<PendingRequest?>, onRegistered: @escaping (WorkspaceFurnishing) -> Void = { _ in }) {
        self.model = model
        self.access = access
        self.initialWorkspaceFurnishingID = initialWorkspaceFurnishingID
        self.initialBundledID = initialBundledID
        self.onRegistered = onRegistered
        _pendingRequest = pendingRequest
        _kind = State(initialValue: initialKind.flatMap { Self.supports($0, model: model) ? $0 : nil } ?? .pedestal)
    }

    static func supports(_ kind: WorkspaceFurnishingKind, model: ExhibitionModel) -> Bool {
        switch kind {
        case .pedestal, .bench: model.workspaceFurnishingIntakeAvailable
        case .stool: model.workspaceFurnishingStoolAvailable
        case .partition, .infoStand, .barrier, .displayCase: model.workspaceFurnishingRoomVisualAvailable
        case .artworkFrame: model.workspaceArtworkFrameAvailable
        case .portalFinish: model.workspacePortalFinishAvailable
        }
    }

    private var adding: Bool { initialWorkspaceFurnishingID == nil }
    private var canInteract: Bool { !busy && access == model.viewportAccessID }
    private var registrationInputComplete: Bool {
        func filled(_ value: String?) -> Bool { value?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false }
        return snapshot != nil && filled(name) && filled(provenance) && filled(rights.holder)
            && (filled(rights.licenseId) || filled(rights.licenseText))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(adding ? "비품 추가" : (target?.name ?? "비품 배치"))
                    .font(.title2.weight(.semibold)).accessibilityAddTraits(.isHeader)
                Text(adding ? "작업 공간에 GLB를 등록합니다." : "현재 전시에서 비품의 배치와 연결을 설정합니다.")
                    .foregroundStyle(.secondary)
            }.padding(.horizontal, 20).padding(.vertical, 14)
            Divider()
            Form {
                if adding {
                    if let selected { intakeSection(selected) }
                    else if let request {
                        Section("확인이 필요한 등록") {
                            Text(request.details.name).fontWeight(.semibold)
                            Text("같은 요청으로 서버의 등록 상태를 확인할 수 있습니다.").foregroundStyle(.secondary)
                        }
                    } else { newSection }
                    registrationHistory
                } else if let target {
                    switch target.kind {
                    case .artworkFrame: artworkFrameSection
                    case .portalFinish: portalSection
                    default: placementSection(target)
                    }
                    if target.kind == .bench, let seatPolicyETag, seatPolicyETag == model.state?.etag {
                        WorkspaceBenchSeatEditor(model: model, entry: target, access: access).id(target.id)
                    }
                } else if placementStaged {
                    Section {
                        Label {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("배치 입력을 보관했습니다.").font(.headline)
                                Text("편집 화면으로 돌아가 전시를 저장하세요.").foregroundStyle(.secondary)
                            }
                        } icon: { Image(systemName: "checkmark.circle") }
                    }
                } else if !busy, message == nil {
                    Text("하단 카탈로그에서 배치할 비품을 선택하세요.").foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
            .disabled(!canInteract)
            Divider()
            VStack(alignment: .leading, spacing: 8) {
                if access == model.viewportAccessID {
                    if let busyActivity {
                        ProgressView(busyActivity).controlSize(.small)
                    } else if adding, let selected, [.validating, .cancelling].contains(selected.phase) {
                        ProgressView(status(selected.phase)).controlSize(.small)
                    }
                }
                if let message { Text(message).accessibilityAddTraits(.updatesFrequently) }
                HStack {
                    Spacer()
                    if placementStaged {
                        Button("편집 화면으로 돌아가기") { dismiss() }
                            .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    } else {
                        Button("닫기", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction).disabled(busy)
                        if adding { primaryRegistrationAction.disabled(!canInteract) }
                    }
                }
            }.padding(.horizontal, 20).padding(.vertical, 12)
        }
        .frame(minWidth: 530, idealWidth: 570, minHeight: 510, idealHeight: 590)
        .interactiveDismissDisabled(busy)
        .fileImporter(isPresented: $choosing, allowedContentTypes: [UTType(filenameExtension: "glb") ?? .data]) { result in
            Task {
                await perform {
                    let file = try await ImportFileSnapshot.load(result.get(), byteLimit: WorkspaceFurnishingModel.byteLimit)
                    let descriptor = try WorkspaceFurnishingModel(snapshot: file)
                    if let selected, selected.declaration.model != descriptor { throw AuthoringFailure("MODEL_CONFLICT") }
                    snapshot = file
                }
            }
        }
        .task {
            if let catalog = try? await Task.detached(priority: .userInitiated, operation: { try BundledWorkspaceFurnishings.catalog() }).value,
               !Task.isCancelled, access == model.viewportAccessID {
                bundledCatalog = catalog
            }
            if request == nil, let initialBundledID, let item = bundledCatalog?.items.first(where: { $0.id == initialBundledID }) {
                await selectBundled(item)
            }
            await reload(revealing: initialWorkspaceFurnishingID)
        }
    }

    @ViewBuilder private var newSection: some View {
        Section("모델 파일") {
            HStack {
                Label {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(snapshot?.name ?? "선택한 파일 없음")
                            .lineLimit(1).truncationMode(.middle).help(snapshot?.name ?? "")
                        if let snapshot {
                            Text(ByteCountFormatter.string(fromByteCount: Int64(snapshot.data.count), countStyle: .file))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                } icon: { Image(systemName: "cube") }
                Spacer()
                Button(snapshot == nil ? "GLB 선택" : "GLB 바꾸기") { choosing = true }
            }
            if let bundledCatalog, !bundledCatalog.items.isEmpty {
                Menu("기본 비품 선택") {
                    ForEach(bundledCatalog.items) { item in
                        Button(item.name) { Task { await selectBundled(item) } }
                            .disabled(item.kind.map { !Self.supports($0, model: model)
                                || ($0 == .portalFinish && !model.workspacePortalRectangularFitAvailable) } ?? true)
                    }
                }
            }
        }
        Section("비품 정보") {
            Picker("종류", selection: $kind) {
                Text("받침").tag(WorkspaceFurnishingKind.pedestal)
                Text("벤치").tag(WorkspaceFurnishingKind.bench)
                if model.workspaceFurnishingStoolAvailable {
                    Text("스툴").tag(WorkspaceFurnishingKind.stool)
                }
                if model.workspaceFurnishingRoomVisualAvailable {
                    Text("칸막이").tag(WorkspaceFurnishingKind.partition)
                    Text("안내 스탠드").tag(WorkspaceFurnishingKind.infoStand)
                    Text("차단대").tag(WorkspaceFurnishingKind.barrier)
                    Text("진열장").tag(WorkspaceFurnishingKind.displayCase)
                }
                if model.workspaceArtworkFrameAvailable {
                    Text("작품 액자").tag(WorkspaceFurnishingKind.artworkFrame)
                }
                if model.workspacePortalFinishAvailable {
                    Text("출입구 마감").tag(WorkspaceFurnishingKind.portalFinish)
                }
            }
            if kind == .portalFinish && model.workspacePortalRectangularFitAvailable {
                Toggle("다른 크기의 출입구에도 맞추기", isOn: $reusablePortal)
                Text("텍스처 없는 사각 프레임을 지원합니다. 원본은 유지되며, 등록할 때 재사용 가능 여부를 확인합니다.")
                    .foregroundStyle(.secondary)
            }
            TextField("이름", text: $name)
            TextField("출처", text: $provenance, axis: .vertical)
            if let snapshot, let descriptor = try? WorkspaceFurnishingModel(snapshot: snapshot),
               preparationPolicy(for: descriptor) != nil {
                Text("등록할 때 서버에서 표시용 액자 모델을 만들고 원본도 보관합니다.").foregroundStyle(.secondary)
            }
            if kind == .infoStand && model.workspaceInfoStandPreparationAvailable {
                Text("권리자가 등록한 안내판은 원본을 보관합니다. 호환 처리가 필요한 경우 표시용 GLB를 별도로 만들며, 배치하기 전에 등록 모델을 확인할 수 있습니다.")
                    .foregroundStyle(.secondary)
            }
        }
        Section("이용 권리") {
            TextField("권리자", text: $rights.holder)
            Picker("권리 근거", selection: $rights.ownership) {
                Text("내가 권리자").tag("owner")
                Text("허가를 받음").tag("authorized-licensee")
            }
            TextField("라이선스 ID", text: optional($rights.licenseId))
            TextField("허용 조건", text: optional($rights.licenseText))
            Toggle("전시 허용", isOn: $rights.permissions.display)
            DisclosureGroup("크레딧과 추가 권리 설정") {
                TextField("표시할 크레딧", text: $rights.creditLine)
                TextField("권리 시작일 (ISO 8601)", text: optional($rights.validFrom))
                TextField("권리 만료일 (ISO 8601)", text: optional($rights.expiresAt))
                Toggle("다운로드 허용", isOn: $rights.permissions.download)
                Toggle("내보내기 허용", isOn: $rights.permissions.export)
                Toggle("상업적 사용 허용", isOn: $rights.permissions.commercial)
            }
        }
    }

    private var registrationHistory: some View {
        Section {
            DisclosureGroup("등록 기록") {
                if intakes.isEmpty { Text("등록 기록이 없습니다.").foregroundStyle(.secondary) }
                ForEach(intakes) { intake in
                    Button("\(intake.declaration.name) · \(status(intake.phase))") {
                        selected = intake
                        snapshot = nil
                    }
                    .disabled(request != nil || snapshot != nil || selected.map(isUnfinished) == true)
                }
                if intakeCursor != nil { Button("등록 기록 더 보기") { Task { await loadMoreIntakes() } } }
                Button("등록 기록 새로 고침") { Task { await reload() } }
            }
        }
    }

    private func isUnfinished(_ intake: WorkspaceFurnishingIntake) -> Bool {
        intake.phase != .registered && intake.phase != .cancelled
    }

    @ViewBuilder private var primaryRegistrationAction: some View {
        if let selected {
            switch selected.phase {
            case .uploading:
                Button("등록 이어서 진행") { Task { await submit() } }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(selected.received == nil && snapshot == nil)
            case .validating, .cancelling:
                Button("상태 확인") { Task { await change(nil) } }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
            case .failed:
                if selected.retryable {
                    Button("검증 다시 시도") { Task { await change(.retry) } }
                        .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                }
            case .registered, .cancelled:
                Button("새 비품 등록") { self.selected = nil; snapshot = nil }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
            }
        } else if request != nil {
            Button("같은 요청 다시 확인") { Task { await submit() } }
                .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
        } else {
            Button("등록 시작") { Task { await submit() } }
                .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(!registrationInputComplete)
        }
    }

    private func intakeSection(_ intake: WorkspaceFurnishingIntake) -> some View {
        Section(intake.declaration.name) {
            if intake.phase == .uploading {
                let received = intake.received?.bytes ?? 0
                let declared = intake.declaration.model.bytes
                ProgressView(value: Double(received), total: Double(declared)) {
                    Text("GLB 파일 수신 확인")
                } currentValueLabel: {
                    let fraction = Double(received) / Double(declared)
                    Text("\(received.formatted()) / \(declared.formatted())바이트 · \(Int(fraction * 100))%")
                }
            } else {
                Text(status(intake.phase))
            }
            if intake.phase == .uploading && intake.received == nil {
                Button("같은 GLB 다시 선택") { choosing = true }
                if let snapshot { Text(snapshot.name).lineLimit(1).truncationMode(.middle) }
            }
            if isUnfinished(intake) {
                Menu("등록 관리") {
                    Button("상태 다시 확인") { Task { await change(nil) } }
                    if intake.phase != .cancelling {
                        Button("등록 취소", role: .destructive) { Task { await change(.cancel) } }
                    }
                    Button("현재 등록을 보관하고 새로 시작") { selected = nil; snapshot = nil }
                }
            }
            if intake.fitProfile == .rectangularPortal {
                Text("다른 크기의 출입구에도 맞추는 프레임으로 등록합니다.").foregroundStyle(.secondary)
            }
            DisclosureGroup("출처와 모델 세부 정보") {
                Text(intake.declaration.provenance).foregroundStyle(.secondary)
                if let preparation = intake.preparation {
                    LabeledContent("보정 방식", value: preparation.recipe == .normalizeLegacySignTexture
                        ? "안내판 텍스처 호환 처리" : "사용하지 않는 탄젠트 속성 제거")
                    LabeledContent("원본 SHA-256") {
                        Text(preparation.sourceModel.sha256).font(.caption.monospaced()).textSelection(.enabled)
                    }
                    LabeledContent("등록 모델 SHA-256") {
                        Text(preparation.renderableModel.sha256).font(.caption.monospaced()).textSelection(.enabled)
                    }
                }
            }
        }
    }

    private func placementSection(_ entry: WorkspaceFurnishingSummary) -> some View {
        Section("\(entry.name) 배치") {
            Picker("공간", selection: $room) {
                Text("공간 선택").tag("")
                ForEach(model.draft?.rooms.compactMap { $0["id"].string } ?? [], id: \.self) { id in
                    Text(model.draft?.rooms.first { $0["id"].string == id }?["name"].string ?? "공간").tag(id)
                }
            }
            HStack {
                TextField("X (m)", text: $x)
                TextField("Y (m)", text: $y)
                TextField("Z (m)", text: $z)
            }
            TextField("방향 (°)", text: $yaw)
            TextField("비례 크기", text: $scale)
            if entry.kind == .infoStand && model.workspaceInfoStandPreparationAvailable {
                if let reviewedInfoStand, reviewedInfoStand.furnishing.id == entry.id, reviewedInfoStand.furnishing.version == entry.version {
                    let descriptor = reviewedInfoStand.furnishing.details.model
                    LabeledContent("배치용 GLB · \(descriptor.bytes) B") {
                        Text(descriptor.sha256).font(.caption.monospaced()).textSelection(.enabled)
                    }
                    Button("확인한 GLB 배치") {
                        Task { await place(entry, expectedModelSHA256: descriptor.sha256, infoStandIntakeId: reviewedInfoStand.intakeId) }
                    }
                        .disabled(room.isEmpty)
                } else if let intake = intakes.first(where: { $0.furnishing?.id == entry.id && $0.furnishing?.version == entry.version }) {
                    if let preparation = intake.preparation {
                        LabeledContent("원본 GLB · \(preparation.sourceModel.bytes) B") {
                            Text(preparation.sourceModel.sha256).font(.caption.monospaced()).textSelection(.enabled)
                        }
                    }
                    Button("배치용 GLB 확인") { Task { await reviewInfoStand(entry, intakeId: intake.id) } }
                } else {
                    Text("배치용 모델을 확인하려면 이 안내판의 등록 기록이 필요합니다.").foregroundStyle(.secondary)
                    if intakeCursor != nil {
                        Button("등록 기록 더 찾기") { Task { await loadMoreIntakes() } }
                    }
                }
            } else {
                Button("전시에 배치") { Task { await place(entry) } }.disabled(room.isEmpty)
            }
        }
    }

    private var portalSection: some View {
        Section("출입구 마감") {
            if let draft = model.draft {
                let doors = (draft.candidate["openings"].items ?? []).filter { $0["type"].string == "door" }
                Picker("대상 문", selection: $portalOpening) {
                    Text("문 선택").tag("")
                    ForEach(Array(doors.enumerated()), id: \.offset) { index, door in
                        if let id = door["id"].string {
                            let wallID = door["surfaceId"].string?.lowercased()
                            let wall = draft.candidate["surfaces"].items?.first { $0["id"].string?.lowercased() == wallID }
                            let roomID = wall?["roomId"].string?.lowercased()
                            let room = draft.rooms.first { $0["id"].string?.lowercased() == roomID }
                            Text("\(room?["name"].string ?? "공간") · 문 \(index + 1)").tag(id)
                        }
                    }
                }
                if let target, target.kind == .portalFinish {
                    Button("문에 마감 연결") {
                        let opening = portalOpening
                        Task { await applyPortal(openingId: opening, furnishingId: target.id) }
                    }.disabled(portalOpening.isEmpty || !model.canEditWorkspacePortalFinish)
                }
                if WorkspacePortalFinish.hasAttachment(openingId: portalOpening, in: draft) {
                    Button("문에서 마감 해제") {
                        let opening = portalOpening
                        Task { await applyPortal(openingId: opening, furnishingId: nil) }
                    }.disabled(!model.canEditWorkspacePortalFinish)
                }
            }
        }
    }

    private func applyPortal(openingId: String, furnishingId: String?) async {
        await perform {
            let changed = try await model.applyWorkspacePortalFinish(openingId: openingId, furnishingId: furnishingId, access: access)
            message = changed ? "출입구 마감 변경을 보관했습니다. 전시를 저장하세요." : "선택한 문의 마감은 이미 이 상태입니다."
        }
    }

    private var artworkFrameSection: some View {
        Section("작품 액자") {
            if let draft = model.draft,
               let placement = draft.placements.first(where: { $0["id"].string == model.placementId }),
               let placementId = placement["id"].string,
               let artworkRevisionId = placement["artworkRevisionId"].string,
               let assetId = placement["assetId"].string {
                let artwork = draft.artworks.first { $0["revisionId"].string == artworkRevisionId }
                let attached = WorkspaceArtworkFrameCommand.hasAttachment(for: placementId, in: draft)
                LabeledContent("선택한 작품", value: artwork?["metadata"]["title"].string ?? "작품")
                if let target, target.kind == .artworkFrame {
                    LabeledContent("선택한 액자", value: target.name)
                    Button(attached ? "선택한 액자로 교체" : "작품에 액자 연결") {
                        Task {
                            await applyFrame(to: placementId) { api, state in
                                try await api.prepareSelectedWorkspaceArtworkFrame(target, placementId: placementId,
                                    artworkRevisionId: artworkRevisionId, assetId: assetId, in: state)
                            }
                        }
                    }
                    .disabled(!model.canEditWorkspaceArtworkFrame)
                }
                if attached {
                    Button("작품에서 액자 해제") {
                        Task {
                            await applyFrame(to: placementId) { api, state in
                                try await api.prepareWorkspaceArtworkFrame(.detach(placementId: placementId), in: state)
                            }
                        }
                    }
                    .disabled(!model.canEditWorkspaceArtworkFrame)
                }
                if model.dirty {
                    Text("액자를 변경하려면 현재 전시를 먼저 저장하세요.").foregroundStyle(.secondary)
                }
            } else {
                Text("전시에서 액자를 연결할 작품을 먼저 선택하세요.").foregroundStyle(.secondary)
            }
        }
    }

    private func applyFrame(to placementId: String,
                            prepare: (AuthoringAPI, NativeDocumentState) async throws -> NativeExhibitionDraft?) async {
        await perform {
            let changed = try await model.applyWorkspaceArtworkFrame(to: placementId, access: access, prepare: prepare)
            message = changed ? "액자 변경을 보관했습니다. 전시를 저장하세요." : "선택한 액자가 이미 연결되어 있습니다."
        }
    }

    private func optional(_ value: Binding<String?>) -> Binding<String> {
        Binding(get: { value.wrappedValue ?? "" }, set: { value.wrappedValue = $0.isEmpty ? nil : $0 })
    }

    private func status(_ phase: WorkspaceFurnishingIntake.Phase) -> String {
        switch phase {
        case .uploading: "파일 수신 대기"
        case .validating: "GLB 확인 중"
        case .cancelling: "취소 처리 중"
        case .registered: "작업 공간에 등록됨"
        case .failed: "등록 실패"
        case .cancelled: "등록 취소됨"
        }
    }

    private func perform(_ activity: String = "비품 상태를 확인하고 있습니다…", _ work: () async throws -> Void) async {
        guard !busy, access == model.viewportAccessID else { return }
        let registeredBefore = selected?.phase == .registered ? selected?.furnishing?.id : nil
        busyActivity = activity
        message = nil
        defer { busyActivity = nil }
        do {
            try await work()
            if !Task.isCancelled, access == model.viewportAccessID, selected?.phase == .registered,
               let item = selected?.furnishing, item.id != registeredBefore {
                onRegistered(item)
            }
        }
        catch {
            message = (error as? AuthoringFailure)?.errorDescription ?? "비품 등록 또는 배치 상태를 다시 확인하세요."
        }
    }

    private func selectBundled(_ item: BundledWorkspaceFurnishings.Item) async {
        await perform {
            guard let bundledCatalog, let selectedKind = item.kind, Self.supports(selectedKind, model: model),
                  selectedKind != .portalFinish || model.workspacePortalRectangularFitAvailable else {
                throw AuthoringFailure("FURNISHING_CATALOG_INVALID")
            }
            let file = try await BundledWorkspaceFurnishings.load(item.id, from: bundledCatalog)
            try Task.checkCancellation()
            guard access == model.viewportAccessID, selected == nil, request == nil,
                  selectedKind != .portalFinish || model.workspacePortalRectangularFitAvailable else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
            _ = try WorkspaceFurnishingModel(snapshot: file)
            var bundledRights = ArtworkMetadata().rights
            bundledRights.holder = "ExhibitOS"
            bundledRights.ownership = "authorized-licensee"
            bundledRights.licenseId = "CC0-1.0"
            bundledRights.licenseText = nil
            bundledRights.creditLine = "ExhibitOS"
            bundledRights.permissions.display = true
            bundledRights.permissions.download = true
            bundledRights.permissions.export = true
            bundledRights.permissions.commercial = true
            kind = selectedKind
            reusablePortal = selectedKind == .portalFinish; name = item.name
            provenance = bundledCatalog.provenance(for: item)
            rights = bundledRights; snapshot = file
        }
    }

    private func selectWorkspace(_ entry: WorkspaceFurnishingSummary) {
        guard access == model.viewportAccessID, entry.rightsAvailability.display else { return }
        target = entry
        reviewedInfoStand = nil
        room = model.roomId
        placementId = UUID().uuidString.lowercased()
    }

    private func reload(revealing id: String? = nil) async {
        await perform {
            let pages = try await model.withWorkspaceFurnishingAccess(access) { api in
                let intakes = try await api.workspaceFurnishingIntakes()
                var entry: WorkspaceFurnishingSummary?
                if let id {
                    let catalog = try await loadWorkspaceFurnishingCatalog(using: api, revealing: id)
                    entry = catalog.items.first { $0.id == id && $0.rightsAvailability.display }
                }
                let seatPolicyETag = entry?.kind == .bench ? await model.workspaceBenchSeatPolicyETag(using: api) : nil
                return (intakes, entry, seatPolicyETag)
            }
            intakes = pages.0.items
            intakeCursor = pages.0.nextCursor
            seatPolicyETag = pages.2
            if id != nil {
                if let entry = pages.1 {
                    selectWorkspace(entry)
                } else {
                    message = "선택한 비품을 배치할 수 없습니다. 등록 상태와 전시 권리를 확인하세요."
                }
            }
        }
    }

    private func loadMoreIntakes() async {
        await perform {
            guard let intakeCursor else { return }
            let page = try await model.withWorkspaceFurnishingAccess(access) {
                try await $0.workspaceFurnishingIntakes(cursor: intakeCursor)
            }
            let known = Set(intakes.map(\.id))
            intakes.append(contentsOf: page.items.filter { !known.contains($0.id) })
            self.intakeCursor = page.nextCursor
        }
    }

    private func preparationPolicy(for model: WorkspaceFurnishingModel) -> WorkspaceArtworkFramePreparationPolicy? {
        guard kind == .artworkFrame, self.model.workspaceArtworkFramePreparationAvailable else { return nil }
        return WorkspaceArtworkFramePreparationPolicy.matching(model)
    }

    private func submit() async {
        await perform("비품 등록 요청을 확인하고 있습니다…") {
            if selected == nil && request == nil {
                guard let snapshot else { throw AuthoringFailure("FURNISHING_DECLARATION") }
                let descriptor = try WorkspaceFurnishingModel(snapshot: snapshot)
                request = .init(access: access, id: UUID().uuidString.lowercased(),
                    details: .init(kind: kind, name: name.trimmingCharacters(in: .whitespacesAndNewlines), rights: rights,
                                   provenance: provenance.trimmingCharacters(in: .whitespacesAndNewlines), model: descriptor),
                    policy: preparationPolicy(for: descriptor),
                    fitProfile: kind == .portalFinish && reusablePortal ? .rectangularPortal : nil)
            }
            try await model.withWorkspaceFurnishingAccess(access) { api in
                var intake: WorkspaceFurnishingIntake
                if let selected {
                    intake = try await api.workspaceFurnishingIntake(selected.id,
                        requireCurrentInfoStandProducer: selected.preparationPolicy == .normalizeLegacySignTexture && selected.phase != .registered)
                } else if let request {
                    do {
                        intake = try await api.createWorkspaceFurnishing(requestId: request.id, details: request.details,
                                                                        preparationPolicy: request.policy,
                                                                        fitProfile: request.fitProfile)
                    } catch let failure as AuthoringFailure {
                        if ["FURNISHING_DECLARATION", "FURNISHING_INTAKE_LIMIT", "FRAME_PREPARATION_SOURCE_UNSUPPORTED",
                            "FRAME_PREPARATION_RIGHTS_DENIED"].contains(failure.code) { self.request = nil }
                        throw failure
                    }
                } else { return }
                try remember(intake)
                guard intake.phase == .uploading else { return }
                if intake.received == nil {
                    guard let snapshot else { return }
                    busyActivity = "GLB 파일을 전송하고 있습니다…"
                    intake = try await api.uploadWorkspaceFurnishing(intake, snapshot: snapshot)
                    try remember(intake)
                }
                busyActivity = "GLB 검증 결과를 확인하고 있습니다…"
                try remember(await api.workspaceFurnishingAction(intake, .complete))
            }
        }
    }

    private func change(_ action: WorkspaceFurnishingIntakeAction?) async {
        guard let selected else { return }
        await perform {
            try await model.withWorkspaceFurnishingAccess(access) { api in
                let current = try await api.workspaceFurnishingIntake(selected.id,
                    requireCurrentInfoStandProducer: selected.preparationPolicy == .normalizeLegacySignTexture && selected.phase != .registered)
                try remember(current, reportFailure: action == nil)
                if let action {
                    busyActivity = action == .cancel ? "등록 취소를 확인하고 있습니다…" : "GLB 검증 결과를 확인하고 있습니다…"
                    try remember(await api.workspaceFurnishingAction(current, action))
                }
            }
        }
    }

    private func remember(_ intake: WorkspaceFurnishingIntake, reportFailure: Bool = true) throws {
        guard access == model.viewportAccessID else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        if let selected, selected.id != intake.id || selected.requestId != intake.requestId
            || selected.declaration != intake.declaration || selected.preparationPolicy != intake.preparationPolicy
            || selected.fitProfile != intake.fitProfile
            || (selected.phase == .registered && (intake.preparation != selected.preparation || intake.furnishing != selected.furnishing)) {
            throw AuthoringFailure("FURNISHING_RESPONSE_INVALID")
        }
        selected = intake
        request = nil
        if intake.received != nil { snapshot = nil }
        if let index = intakes.firstIndex(where: { $0.id == intake.id }) { intakes[index] = intake }
        else { intakes.insert(intake, at: 0) }
        if reportFailure, intake.phase == .failed, let error = intake.error {
            throw AuthoringFailure(error.code)
        }
    }

    private func reviewInfoStand(_ entry: WorkspaceFurnishingSummary, intakeId: String) async {
        await perform {
            let furnishing = try await model.withWorkspaceFurnishingAccess(access) { api in
                try await api.reviewWorkspaceInfoStand(intakeId: intakeId, furnishingId: entry.id, version: entry.version)
            }
            reviewedInfoStand = (furnishing, intakeId)
        }
    }

    private func place(_ entry: WorkspaceFurnishingSummary, expectedModelSHA256: String? = nil, infoStandIntakeId: String? = nil) async {
        await perform {
            guard let x = Double(x), let y = Double(y), let z = Double(z),
                  let yaw = Double(yaw), let scale = Double(scale) else { throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID") }
            let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: entry.id,
                furnishingVersion: entry.version, position: [x, y, z], yaw: yaw * .pi / 180, scale: scale)
            if try await model.placeWorkspaceFurnishing(placement, access: access,
                expectedModelSHA256: expectedModelSHA256, infoStandIntakeId: infoStandIntakeId) {
                target = nil
                reviewedInfoStand = nil
                placementStaged = true
            }
        }
    }
}

func loadWorkspaceFurnishingCatalog(using api: AuthoringAPI, cursor: String? = nil, revealing id: String? = nil)
    async throws -> (items: [WorkspaceFurnishingSummary], nextCursor: String?) {
    var items: [WorkspaceFurnishingSummary] = []
    var seenIDs = Set<String>()
    var seenCursors = Set<String>()
    var next = cursor
    repeat {
        if let next, !seenCursors.insert(next).inserted { throw AuthoringFailure("FURNISHING_RESPONSE_INVALID") }
        let page = try await api.workspaceFurnishings(cursor: next)
        try Task.checkCancellation()
        items.append(contentsOf: page.items.filter { seenIDs.insert($0.id).inserted })
        next = page.nextCursor
        if id == nil || id.map(seenIDs.contains) == true { break }
    } while next != nil
    return (items, next)
}
