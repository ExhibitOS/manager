// SPDX-License-Identifier: Apache-2.0
import AppKit
import ManagerCore
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct WorkspaceMaterialIntakeView: View {
    @ObservedObject var model: ExhibitionModel
    let access: UUID
    @Environment(\.dismiss) private var dismiss
    @State private var kind = WorkspaceMaterialKind.floor
    @State private var name = ""
    @State private var provenance = ""
    @State private var rights = {
        var value = ArtworkMetadata().rights
        value.licenseText = nil
        value.permissions.display = false
        return value
    }()
    @State private var files: [WorkspaceMaterialRole: ImportFileSnapshot] = [:]
    @State private var role = WorkspaceMaterialRole.color
    @State private var choosing = false
    @State private var busyActivity: String?
    @State private var declaration: WorkspaceMaterialDeclaration?
    @State private var intake: WorkspaceMaterialIntake?
    @State private var preview: NSImage?
    @State private var errorMessage: String?
    @State private var library: [WorkspaceMaterialSummary] = []
    @State private var nextCursor: String?
    @State private var loadedLibrary = false
    @State private var surfaceMaterial: WorkspaceMaterialSummary?
    @State private var targetSurfaceId = ""
    @State private var repeatWidth = "2"
    @State private var repeatLength = "2"
    @State private var rotationDegrees = "0"
    @State private var bindingNotice: String?
    private var busy: Bool { busyActivity != nil }

    private func surfaceLabel(_ kind: WorkspaceMaterialKind) -> String {
        kind == .floor ? "바닥" : "벽"
    }

    private func surfaces(for kind: WorkspaceMaterialKind) -> [(id: String, name: String)] {
        guard let draft = model.draft else { return [] }
        return (draft.candidate["surfaces"].items ?? []).enumerated().compactMap { index, surface in
            guard surface["type"].string == kind.rawValue, let id = surface["id"].string else { return nil }
            let room = draft.rooms.first(where: { $0["id"] == surface["roomId"] })?["name"].string ?? "공간"
            return (id, "\(room) · \(surfaceLabel(kind)) \(index + 1)")
        }
    }

    private var readyToStart: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !provenance.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !rights.holder.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && (rights.licenseId != nil || rights.licenseText != nil) && files[.color] != nil
    }
    private var canClose: Bool {
        guard !busy else { return false }
        if access != model.viewportAccessID { return true }
        guard declaration != nil else { return true }
        guard let phase = intake?.phase else { return false }
        return [.registered, .failed, .cancelled].contains(phase)
    }
    private func label(_ role: WorkspaceMaterialRole) -> String {
        switch role {
        case .color: "색상"
        case .normal: "요철"
        case .roughness: "거칠기"
        case .ao: "주변광 차폐"
        }
    }
    private func optional(_ value: Binding<String?>) -> Binding<String> {
        Binding(get: { value.wrappedValue ?? "" }, set: { value.wrappedValue = $0.isEmpty ? nil : $0 })
    }
    private var status: String {
        switch intake?.phase {
        case .uploading: "파일 수신 대기"
        case .validating: "파일과 권리 확인 중"
        case .cancelling: "취소 처리 중"
        case .registered: "작업공간에 등록됨"
        case .failed: "등록 실패"
        case .cancelled: "등록 취소됨"
        case nil: declaration == nil ? "등록 준비" : "등록 요청 상태 확인 필요"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("작업공간 재질 등록").font(.headline)
            Form {
                Section("재질") {
                    Picker("종류", selection: $kind) {
                        Text("바닥").tag(WorkspaceMaterialKind.floor)
                        Text("벽").tag(WorkspaceMaterialKind.wall)
                    }
                    TextField("재질 이름", text: $name)
                    TextField("출처", text: $provenance, axis: .vertical)
                    ForEach(WorkspaceMaterialRole.allCases, id: \.self) { mapRole in
                        LabeledContent(mapRole == .color ? "색상 (필수)" : label(mapRole)) {
                            HStack {
                                if let file = files[mapRole] {
                                    Text(file.name).lineLimit(1).truncationMode(.middle)
                                    Button("제거") { files[mapRole] = nil }
                                        .accessibilityLabel(label(mapRole) + " 파일 제거")
                                }
                                Button("PNG 선택") { role = mapRole; choosing = true }
                                    .accessibilityLabel(label(mapRole) + " PNG 선택")
                            }
                        }
                    }
                    Text("맵당 최대 8 MiB입니다. 요철 맵은 +Y 형식을 사용하세요.").foregroundStyle(.secondary)
                }
                Section("권리와 허용 범위") {
                    TextField("권리자", text: $rights.holder)
                    Picker("권리 근거", selection: $rights.ownership) {
                        Text("내가 권리자").tag("owner")
                        Text("허가를 받음").tag("authorized-licensee")
                    }
                    TextField("표시할 크레딧", text: $rights.creditLine)
                    TextField("라이선스 ID", text: optional($rights.licenseId))
                    TextField("허용 조건", text: optional($rights.licenseText), axis: .vertical)
                    Toggle("전시 허용", isOn: $rights.permissions.display)
                    Toggle("다운로드 허용", isOn: $rights.permissions.download)
                    Toggle("OEX 내보내기 허용", isOn: $rights.permissions.export)
                    Toggle("상업적 사용 허용", isOn: $rights.permissions.commercial)
                    TextField("권리 시작일 (ISO 8601, 선택)", text: optional($rights.validFrom))
                    TextField("권리 만료일 (ISO 8601, 선택)", text: optional($rights.expiresAt))
                }
                if declaration == nil {
                    Section("등록한 재질") {
                        ForEach(library.indices, id: \.self) { index in
                            let item = library[index]
                            HStack {
                                Button(item.name) { Task { await previewLibraryItem(item) } }
                                    .disabled(!item.rightsAvailability.display)
                                if item.rightsAvailability.display && (item.kind == .floor || model.workspaceWallMaterialAvailable) {
                                    let label = surfaceLabel(item.kind)
                                    Spacer()
                                    Button("\(label) 연결 설정") {
                                        surfaceMaterial = item; targetSurfaceId = ""; bindingNotice = nil
                                    }
                                    .accessibilityLabel("\(item.name) \(label) 연결 설정")
                                }
                            }
                        }
                        if !loadedLibrary || nextCursor != nil {
                            Button(loadedLibrary ? "재질 더 보기" : "등록 목록 보기") {
                                Task { await loadLibrary() }
                            }
                        }
                        if loadedLibrary && library.isEmpty { Text("등록된 재질이 없습니다.").foregroundStyle(.secondary) }
                    }
                    if let surfaceMaterial {
                        let label = surfaceLabel(surfaceMaterial.kind)
                        Section("\(label) 연결") {
                            Text(surfaceMaterial.name)
                            Picker(label, selection: $targetSurfaceId) {
                                Text("\(label) 선택").tag("")
                                ForEach(surfaces(for: surfaceMaterial.kind), id: \.id) { surface in Text(surface.name).tag(surface.id) }
                            }
                            TextField("가로 반복 (m)", text: $repeatWidth)
                            TextField("세로 반복 (m)", text: $repeatLength)
                            TextField("회전 (°)", text: $rotationDegrees)
                            Text(model.workspaceMaterialMultimapAvailable
                                 ? "색상 맵과 등록된 노멀·거칠기 맵을 연결합니다."
                                 : "색상 맵 한 장을 연결합니다.")
                                .foregroundStyle(.secondary)
                            Button("\(label)에 연결") { Task { await applySurfaceMaterial(surfaceMaterial) } }
                                .disabled(targetSurfaceId.isEmpty)
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .disabled(busy || declaration != nil)
            if let intake, intake.phase == .uploading, let declaration, !declaration.maps.isEmpty {
                ProgressView(value: Double(intake.received.count), total: Double(declaration.maps.count)) {
                    Text("재질 파일 수신 확인")
                } currentValueLabel: {
                    let fraction = Double(intake.received.count) / Double(declaration.maps.count)
                    Text("\(intake.received.count) / \(declaration.maps.count)개 · \(fraction.formatted(.percent.precision(.fractionLength(0))))")
                }.controlSize(.small)
            }
            if let busyActivity {
                ProgressView(busyActivity).controlSize(.small)
            } else if intake?.phase == .validating || intake?.phase == .cancelling {
                ProgressView(status).controlSize(.small)
            } else if intake?.phase != .uploading {
                Text(status).accessibilityAddTraits(.updatesFrequently)
            }
            if let errorMessage { Text(errorMessage).foregroundStyle(.secondary) }
            if let bindingNotice { Text(bindingNotice).accessibilityAddTraits(.updatesFrequently) }
            if let preview {
                Image(nsImage: preview).resizable().scaledToFit().frame(maxHeight: 120)
                    .accessibilityLabel("등록된 재질의 색상 맵")
            }
            actions
        }
        .padding(20)
        .frame(minWidth: 520, idealWidth: 560, minHeight: 510)
        .interactiveDismissDisabled(!canClose)
        .fileImporter(isPresented: $choosing, allowedContentTypes: [.png]) { result in
            let chosenRole = role
            Task {
                await perform {
                    let snapshot = try await ImportFileSnapshot.load(result.get(), byteLimit: WorkspaceMaterialMap.byteLimit)
                    _ = try WorkspaceMaterialMap(snapshot: snapshot, role: chosenRole)
                    files[chosenRole] = snapshot
                }
            }
        }
    }

    private var actions: some View {
        HStack {
            Button("닫기", role: .cancel) { dismiss() }.disabled(!canClose).keyboardShortcut(.cancelAction)
            Spacer()
            if let intake {
                if [.uploading, .validating, .failed].contains(intake.phase) {
                    Button("등록 취소") { Task { await change(.cancel) } }
                }
                if intake.retryable {
                    Button("검증 다시 시도") { Task { await change(.retry) } }
                }
                if let material = intake.material {
                    Button("색상 미리보기") { Task { await showPreview(material) } }
                } else {
                    Button("상태 확인") { Task { await refresh() } }
                }
            }
            if declaration == nil || intake == nil || intake?.phase == .uploading {
                Button(declaration == nil ? "등록 시작" : "이어서 등록") { Task { await submit() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(declaration == nil && !readyToStart)
            }
        }.disabled(busy)
    }

    private func perform(_ activity: String = "재질 상태를 확인하고 있습니다…", _ work: () async throws -> Void) async {
        guard !busy else { return }
        busyActivity = activity; errorMessage = nil
        defer { busyActivity = nil }
        do { try await work() }
        catch {
            let failure = error as? AuthoringFailure ?? AuthoringFailure((error as? BridgeFailure)?.code ?? "CONNECTION_FAILED")
            let map = failure.role.flatMap(WorkspaceMaterialRole.init(rawValue:))
            let message = failure.errorDescription ?? "등록 상태를 다시 확인하세요."
            errorMessage = map.map { label($0) + " 파일 · " + message } ?? message
        }
    }

    private func remember(_ next: WorkspaceMaterialIntake) throws {
        try next.check(id: intake?.intakeId, declaration: declaration)
        intake = next
        if let error = next.error, next.phase == .failed {
            throw AuthoringFailure(error.code, role: error.role?.rawValue, field: error.field)
        }
    }

    private func submit() async {
        await perform("재질 등록 요청을 확인하고 있습니다…") {
            if declaration == nil {
                let maps = try Dictionary(uniqueKeysWithValues: files.map { role, file in
                    (role.rawValue, try WorkspaceMaterialMap(snapshot: file, role: role))
                })
                declaration = WorkspaceMaterialDeclaration(requestId: UUID().uuidString.lowercased(), kind: kind,
                    name: name, rights: rights, provenance: provenance, maps: maps)
            }
            guard let declaration else { return }
            try await model.withWorkspaceMaterialAccess(access) { api in
                if let intake {
                    try remember(await api.workspaceMaterialIntake(intake.intakeId))
                } else {
                    let next: WorkspaceMaterialIntake
                    do {
                        next = try await api.createWorkspaceMaterial(declaration)
                    } catch let failure as AuthoringFailure {
                        self.declaration = declaration.pendingAfterCreateFailure(failure)
                        throw failure
                    }
                    try remember(next)
                }
                guard let intake, intake.phase == .uploading else { return }
                for role in WorkspaceMaterialRole.allCases where intake.received[role.rawValue] == nil {
                    guard let snapshot = files[role] else { continue }
                    busyActivity = label(role) + " 맵을 전송하고 있습니다…"
                    try remember(await api.uploadWorkspaceMaterial(intake.intakeId, role: role, snapshot: snapshot))
                }
                busyActivity = "재질 파일과 권리 확인 결과를 기다리고 있습니다…"
                try remember(await api.workspaceMaterialAction(intake.intakeId, .complete))
            }
        }
    }

    private func refresh() async {
        guard let id = intake?.intakeId else { return }
        await perform {
            try await model.withWorkspaceMaterialAccess(access) { api in
                try remember(await api.workspaceMaterialIntake(id))
            }
        }
    }

    private func change(_ action: WorkspaceMaterialIntakeAction) async {
        guard let id = intake?.intakeId else { return }
        await perform(action == .cancel ? "등록 취소를 확인하고 있습니다…" : "재질 파일과 권리 확인 결과를 기다리고 있습니다…") {
            try await model.withWorkspaceMaterialAccess(access) { api in
                try remember(await api.workspaceMaterialAction(id, action))
            }
        }
    }

    private func showPreview(_ material: WorkspaceMaterial) async {
        preview = nil
        await perform {
            let data = try await model.withWorkspaceMaterialAccess(access) { api in
                try await api.workspaceMaterialPreview(material, role: .color)
            }
            guard let image = NSImage(data: data) else { throw AuthoringFailure("FORMAT_REJECTED") }
            preview = image
        }
    }

    private func loadLibrary() async {
        await perform {
            let page = try await model.withWorkspaceMaterialAccess(access) { api in
                try await api.workspaceMaterials(cursor: nextCursor)
            }
            library.append(contentsOf: page.items)
            nextCursor = page.nextCursor
            loadedLibrary = true
        }
    }

    private func previewLibraryItem(_ item: WorkspaceMaterialSummary) async {
        preview = nil
        await perform {
            let data = try await model.withWorkspaceMaterialAccess(access) { api in
                let material = try await api.workspaceMaterial(item.id, version: item.version)
                return try await api.workspaceMaterialPreview(material, role: .color)
            }
            guard let image = NSImage(data: data) else { throw AuthoringFailure("FORMAT_REJECTED") }
            preview = image
        }
    }

    private func applySurfaceMaterial(_ material: WorkspaceMaterialSummary) async {
        bindingNotice = nil
        await perform {
            guard let width = Double(repeatWidth), let length = Double(repeatLength),
                  let degrees = Double(rotationDegrees), (-180...180).contains(degrees) else {
                throw AuthoringFailure("WORKSPACE_MATERIAL_BINDING_INVALID")
            }
            let binding = try WorkspaceSurfaceMaterialBinding(surfaceKind: material.kind, surfaceId: targetSurfaceId,
                materialId: material.id, materialVersion: material.version, repeatMeters: [width, length], rotation: degrees * (.pi / 180))
            let changed = try await model.applyWorkspaceSurfaceMaterial(binding, access: access)
            let label = surfaceLabel(material.kind)
            bindingNotice = changed ? "\(label) 연결을 보관했습니다. 닫은 뒤 전시를 저장하세요." : "이 \(label)에 같은 재질과 설정이 이미 저장되어 있습니다."
        }
    }
}
