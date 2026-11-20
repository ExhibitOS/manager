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
    @Published private(set) var starting = false
    @Published private(set) var inspectingBudget = false
    @Published private(set) var openingViewer = false
    @Published var viewer: NativeStaticViewerEntry?
    private var exportGeneration = UUID()
    private var viewerGeneration = UUID()
    @Published private var budgetInspection: (publication: NativePublication, report: DocumentValue?)?

    func budgetInspection(for publication: NativePublication) -> (report: DocumentValue?, attempted: Bool) {
        guard let budgetInspection, budgetInspection.publication == publication else { return (nil, false) }
        return (budgetInspection.report, true)
    }
    var running: Bool { status?.running == true }
    private var read: (() async throws -> NativeStaticExportStatus?)?
    private var start: ((String, URL, Bool) async throws -> NativeStaticExportStatus)?
    private var cancel: (() async throws -> NativeStaticExportStatus?)?
    private var openPreview: ((String) async throws -> NativeStaticViewerEntry)?
    private var closePreview: ((String) async throws -> Void)?
    @Published private var viewerReservation: (id: String, close: (String) async throws -> Void)?
    private var closingViewer = false
    var canOpenViewer: Bool { openPreview != nil && !openingViewer && viewerReservation == nil }
    var viewerClosePending: Bool { viewer == nil && viewerReservation != nil }
    private var poll: Task<Void, Never>?
    func connect(read: @escaping () async throws -> NativeStaticExportStatus?, start: @escaping (String, URL, Bool) async throws -> NativeStaticExportStatus, cancel: @escaping () async throws -> NativeStaticExportStatus?,
                 openPreview: @escaping (String) async throws -> NativeStaticViewerEntry, closePreview: @escaping (String) async throws -> Void) async {
        await closeViewer()
        exportGeneration = UUID()
        budgetInspection = nil
        self.read = read; self.start = start; self.cancel = cancel
        self.openPreview = openPreview; self.closePreview = closePreview
        do { status = try await read(); if running { follow() } }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED") }
    }
    func showViewer(_ completed: NativeStaticExportStatus) async {
        guard canOpenViewer, status?.jobId == completed.jobId, completed.state == "completed",
              let receipt = completed.receipt, let openPreview, let closePreview else { return }
        let generation = viewerGeneration
        openingViewer = true; failure = nil
        defer { openingViewer = false }
        do {
            let entry = try await openPreview(completed.jobId)
            viewerReservation = (entry.id, closePreview)
            guard !Task.isCancelled, generation == viewerGeneration, status?.jobId == completed.jobId else {
                await closeViewer()
                return
            }
            guard entry.revisionSha256 == receipt.revisionSha256,
                  let origin = URLComponents(url: entry.url, resolvingAgainstBaseURL: false),
                  origin.scheme == "http", origin.host == "127.0.0.1",
                  let port = origin.port, port >= 1024, origin.path == "/index.html",
                  origin.user == nil, origin.password == nil, origin.query == nil, origin.fragment == nil else {
                await closeViewer()
                throw BridgeFailure("NATIVE_STATIC_VIEWER_RECEIPT_INVALID")
            }
            viewer = entry
        } catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_VIEWER_UNAVAILABLE") }
    }
    func closeViewer(id: String? = nil) async {
        guard id == nil || viewerReservation?.id == id else { return }
        viewerGeneration = UUID()
        viewer = nil
        guard !closingViewer, let reservation = viewerReservation else { return }
        closingViewer = true
        defer { closingViewer = false }
        do {
            try await reservation.close(reservation.id)
            viewerReservation = nil
        }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_VIEWER_UNAVAILABLE") }
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
    func chooseAndExport(_ completed: NativePublicationCompleted, privateEditor: Bool = false,
                         isCurrent: () -> Bool, inspectBudget: (() async throws -> DocumentValue)? = nil) async {
        guard !choosing, !running, start != nil, isCurrent() else { return }
        let generation = exportGeneration
        choosing = true; failure = nil; notice = ""
        defer { choosing = false }
        let panel = NSOpenPanel()
        panel.title = privateEditor ? "개인 사본 편집을 포함한 전시 저장 위치" : "정적 전시 저장 위치"
        panel.message = "선택한 위치에 새 전시 폴더를 만듭니다. 기존 파일은 그대로 보관됩니다."
        panel.prompt = "이 위치에 저장"
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.allowsMultipleSelection = false; panel.canCreateDirectories = true
        guard let window = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.identifier?.rawValue == "manager" }) else { failure = BridgeFailure("NATIVE_STATIC_OUTPUT_INVALID"); return }
        let answer = await withCheckedContinuation { continuation in panel.beginSheetModal(for: window) { continuation.resume(returning: $0) } }
        guard answer == .OK, let parent = panel.url else { notice = "저장을 취소했습니다."; return }
        guard generation == exportGeneration, !Task.isCancelled else { return }
        let prefix = privateEditor ? "ExhibitOS-개인사본-" : "ExhibitOS-전시-"
        let output = parent.resolvingSymlinksInPath().appendingPathComponent(prefix + String(UUID().uuidString.lowercased().prefix(8)), isDirectory: true)
        await export(completed.publication, to: output, privateEditor: privateEditor,
                     isCurrent: isCurrent, inspectBudget: inspectBudget)
    }

    func export(_ publication: NativePublication, to output: URL, privateEditor: Bool,
                isCurrent: () -> Bool, inspectBudget: (() async throws -> DocumentValue)?) async {
        guard !starting, !running, let start else { return }
        guard isCurrent() else { failure = BridgeFailure("NATIVE_PUBLICATION_STATE_CHANGED"); return }
        let generation = exportGeneration
        starting = true
        defer { starting = false }
        if !privateEditor, let inspectBudget {
            inspectingBudget = true
            budgetInspection = (publication, nil)
            let report = try? await inspectBudget()
            inspectingBudget = false
            guard generation == exportGeneration, !Task.isCancelled else { return }
            budgetInspection = (publication, report)
        }
        guard isCurrent() else {
            budgetInspection = nil
            failure = BridgeFailure("NATIVE_PUBLICATION_STATE_CHANGED")
            return
        }
        do { status = try await start(publication.publicationId, output, privateEditor); follow() }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED") }
    }
    func cancelExport() async {
        guard let cancel else { return }
        do { status = try await cancel(); if running { follow() } }
        catch { failure = error as? BridgeFailure ?? BridgeFailure("NATIVE_STATIC_EXPORT_FAILED") }
    }
    func cancelAndWait() async -> Bool {
        exportGeneration = UUID()
        await closeViewer()
        if inspectingBudget { budgetInspection = nil }
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
    let inspectBudget: (NativePublicationCompleted) async throws -> DocumentValue
    let isCurrent: (NativePublicationCompleted) -> Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Divider()
            Text("정적 전시 파일 저장").font(.headline)
            Text("호스팅에 올릴 Viewer와 감상용 작품 파일을 새 폴더에 저장합니다.").foregroundStyle(.secondary)
            if let completed, completed.publication.status == "published" {
                let budget = model.budgetInspection(for: completed.publication)
                if budget.attempted {
                    PreparedViewerBudgetStatusView(analysis: budget.report, calculating: model.inspectingBudget)
                }
                Button("정적 전시 폴더 저장…") {
                    Task {
                        await model.chooseAndExport(completed, isCurrent: { isCurrent(completed) },
                                                    inspectBudget: { try await inspectBudget(completed) })
                    }
                }.disabled(!ready || model.running || model.choosing)
                Button("개인 사본 편집을 포함해 저장…") {
                    Task { await model.chooseAndExport(completed, privateEditor: true, isCurrent: { isCurrent(completed) }) }
                }
                    .disabled(!ready || model.running || model.choosing)
                Text("CC0 작품과 자료로 개인 사본을 편집할 수 있는 로컬 폴더를 만듭니다.").font(.callout).foregroundStyle(.secondary)
            } else { Text("먼저 위에서 공개용 저장본을 준비하세요.").foregroundStyle(.secondary) }
            if model.starting && !model.running && !model.inspectingBudget { ProgressView("정적 전시 파일 저장을 준비하고 있습니다…") }
            if model.running {
                if model.failure == nil { ProgressView("전시 파일을 저장하고 확인하고 있습니다…") }
                Button("파일 저장 중단") { Task { await model.cancelExport() } }
            }
            if let value = model.status {
                if value.state == "completed", let receipt = value.receipt {
                    if completed?.publication.publicationId != receipt.publicationId { Text("이전에 준비한 전시의 저장 결과입니다.").foregroundStyle(.secondary) }
                    Label(value.privateEditor == true ? "개인 사본 편집을 포함한 전시 파일 저장 완료" : "정적 전시 파일 저장 완료", systemImage: "checkmark.circle")
                    Text("파일 \(receipt.files)개 · \(ByteCountFormatter.string(fromByteCount: Int64(receipt.bytes), countStyle: .file))")
                    Text(receipt.output).textSelection(.enabled)
                    Button("내보낸 전시 열기") { Task { await model.showViewer(value) } }
                        .disabled(!model.canOpenViewer)
                    if model.openingViewer { ProgressView("내보낸 전시 파일을 확인하고 있습니다…") }
                    Button("Finder에서 보기") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: receipt.output)]) }
                    if value.privateEditor != true { Text("인터넷 공개 결과는 아래 온라인 공개에서 확인하세요.").foregroundStyle(.secondary) }
                    DisclosureGroup("확인한 저장본") { Text(receipt.publicationId).textSelection(.enabled); Text(receipt.manifestSha256).textSelection(.enabled) }
                } else if !value.running {
                    Text(value.state == "cancelled" ? "파일 저장을 중단했습니다." : "파일 저장을 완료하지 못했습니다.")
                    if let code = value.errorCode { Text(BridgeFailure(code).localizedDescription).foregroundStyle(.secondary) }
                    Text("위 버튼으로 새 폴더에 다시 저장할 수 있습니다.").foregroundStyle(.secondary)
                    DisclosureGroup("오류 상세") { Text(value.errorCode ?? "NATIVE_STATIC_EXPORT_FAILED").textSelection(.enabled); Text(value.output).textSelection(.enabled) }
                }
            }
            if let failure = model.failure { Text(failure.localizedDescription); DisclosureGroup("오류 상세") { Text(failure.code).textSelection(.enabled) } }
            if model.viewerClosePending { Button("전시 닫기 다시 시도") { Task { await model.closeViewer() } } }
            if !model.notice.isEmpty { Text(model.notice).foregroundStyle(.secondary) }
        }
        .sheet(item: $model.viewer) { entry in
            NativeStaticViewerSheet(entry: entry) { Task { await model.closeViewer(id: entry.id) } }
                .onDisappear { Task { await model.closeViewer(id: entry.id) } }
        }
    }
}
