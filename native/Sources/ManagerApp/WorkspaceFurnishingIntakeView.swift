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
    @State private var choosing = false
    @State private var busy = false
    @State private var message: String?
    @State private var request: (id: String, details: WorkspaceFurnishingDetails)?
    @State private var selected: WorkspaceFurnishingIntake?
    @State private var intakes: [WorkspaceFurnishingIntake] = []
    @State private var intakeCursor: String?
    @State private var library: [WorkspaceFurnishingSummary] = []
    @State private var catalogCursor: String?
    @State private var target: WorkspaceFurnishingSummary?
    @State private var room = ""
    @State private var x = "0", y = "0", z = "0", yaw = "0", scale = "1"
    @State private var placementId = UUID().uuidString.lowercased()

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
                            room = model.roomId
                            placementId = UUID().uuidString.lowercased()
                        }.disabled(!entry.rightsAvailability.display)
                    }
                    if catalogCursor != nil {
                        Button("비품 더 보기") { Task { await loadMore(intake: false) } }
                    }
                }
                if let target { placementSection(target) }
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
        .task { await reload() }
    }

    private var newSection: some View {
        Section("GLB 등록") {
            Picker("종류", selection: $kind) {
                Text("받침").tag(WorkspaceFurnishingKind.pedestal)
                Text("벤치").tag(WorkspaceFurnishingKind.bench)
            }
            TextField("이름", text: $name)
            TextField("출처", text: $provenance, axis: .vertical)
            Button("GLB 선택") { choosing = true }
            if let snapshot { Text(snapshot.name).lineLimit(1).truncationMode(.middle) }
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
            Button("전시에 배치") { Task { await place(entry) } }.disabled(room.isEmpty)
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

    private func reload() async {
        await perform {
            let pages = try await model.withWorkspaceFurnishingAccess(access) { api in
                let intakes = try await api.workspaceFurnishingIntakes()
                let catalog = try await api.workspaceFurnishings()
                return (intakes, catalog)
            }
            intakes = pages.0.items
            intakeCursor = pages.0.nextCursor
            library = pages.1.items
            catalogCursor = pages.1.nextCursor
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

    private func submit() async {
        await perform {
            if selected == nil && request == nil {
                guard let snapshot else { throw AuthoringFailure("FURNISHING_DECLARATION") }
                let descriptor = try WorkspaceFurnishingModel(snapshot: snapshot)
                request = (UUID().uuidString.lowercased(), .init(kind: kind,
                    name: name.trimmingCharacters(in: .whitespacesAndNewlines), rights: rights,
                    provenance: provenance.trimmingCharacters(in: .whitespacesAndNewlines), model: descriptor))
            }
            try await model.withWorkspaceFurnishingAccess(access) { api in
                var intake: WorkspaceFurnishingIntake
                if let selected {
                    intake = try await api.workspaceFurnishingIntake(selected.id)
                } else if let request {
                    do {
                        intake = try await api.createWorkspaceFurnishing(requestId: request.id, details: request.details)
                    } catch let failure as AuthoringFailure {
                        if failure.code == "FURNISHING_DECLARATION" || failure.code == "FURNISHING_INTAKE_LIMIT" { self.request = nil }
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
        if let selected, selected.id != intake.id || selected.requestId != intake.requestId || selected.declaration != intake.declaration {
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

    private func place(_ entry: WorkspaceFurnishingSummary) async {
        await perform {
            guard let x = Double(x), let y = Double(y), let z = Double(z),
                  let yaw = Double(yaw), let scale = Double(scale) else { throw AuthoringFailure("WORKSPACE_FURNISHING_INVALID") }
            let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: entry.id,
                furnishingVersion: entry.version, position: [x, y, z], yaw: yaw * .pi / 180, scale: scale)
            if try await model.placeWorkspaceFurnishing(placement, access: access) {
                target = nil
                message = "배치 입력을 보관했습니다. 전시를 저장하세요."
            }
        }
    }
}
