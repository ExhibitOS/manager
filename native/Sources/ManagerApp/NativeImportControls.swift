// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct NativeImportControls: View {
    @ObservedObject var model: AuthoringModel
    let ready: Bool
    let mode: Mode
    let openArtwork: (String) -> Void
    private var belongs: Bool { model.transfer?.intent == nil || model.transfer?.intent?.artworkId == model.selected?.id }
    private var approvedAssets: [NativeArtwork.Asset] { model.selected?.assets.filter { $0.state == "approved" } ?? [] }
    private var needsNewRevision: Bool {
        guard let artwork = model.selected else { return false }
        return artwork.approvedRevision == artwork.revision && artwork.approvedAssetId != model.assetId
    }
    var body: some View {
        Form {
          Section("작품 파일") {
            VStack(alignment: .leading, spacing: 12) {
                if !belongs, let id = model.transfer?.intent?.artworkId {
                    Text("다른 작품에 시작한 파일 검사가 있습니다.")
                    Button("검사 중인 작품 열기") { openArtwork(id) }
                } else {
                    Button(model.importFile == nil ? "\(model.importFormats) 파일 선택…" : "같은 파일 다시 선택…", systemImage: "doc.badge.plus") { Task { await model.chooseImportFile() } }
                    if let file = model.importFile {
                        Text("\(file.name) · \(file.data.count.formatted())바이트").textSelection(.enabled)
                        if ImportFileSnapshot.mayExceedStaticOutputLimit(mime: file.mime, bytes: file.data.count) {
                            Text("이 GLB는 25 MiB보다 큽니다. 정적 전시에 그대로 쓰이면 개별 파일 제한을 넘습니다. 다른 감상용 파일이 사용될 수 있으므로 실제 배포 파일을 확인하세요. 보존된 Processing 촬영 작업에서 웹 관람용 텍스처 사본을 새로 만들 수 있지만, 결과의 크기와 외관은 확인이 필요합니다. 25 MiB 이하 결과는 보장하지 않으며 외부 GLB의 자동 변환은 지원하지 않습니다.")
                                .foregroundStyle(.secondary)
                        }
                    }
                    else if let name = model.transfer?.fileName { Text("저장한 요청: \(name)").textSelection(.enabled) }
                    TextField("파일 1단위의 길이 (m)", value: $model.importScale, format: .number).disabled(model.transfer?.intent != nil)
                    if model.metadataDirty || model.selected == nil {
                        Text("작품 정보와 권리를 먼저 저장하세요.").foregroundStyle(.secondary)
                    }
                    if model.importJob == nil || model.importJob?.state == "uploading" {
                        Button("파일 검사 시작·계속") { Task { await model.uploadImportFile() } }
                            .disabled(model.metadataDirty || model.selected == nil || model.importFile == nil || model.transfer == nil || !model.importScale.isFinite || model.importScale <= 0)
                    }
                }
                if model.transfer?.intent != nil && model.importJob == nil {
                    Button("이전 검사 요청 찾기") { Task { await model.recoverImport() } }
                }
                if let job = model.importJob {
                    Label(stateTitle(job.state), systemImage: job.state == "approved" ? "checkmark.circle" : "doc.text.magnifyingglass")
                    if model.jpegOriginalStored { Text("JPEG 원본과 감상용 PNG를 이 Mac에 보관했습니다.").foregroundStyle(.secondary) }
                    if let near = job.modelDerivation {
                        if !near.workerDrained, job.state == "cancelled" {
                            Text("파일 처리의 종료를 확인하고 있습니다.").foregroundStyle(.secondary)
                        } else if near.status == "pending" {
                            Text("원본 파일을 검사하고 감상용 파일을 준비하고 있습니다.").foregroundStyle(.secondary)
                        } else if near.status == "failed" {
                            Text(job.state == "approved" ? "원본 가져오기는 완료했습니다. 감상용 파일은 준비하지 못했습니다." : "감상용 파일이 준비되지 않았습니다. 검사 상태를 확인하세요.").foregroundStyle(.secondary)
                        } else {
                            Text(model.modelOutputsCurrent ? "원본과 감상용 파일을 보관했습니다." : "감상용 파일의 현재 저장 상태를 확인하세요.").foregroundStyle(.secondary)
                        }
                        if mode == .expert {
                            DisclosureGroup("원본·감상용 파일 비교") {
                                if let full = near.full { Text("원본 파일: \(full.bytes.formatted())바이트"); Text(full.sha256).textSelection(.enabled) }
                                if let reduced = near.reduced { Text("감상용 파일: \(reduced.bytes.formatted())바이트"); Text(reduced.sha256).textSelection(.enabled) }
                                if model.modelOutputsCurrent { Text("형상을 유지하며 텍스처만 줄인 파일입니다.") }
                                if let code = near.errorCode { Text(code).textSelection(.enabled) }
                            }
                        }
                    }
                    if ["queued", "processing"].contains(job.state) { ProgressView(value: Double(job.progress), total: 100).accessibilityLabel("파일 검사 진행") }
                    HStack {
                        Button("상태 다시 확인") { Task { await model.refreshImport() } }
                        if job.state == "uploading" { Button("받은 원본으로 검사 계속") { Task { await model.recoverImport() } } }
                        if job.state == "failed" { Button("검사 재시도") { Task { await model.changeImport(.retry) } }.disabled(!job.canRetry) }
                        if job.state == "approved", job.modelDerivation?.status == "failed" {
                            Button("감상용 파일 재시도") { Task { await model.changeImport(.retry) } }
                                .disabled(!job.canRetry || !model.modelDerivationEnabled || model.modelWorkerPending)
                        }
                        if !job.terminal { Button("검사 중단") { Task { await model.changeImport(.cancel) } } }
                        if job.terminal { Button("다른 파일 검사 준비") { Task { await model.clearImport() } }.disabled(model.modelWorkerPending || job.modelDerivation?.workerDrained == false) }
                    }
                    if let code = job.errorCode {
                        Text(code == "FORMAT_REJECTED" ? "지원하는 파일 형식인지 확인하세요. 다른 파일을 쓰려면 검사를 중단한 뒤 새 파일을 선택하세요." : "파일 검사를 마치지 못했습니다. 상태와 접근 권한을 확인하세요.").foregroundStyle(.secondary)
                        DisclosureGroup("검사 오류 상세") { Text(code).textSelection(.enabled) }
                    }
                }
                if !approvedAssets.isEmpty {
                    Picker("전시에 쓸 파일", selection: $model.assetId) {
                        Text("파일 선택").tag("")
                        ForEach(approvedAssets) { asset in Text("\(asset.mime == "model/gltf-binary" ? "GLB" : "PNG") · \(asset.id.prefix(8))").tag(asset.id) }
                    }
                    if needsNewRevision { Text("다른 파일을 승인하려면 작품 정보를 한 번 더 저장해 새 버전을 만드세요.").foregroundStyle(.secondary) }
                    Toggle("저장한 정보와 파일·권리를 확인했습니다", isOn: $model.approvalAcknowledged)
                    Button("작품 정보·권리 승인") { Task { await model.approveImportedArtwork() } }
                        .disabled(!model.approvalAcknowledged || model.metadataDirty || model.assetId.isEmpty || needsNewRevision)
                    if let artwork = model.selected, artwork.approvedRevision == artwork.revision { Text("저장한 이 버전은 전시에서 사용할 수 있습니다.").foregroundStyle(.secondary) }
                }
            }.frame(maxWidth: .infinity, alignment: .leading).disabled(model.busy || !ready)
          }
          Section("선택 도구 · Processing") {
            OptionalProcessingControls()
          }
        }.formStyle(.grouped).fixedSize(horizontal: false, vertical: true)
        .onChange(of: model.assetId) { _, _ in model.approvalAcknowledged = false }
    }
    private func stateTitle(_ state: String) -> String {
        switch state { case "uploading": "파일 전송 대기"; case "queued": "파일 검사 대기"; case "processing": "파일 검사 중"; case "approved": "파일 검사 완료"; case "failed": "파일 검사 실패"; case "cancelled": "파일 검사 중단됨"; default: "검사 상태 확인 필요" }
    }
}

