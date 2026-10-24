// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import AppKit
import ManagerCore

struct NativeMaintenanceView: View {
    @ObservedObject var model: ManagerModel
    @State private var archive: URL?
    @State private var key: URL?
    @State private var confirmRestore = false
    @State private var confirmActivation = false
    @State private var targetID = ""
    @State private var confirmSwitch = false

    var body: some View {
        GroupBox("백업과 복원") {
            VStack(alignment: .leading, spacing: 12) {
                Text("백업에는 작품·전시·권리 정보와 작업 상태가 포함됩니다. 백업과 키 파일이 모두 있어야 복원할 수 있습니다.").foregroundStyle(.secondary)
                if model.hasUnsavedAuthoring { Text("현재 작품과 전시 입력을 먼저 저장하세요.").foregroundStyle(.secondary) }
                if model.finalizingRestoration { Text("복원 최종 확인은 중단할 수 없습니다. 종료를 요청하면 확인을 마친 뒤 종료합니다.").foregroundStyle(.secondary) }
                HStack {
                    Button("백업 저장…", systemImage: "externaldrive") { chooseBackup() }
                        .disabled(!model.canMaintain || model.pendingRestoration || model.native?.configuration == nil)
                    Button(model.pendingRestoration ? "같은 백업으로 복원 재개…" : "백업 복원…", systemImage: "clock.arrow.circlepath") { chooseRestore() }
                        .disabled(!model.canMaintain)
                }
                if let restoration = model.native?.restoration {
                    if restoration.state == "activated" {
                        Label("복원한 작업 공간을 확인했습니다. 준비 화면에서 로컬 제작을 시작하세요.", systemImage: "checkmark.circle")
                    } else if restoration.state == "staged" {
                        Text("복원 후보를 준비했습니다. 원래 작업 공간은 그대로 보관됩니다.")
                        Button("복원한 작업 공간 사용") { confirmActivation = true }.disabled(!model.canMaintain)
                    } else {
                        Text("중단된 복원 후보를 보관했습니다. 같은 백업과 올바른 키 파일로 다시 확인하세요.")
                    }
                }
                if model.native?.backup?.state == "completed", let destination = model.backupLocation {
                    Label("암호화 백업을 저장하고 무결성을 확인했습니다.", systemImage: "checkmark.circle")
                    Button("백업 Finder에서 보기") { NSWorkspace.shared.activateFileViewerSelecting([destination]) }
                }
                if !model.maintenanceNotice.isEmpty { Text(model.maintenanceNotice).foregroundStyle(.secondary) }
                if let context = model.context, context.installations.count > 1 {
                    Picker("보관한 작업 공간", selection: $targetID) {
                        Text("작업 공간 선택").tag("")
                        ForEach(context.installations) { installation in
                            Text("\(installation.kind == "recovery" ? "복구 작업 공간" : "원래 작업 공간") · \(installation.id.prefix(8))").tag(installation.id)
                                .disabled(!installation.available)
                        }
                    }
                    Button("선택한 작업 공간 열기") { confirmSwitch = true }
                        .disabled(!model.canMaintain || targetID.isEmpty || targetID == context.activeId)
                    Text("전환할 때 로컬 제작을 정지합니다. 각 작업 공간의 파일은 보관됩니다.").foregroundStyle(.secondary)
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        .confirmationDialog(model.pendingRestoration ? "보관한 복원을 다시 확인할까요?" : "새 작업 공간에 복원할까요?", isPresented: $confirmRestore) {
            Button(model.pendingRestoration ? "복원 재개" : "새 작업 공간에 복원") {
                guard let archive, let key else { return }
                Task { await model.restoreBackup(source: archive, key: key, createWorkspace: !model.pendingRestoration) }
            }
            if !model.pendingRestoration, model.context?.installations.first(where: { $0.id == model.context?.activeId })?.kind == "recovery" {
                Button("선택한 복구 공간에서 다시 확인") {
                    guard let archive, let key else { return }
                    Task { await model.restoreBackup(source: archive, key: key, createWorkspace: false) }
                }
            }
            Button("취소", role: .cancel) { archive = nil; key = nil }
        } message: { Text("로컬 제작을 정지하고 선택한 백업을 확인합니다. 원래 데이터와 백업 파일은 보관됩니다.") }
        .confirmationDialog("복원한 작업 공간을 사용할까요?", isPresented: $confirmActivation) {
            Button("확인하고 사용") { Task { await model.activateRestoration() } }
            Button("취소", role: .cancel) {}
        } message: { Text("작품·전시·권한·파일을 다시 확인하고 이 작업 공간의 복원을 마칩니다. 로컬 제작은 별도로 시작하세요.") }
        .confirmationDialog("작업 공간을 전환할까요?", isPresented: $confirmSwitch) {
            Button("정지하고 전환") { Task { await model.selectWorkspace(targetID) } }
            Button("취소", role: .cancel) {}
        } message: { Text("로컬 제작을 정지합니다. 현재 작업 공간은 보관됩니다.") }
    }

    private func chooseBackup() {
        let keyPanel = NSSavePanel()
        keyPanel.title = "백업 키 파일 저장"
        keyPanel.nameFieldStringValue = "ExhibitOS-backup-key.bin"
        keyPanel.message = "이 파일을 분실하면 백업을 복원할 수 없습니다. 키 파일을 안전하게 따로 보관하세요."
        guard keyPanel.runModal() == .OK, let key = keyPanel.url else { return }
        let directory = NSOpenPanel()
        directory.title = "백업을 저장할 폴더 선택"
        directory.canChooseFiles = false; directory.canChooseDirectories = true; directory.canCreateDirectories = true
        guard directory.runModal() == .OK, let parent = directory.url else { return }
        let destination = parent.appendingPathComponent("ExhibitOS-backup-\(UUID().uuidString.lowercased())", isDirectory: true)
        Task { await model.makeBackup(key: key, destination: destination) }
    }
    private func chooseRestore() {
        let directory = NSOpenPanel()
        directory.title = "암호화 백업 폴더 선택"
        directory.canChooseFiles = false; directory.canChooseDirectories = true; directory.allowsMultipleSelection = false
        guard directory.runModal() == .OK, let source = directory.url else { return }
        let keyPanel = NSOpenPanel()
        keyPanel.title = "이 백업의 키 파일 선택"
        keyPanel.canChooseFiles = true; keyPanel.canChooseDirectories = false; keyPanel.allowsMultipleSelection = false
        guard keyPanel.runModal() == .OK, let selectedKey = keyPanel.url else { return }
        archive = source; key = selectedKey; confirmRestore = true
    }
}
