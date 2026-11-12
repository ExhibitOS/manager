// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import Foundation
import ManagerCore

/// A closed placement/plaque proposal, never an imported exhibition document.
struct WorkspaceInfoStandProposal: Equatable {
    let etag: String
    let modelSHA256: String
    let intakeId: String
    let replacement: WorkspaceInfoStandReplacement

    init(_ value: DocumentValue) throws {
        guard case .object(let fields) = value,
              Set(fields.keys) == ["etag", "modelSha256", "intakeId", "placement", "plaque"],
              let etag = value["etag"].string, !etag.isEmpty,
              let digest = value["modelSha256"].string,
              digest.utf8.count == 64, digest.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }),
              let intakeId = value["intakeId"].string, UUID(uuidString: intakeId) != nil else {
            throw AuthoringFailure("WORKSPACE_INFO_STAND_REPLACEMENT_INVALID")
        }
        replacement = try WorkspaceInfoStandReplacement(value: .object([
            "placement": value["placement"], "plaque": value["plaque"]]))
        self.etag = etag; modelSHA256 = digest; self.intakeId = intakeId
    }
}

struct WorkspaceInfoStandPreviewView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool

    var body: some View {
        Form {
            Section("안내대와 명판 함께 교체") {
                Text("교체안을 열어 대상 작품과 안내대·명판의 위치를 함께 확인하세요. 미리보기와 적용 전에는 전시가 바뀌지 않습니다.")
                    .foregroundStyle(.secondary)
                Button("교체안 열기…", systemImage: "doc.badge.plus") { model.openInfoStandProposal() }
                    .disabled(!ready || !model.canOpenInfoStandProposal)
                if let name = model.infoStandProposalName { Text(name).font(.caption).foregroundStyle(.secondary) }
                if let target = model.infoStandTargetTitle { LabeledContent("대상 작품", value: target) }
                if let name = model.infoStandReplacementName { LabeledContent("새 안내대", value: name) }
                if let size = model.infoStandPlaqueSize { LabeledContent("명판 크기", value: size) }
            }
            Section("공간에서 확인") {
                Button("대상 작품 보기", systemImage: "viewfinder") { Task { await model.focusInfoStandTarget() } }
                    .disabled(!ready || !model.canFocusInfoStandTarget)
                Text("대상 작품을 먼저 찾은 뒤 미리 보세요. 같은 시점을 유지하며 안내대와 명판이 바뀝니다. 미리보기 중에도 시점을 돌려 주변과의 간격을 확인할 수 있습니다.")
                    .foregroundStyle(.secondary)
                Button("교체 미리보기") { Task { await model.previewInfoStandReplacement() } }
                    .disabled(!ready || !model.canPreviewInfoStandReplacement)
                HStack {
                    Button("교체 적용") { Task { await model.applyInfoStandReplacement() } }
                        .disabled(!ready || !model.canApplyInfoStandReplacement)
                    Button("미리보기 취소", role: .cancel) { model.cancelInfoStandPreview() }
                        .disabled(model.busy || !model.infoStandPreviewing)
                }
                if !model.infoStandStatus.isEmpty { Text(model.infoStandStatus).foregroundStyle(.secondary) }
                Text("적용은 이 Mac의 편집 입력에만 보관합니다. 전시 저장은 별도로 진행하세요.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }.formStyle(.grouped)
        .onDisappear { model.cancelInfoStandPreview() }
    }
}