@MainActor
private struct OptionalProcessingControls: View {
    @Environment(\.scenePhase) private var scenePhase
    @State private var found = false
    @State private var opening = false
    @State private var notice = ""
    private let bundleIdentifier = "org.exhibitos.processing"

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Processing 없이도 외부 GLB·PNG 파일로 작품을 만들 수 있습니다.")
            Text("iPhone Capture의 EXCP 파일은 Mac으로 옮겨 Processing에서 열고, 내보낸 GLB를 위에서 선택하세요.").foregroundStyle(.secondary)
            Text(notice).foregroundStyle(.secondary)
            HStack {
                Button("Processing 다시 찾기", systemImage: "magnifyingglass") { refresh() }
                Button(opening ? "Processing 여는 중…" : "Processing 열기", systemImage: "arrow.up.forward.app") {
                    Task { await open() }
                }.disabled(!found || opening)
            }.disabled(opening)
        }
        .onAppear { refresh() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active && !opening { refresh() }
        }
    }

    private func applicationURL() -> URL? {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleIdentifier),
              Bundle(url: url)?.bundleIdentifier == bundleIdentifier else { return nil }
        return url
    }

    private func refresh() {
        found = applicationURL() != nil
        notice = found ? "이 Mac에서 Processing을 찾았습니다. 필요할 때 직접 열 수 있습니다." : "이 Mac에서 Processing을 찾지 못했습니다. 설치한 뒤 다시 찾거나, 위에서 GLB·PNG 파일을 선택하세요."
    }

    private func open() async {
        guard !opening else { return }
        guard let url = applicationURL() else { refresh(); return }
        opening = true
        defer { opening = false }
        do {
            _ = try await NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
            notice = "Processing을 열었습니다. 작업을 마치면 내보낸 GLB를 여기서 선택하세요."
        } catch {
            notice = "Processing을 열지 못했습니다. Finder에서 앱의 실행 권한과 macOS 안내를 확인한 뒤 다시 시도하세요. GLB·PNG 파일은 계속 선택할 수 있습니다."
        }
    }
}
