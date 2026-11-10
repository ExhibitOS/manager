// SPDX-License-Identifier: Apache-2.0
import ManagerCore
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct WorkspaceFurnishingIntakeView: View {
    @ObservedObject var model: ExhibitionModel
    let access: UUID
    @Environment(\.dismiss) private var dismiss
    @State private var kind = WorkspaceFurnishingKind.pedestal
    @State private var name = ""
    @State private var provenance = ""
    @State private var rights = {
        var value = ArtworkMetadata().rights
        value.licenseText = nil
        value.permissions.display = false
        return value
    }()
    @State private var snapshot: ImportFileSnapshot?
    @State private var bundledItems: [BundledWorkspaceFurnishings.Item] = []
    @State private var bundledRoot: URL?
    @State private var choosing = false
    @State private var busy = false
    @State private var message: String?
    @State private var request: (id: String, details: WorkspaceFurnishingDetails, policy: WorkspaceArtworkFramePreparationPolicy?)?
    @State private var selected: WorkspaceFurnishingIntake?
    @State private var intakes: [WorkspaceFurnishingIntake] = []
    @State private var intakeCursor: String?
    @State private var library: [WorkspaceFurnishingSummary] = []
    @State private var catalogCursor: String?
    @State private var target: WorkspaceFurnishingSummary?
    @State private var reviewedInfoStand: WorkspaceFurnishing?
    @State private var room = ""
    @State private var portalOpening = ""
    @State private var x = "0"
    @State private var y = "0"
    @State private var z = "0"
    @State private var yaw = "0"
    @State private var scale = "1"
    @State private var placementId = UUID().uuidString.lowercased()
    @State private var seatPolicyETag: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("작업 공간 비품").font(.headline)
            Form {
                if let selected { intakeSection(selected) }
                else if let request {
                    Section("등록 상태 확인") {
                        Text(request.details.name)
                        Button("같은 요청 다시 확인") { Task { await submit() } }
                        Button("목록에서 찾기") { Task { await reload() } }
                    }
                } else { newSection }

                Section("등록 기록") {
                    ForEach(intakes) { intake in
                        Button("\(intake.declaration.name) · \(status(intake.phase))") {
                            selected = intake
                            request = nil
                            snapshot = nil
                        }
                    }
                    if intakeCursor != nil {
                        Button("등록 기록 더 보기") { Task { await loadMore(intake: true) } }
                    }
                }
                Section("등록한 비품") {
                    ForEach(library) { entry in
                        Button("\(entry.name) · v\(entry.version)") {
                            target = entry
                            reviewedInfoStand = nil
                            room = model.roomId
                            placementId = UUID().uuidString.lowercased()
                        }.disabled(!entry.rightsAvailability.display)
                    }
                    if catalogCursor != nil {
                        Button("비품 더 보기") { Task { await loadMore(intake: false) } }
                    }
                }
                if let target, target.kind != .artworkFrame, target.kind != .portalFinish { placementSection(target) }
                if let target, target.kind == .bench, let seatPolicyETag, seatPolicyETag == model.state?.etag {
                    WorkspaceBenchSeatEditor(model: model, entry: target, access: access)
                        .id(target.id)
                }
                if model.workspaceArtworkFrameAvailable { artworkFrameSection }
                if model.workspacePortalFinishAvailable { portalSection }
            }.disabled(busy || access != model.viewportAccessID)
            if let message { Text(message).accessibilityAddTraits(.updatesFrequently) }
            HStack {
                Button("목록 새로 고침") { Task { await reload() } }.disabled(busy || access != model.viewportAccessID)
                Spacer()
                Button("닫기", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction).disabled(busy)
            }
        }
        .padding(20)
        .frame(minWidth: 530, minHeight: 560)
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
                bundledRoot = catalog.0; bundledItems = catalog.1
            }
            await reload()
        }
    }

    private var newSection: some View {
        Section("GLB 등록") {
            if !bundledItems.isEmpty {
                Menu("기본 비품 선택") {
                    ForEach(bundledItems) { item in
                        Button(item.name) { Task { await selectBundled(item) } }
                            .disabled(item.kind == .stool && !model.workspaceFurnishingStoolAvailable ||
                                      item.kind == .infoStand && !model.workspaceFurnishingRoomVisualAvailable)
                    }
                }
            }
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
            TextField("이름", text: $name)
            TextField("출처", text: $provenance, axis: .vertical)
            Button("GLB 선택") { choosing = true }
            if let snapshot { Text(snapshot.name).lineLimit(1).truncationMode(.middle) }
            if let snapshot, let descriptor = try? WorkspaceFurnishingModel(snapshot: snapshot),
               preparationPolicy(for: descriptor) != nil {
                Text("등록할 때 서버에서 표시용 액자 모델을 만들고 원본도 보관합니다.").foregroundStyle(.secondary)
            }
            if kind == .infoStand && model.workspaceInfoStandPreparationAvailable {
                Text("권리자가 등록한 안내판은 원본을 보관합니다. 호환 처리가 필요한 경우 표시용 GLB를 별도로 만들며, 배치하기 전에 등록 모델을 확인할 수 있습니다.")
                    .foregroundStyle(.secondary)
            }
            TextField("권리자", text: $rights.holder)
            Picker("권리 근거", selection: $rights.ownership) {
                Text("내가 권리자").tag("owner")
                Text("허가를 받음").tag("authorized-licensee")
            }
            TextField("표시할 크레딧", text: $rights.creditLine)
            TextField("라이선스 ID", text: optional($rights.licenseId))
            TextField("허용 조건", text: optional($rights.licenseText))
            TextField("권리 시작일 (ISO 8601, 선택)", text: optional($rights.validFrom))
            TextField("권리 만료일 (ISO 8601, 선택)", text: optional($rights.expiresAt))
            Toggle("전시 허용", isOn: $rights.permissions.display)
            Toggle("다운로드 허용", isOn: $rights.permissions.download)
            Toggle("내보내기 허용", isOn: $rights.permissions.export)
            Toggle("상업적 사용 허용", isOn: $rights.permissions.commercial)
            Button("등록 시작") { Task { await submit() } }
                .disabled(snapshot == nil || name.isEmpty || provenance.isEmpty || rights.holder.isEmpty
                          || (rights.licenseId == nil && rights.licenseText == nil))
        }
    }

    private func intakeSection(_ intake: WorkspaceFurnishingIntake) -> some View {
        Section(intake.declaration.name) {
            Text(status(intake.phase))
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
            if intake.phase == .uploading {
                if intake.received == nil {
                    Button("같은 GLB 다시 선택") { choosing = true }
                    if let snapshot { Text(snapshot.name).lineLimit(1).truncationMode(.middle) }
                }
                Button("등록 이어서 진행") { Task { await submit() } }
                    .disabled(intake.received == nil && snapshot == nil)
            }
            Button("상태 다시 확인") { Task { await change(nil) } }
            if intake.retryable { Button("검증 다시 시도") { Task { await change(.retry) } } }
            if [.uploading, .validating, .failed].contains(intake.phase) {
                Button("등록 취소") { Task { await change(.cancel) } }
            }
            Button("새 등록 준비") { selected = nil; request = nil; snapshot = nil }
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
                if let reviewedInfoStand, reviewedInfoStand.id == entry.id, reviewedInfoStand.version == entry.version {
                    let descriptor = reviewedInfoStand.details.model
                    LabeledContent("배치용 GLB · \(descriptor.bytes) B") {
                        Text(descriptor.sha256).font(.caption.monospaced()).textSelection(.enabled)
                    }
                    Button("확인한 GLB 배치") { Task { await place(entry, expectedModelSHA256: descriptor.sha256) } }
                        .disabled(room.isEmpty)
                } else {
                    Button("배치용 GLB 확인") { Task { await reviewInfoStand(entry) } }
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
                } else {
                    Text("등록한 비품 목록에서 출입구 마감을 선택하세요.").foregroundStyle(.secondary)
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
                } else if !attached {
                    Text("등록한 비품 목록에서 액자를 선택하세요.").foregroundStyle(.secondary)
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

    private func perform(_ work: () async throws -> Void) async {
        guard !busy, access == model.viewportAccessID else { return }
        busy = true
        message = nil
        defer { busy = false }
        do { try await work() }
        catch {
            message = (error as? AuthoringFailure)?.errorDescription ?? "비품 등록 또는 배치 상태를 다시 확인하세요."
        }
    }

    private func selectBundled(_ item: BundledWorkspaceFurnishings.Item) async {
        await perform {
            guard let root = bundledRoot, let selectedKind = item.kind else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
            let file = try await BundledWorkspaceFurnishings.load(item, root: root)
            try Task.checkCancellation()
            guard access == model.viewportAccessID, selected == nil, request == nil else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
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
            kind = selectedKind; name = item.name
            provenance = "ExhibitOS default furnishings 1.2.0, \(item.catalogId), original-v1."
            rights = bundledRights; snapshot = file
        }
    }

    private func reload() async {
        await perform {
            let pages = try await model.withWorkspaceFurnishingAccess(access) { api in
                let intakes = try await api.workspaceFurnishingIntakes()
                let catalog = try await api.workspaceFurnishings()
                let seatPolicyETag = await model.workspaceBenchSeatPolicyETag(using: api)
                return (intakes, catalog, seatPolicyETag)
            }
            intakes = pages.0.items
            intakeCursor = pages.0.nextCursor
            library = pages.1.items
            catalogCursor = pages.1.nextCursor
            seatPolicyETag = pages.2
        }
    }

    private func loadMore(intake: Bool) async {
        await perform {
            if intake {
                guard let intakeCursor else { return }
                let page = try await model.withWorkspaceFurnishingAccess(access) { try await $0.workspaceFurnishingIntakes(cursor: intakeCursor) }
                let known = Set(intakes.map(\.id))
                intakes.append(contentsOf: page.items.filter { !known.contains($0.id) })
                self.intakeCursor = page.nextCursor
            } else {
                guard let catalogCursor else { return }
                let page = try await model.withWorkspaceFurnishingAccess(access) { try await $0.workspaceFurnishings(cursor: catalogCursor) }
                let known = Set(library.map(\.id))
                library.append(contentsOf: page.items.filter { !known.contains($0.id) })
                self.catalogCursor = page.nextCursor
            }
        }
    }

    private func preparationPolicy(for model: WorkspaceFurnishingModel) -> WorkspaceArtworkFramePreparationPolicy? {
        guard kind == .artworkFrame, self.model.workspaceArtworkFramePreparationAvailable else { return nil }
        return WorkspaceArtworkFramePreparationPolicy.matching(model)
    }

    private func submit() async {
        await perform {
            if selected == nil && request == nil {
                guard let snapshot else { throw AuthoringFailure("FURNISHING_DECLARATION") }
                let descriptor = try WorkspaceFurnishingModel(snapshot: snapshot)
                request = (UUID().uuidString.lowercased(), .init(kind: kind,
                    name: name.trimmingCharacters(in: .whitespacesAndNewlines), rights: rights,
                    provenance: provenance.trimmingCharacters(in: .whitespacesAndNewlines), model: descriptor),
                    preparationPolicy(for: descriptor))
            }
            try await model.withWorkspaceFurnishingAccess(access) { api in
                var intake: WorkspaceFurnishingIntake
                if let selected {
                    intake = try await api.workspaceFurnishingIntake(selected.id)
                } else if let request {
                    do {
                        intake = try await api.createWorkspaceFurnishing(requestId: request.id, details: request.details,
                                                                        preparationPolicy: request.policy)
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
                    intake = try await api.uploadWorkspaceFurnishing(intake, snapshot: snapshot)
                    try remember(intake)
                }
                try remember(await api.workspaceFurnishingAction(intake, .complete))
            }
        }
    }

    private func change(_ action: WorkspaceFurnishingIntakeAction?) async {
        guard let selected else { return }
        await perform {
            try await model.withWorkspaceFurnishingAccess(access) { api in
                let current = try await api.workspaceFurnishingIntake(selected.id)
                try remember(current, reportFailure: action == nil)
                if let action { try remember(await api.workspaceFurnishingAction(current, action)) }
            }
        }
    }

    private func remember(_ intake: WorkspaceFurnishingIntake, reportFailure: Bool = true) throws {
        guard access == model.viewportAccessID else { throw AuthoringFailure("ACCOUNT_CHANGED") }
        if let selected, selected.id != intake.id || selected.requestId != intake.requestId
            || selected.declaration != intake.declaration || selected.preparationPolicy != intake.preparationPolicy
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

    private func reviewInfoStand(_ entry: WorkspaceFurnishingSummary) async {
        await perform {
            let furnishing = try await model.withWorkspaceFurnishingAccess(access) { api in
                let furnishing = try await api.workspaceFurnishing(entry.id, version: entry.version)
                guard furnishing.details.kind == .infoStand else { throw AuthoringFailure("MODEL_CONFLICT") }
                _ = try await api.workspaceFurnishingModel(furnishing)
                return furnishing
            }
            if let registered = intakes.first(where: { $0.furnishing?.id == entry.id })?.furnishing,
               registered != furnishing {
                throw AuthoringFailure("MODEL_CONFLICT")
            }
            reviewedInfoStand = furnishing
        }
    }

    private func place(_ entry: WorkspaceFurnishingSummary, expectedModelSHA256: String? = nil) async {
        await perform {
            guard let x = Double(x), let y = Double(y), let z = Double(z),
                  let yaw = Double(yaw), let scale = Double(scale) else { throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID") }
            let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: entry.id,
                furnishingVersion: entry.version, position: [x, y, z], yaw: yaw * .pi / 180, scale: scale)
            if try await model.placeWorkspaceFurnishing(placement, access: access, expectedModelSHA256: expectedModelSHA256) {
                target = nil
                reviewedInfoStand = nil
                message = "배치 입력을 보관했습니다. 전시를 저장하세요."
            }
        }
    }
}
