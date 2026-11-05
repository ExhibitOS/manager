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
    @State private var busy = false
    @State private var declaration: WorkspaceMaterialDeclaration?
    @State private var intake: WorkspaceMaterialIntake?
    @State private var preview: NSImage?
    @State private var errorMessage: String?
    @State private var library: [WorkspaceMaterialSummary] = []
    @State private var nextCursor: String?
    @State private var loadedLibrary = false

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
        case .uploading: "파일 수신 \(intake?.received.count ?? 0) / \(declaration?.maps.count ?? 0)"
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
                            Button(item.name) { Task { await previewLibraryItem(item) } }
                                .disabled(!item.rightsAvailability.display)
                        }
                        if !loadedLibrary || nextCursor != nil {
                            Button(loadedLibrary ? "재질 더 보기" : "등록 목록 보기") {
                                Task { await loadLibrary() }
                            }
                        }
                        if loadedLibrary && library.isEmpty { Text("등록된 재질이 없습니다.").foregroundStyle(.secondary) }
                    }
                }
            }
            .formStyle(.grouped)
            .disabled(busy || declaration != nil)
            if busy || intake?.phase == .validating || intake?.phase == .cancelling {
                ProgressView(status).controlSize(.small)
            } else {
                Text(status).accessibilityAddTraits(.updatesFrequently)
            }
            if let errorMessage { Text(errorMessage).foregroundStyle(.secondary) }
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

    private func perform(_ work: () async throws -> Void) async {
        guard !busy else { return }
        busy = true; errorMessage = nil
        defer { busy = false }
        do { try await work() }
        catch {
            let failure = error as? AuthoringFailure ?? AuthoringFailure("CONNECTION_FAILED")
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
        await perform {
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
                    do { try remember(await api.createWorkspaceMaterial(declaration)) }
                    catch let error as AuthoringFailure where error.code == "MATERIAL_DECLARATION" && intake == nil {
                        self.declaration = nil
                        throw error
                    }
                }
                guard let intake, intake.phase == .uploading else { return }
                for role in WorkspaceMaterialRole.allCases where intake.received[role.rawValue] == nil {
                    guard let snapshot = files[role] else { continue }
                    try remember(await api.uploadWorkspaceMaterial(intake.intakeId, role: role, snapshot: snapshot))
                }
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
        await perform {
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
}
