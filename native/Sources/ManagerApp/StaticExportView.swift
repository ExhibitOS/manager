// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

@MainActor
final class StaticExportModel: ObservableObject {
    @Published private(set) var status: NativeStaticExportStatus?
    @Published private(set) var choosing = false
    @Published private(set) var failure: BridgeFailure?
    @Published private(set) var notice = ""
    var running: Bool { status?.running == true }
    private var read: (() async throws -> NativeStaticExportStatus?)?
    private var start: ((String, URL) async throws -> NativeStaticExportStatus)?
    private var cancel: (() async throws -> NativeStaticExportStatus?)?
    private var poll: Task<Void, Never>?
    func connect(read: @escaping () async throws -> NativeStaticExportStatus?, start: @escaping (String, URL) async throws -> NativeStaticExportStatus, cancel: @escaping () async throws -> NativeStaticExportStatus?) async {
        self.read = read; self.start = start; self.cancel = cancel
        do { status = try await read(); if running { follow() } }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED") }
    }
    private func follow() {
        guard poll == nil else { return }
        poll = Task { [weak self] in
            defer { self?.poll = nil }
            while let self, !Task.isCancelled, self.running {
                do {
                    try await Task.sleep(for: .milliseconds(250))
                    guard !Task.isCancelled, let read = self.read else { return }
                    self.status = try await read()
                } catch is CancellationError { return }
                catch { self.failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED"); return }
            }
        }
    }
    func chooseAndExport(_ completed: NativePublicationCompleted) async {
        guard !choosing, !running, let start else { return }
        choosing = true; failure = nil; notice = ""
        defer { choosing = false }
        let panel = NSOpenPanel()
        panel.title = "정적 전시 저장 위치"
        panel.message = "선택한 위치에 새 전시 폴더를 만듭니다. 기존 파일은 그대로 보관됩니다."
        panel.prompt = "이 위치에 저장"
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.allowsMultipleSelection = false; panel.canCreateDirectories = true
        guard let window = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.identifier?.rawValue == "manager" }) else { failure = BridgeFailure("NATIVE_STATIC_OUTPUT_INVALID"); return }
        let answer = await withCheckedContinuation { continuation in panel.beginSheetModal(for: window) { continuation.resume(returning: $0) } }
        guard answer == .OK, let parent = panel.url else { notice = "저장을 취소했습니다."; return }
        let output = parent.resolvingSymlinksInPath().appendingPathComponent("ExhibitOS-전시-" + String(UUID().uuidString.lowercased().prefix(8)), isDirectory: true)
        do { status = try await start(completed.publication.publicationId, output); follow() }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED") }
    }
    func cancelExport() async {
        guard let cancel else { return }
        do { status = try await cancel(); if running { follow() } }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED") }
    }
    func cancelAndWait() async -> Bool {
        guard running else { return true }
        await cancelExport()
        let end = ContinuousClock.now.advanced(by: .seconds(15))
        while running, ContinuousClock.now < end {
            do { try await Task.sleep(for: .milliseconds(100)); if let read { status = try await read() } }
            catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED"); return false }
        }
        return !running
    }
}

struct NativeStaticExportView: View {
    @ObservedObject var model: StaticExportModel
    let completed: NativePublicationCompleted?
    let ready: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Divider()
            Text("정적 전시 파일 저장").font(.headline)
            Text("호스팅에 올릴 Viewer와 표시용 작품 파일을 새 폴더에 저장합니다.").foregroundStyle(.secondary)
            if let completed, completed.publication.status == "published" {
                Button("정적 전시 폴더 저장…") { Task { await model.chooseAndExport(completed) } }.disabled(!ready || model.running || model.choosing)
            } else { Text("먼저 위에서 공개용 저장본을 준비하세요.").foregroundStyle(.secondary) }
            if model.running { ProgressView("전시 파일을 저장하고 확인하고 있습니다…"); Button("파일 저장 중단") { Task { await model.cancelExport() } } }
            if let value = model.status {
                if value.state == "completed", let receipt = value.receipt {
                    if completed?.publication.publicationId != receipt.publicationId { Text("이전에 준비한 전시의 저장 결과입니다.").foregroundStyle(.secondary) }
                    Label("정적 전시 파일 저장 완료", systemImage: "checkmark.circle")
                    Text("파일 \(receipt.files)개 · \(ByteCountFormatter.string(fromByteCount: Int64(receipt.bytes), countStyle: .file))")
                    Text(receipt.output).textSelection(.enabled)
                    Button("Finder에서 보기") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: receipt.output)]) }
                    Text("이 폴더는 아직 호스팅에 업로드되지 않았습니다.").foregroundStyle(.secondary)
                    DisclosureGroup("확인한 저장본") { Text(receipt.publicationId).textSelection(.enabled); Text(receipt.manifestSha256).textSelection(.enabled) }
                } else if !value.running {
                    Text(value.state == "cancelled" ? "파일 저장을 중단했습니다." : "파일 저장을 완료하지 못했습니다.")
                    Text("기존 파일은 보관됩니다. 위 버튼으로 새 폴더에 다시 저장할 수 있습니다.").foregroundStyle(.secondary)
                    DisclosureGroup("오류 상세") { Text(value.errorCode ?? "NATIVE_STATIC_EXPORT_FAILED").textSelection(.enabled); Text(value.output).textSelection(.enabled) }
                }
            }
            if let failure = model.failure { Text(failure.localizedDescription); DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) } }
            if !model.notice.isEmpty { Text(model.notice).foregroundStyle(.secondary) }
        }
    }
}
