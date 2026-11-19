// SPDX-License-Identifier: Apache-2.0
import ManagerCore
import SwiftUI

@MainActor
struct DefaultCatalogueUpdateView: View {
    @ObservedObject var model: ExhibitionModel
    let access: UUID
    let reload: @MainActor () async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var check: DefaultCatalogueUpdateCheck?
    @State private var receipt: DefaultCatalogueUpdateReceipt?
    @State private var activity: String?
    @State private var failure: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("기본 에셋 업데이트").font(.title2)
            if let receipt {
                LabeledContent("설치된 버전", value: receipt.active.bundleVersion)
                LabeledContent("다운로드한 용량", value: bytes(receipt.downloadedBytes))
            } else if let check {
                LabeledContent("현재 버전", value: check.active.bundleVersion)
                if let update = check.update {
                    LabeledContent("설치할 버전", value: update.bundleVersion)
                    LabeledContent("필요한 다운로드", value: bytes(update.downloadBytes))
                    if !update.added.isEmpty || !update.changed.isEmpty {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 12) {
                                changes("추가", update.added)
                                changes("변경", update.changed)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .frame(maxHeight: 220)
                    }
                } else {
                    Text("설치할 업데이트가 없습니다.").foregroundStyle(.secondary)
                }
            }
            if let activity { ProgressView(activity).controlSize(.small) }
            if let failure {
                Label(failure, systemImage: "exclamationmark.circle")
                    .font(.callout)
            }
            HStack {
                Spacer()
                Button("닫기") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(activity != nil)
                if check?.update != nil && receipt == nil && failure == nil {
                    Button("설치") { Task { await install() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(activity != nil || model.busy || access != model.viewportAccessID)
                } else if failure != nil {
                    Button("다시 확인") { Task { await refresh() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(activity != nil || model.busy || access != model.viewportAccessID)
                }
            }
        }
        .padding(24)
        .frame(minWidth: 420, idealWidth: 460)
        .interactiveDismissDisabled(activity != nil)
        .task { await refresh() }
    }

    @ViewBuilder
    private func changes(_ title: String, _ items: [DefaultCatalogueUpdateCheck.Change]) -> some View {
        if !items.isEmpty {
            Text(title).font(.headline)
            ForEach(items) { item in
                LabeledContent(item.id, value: "v\(item.version)")
            }
        }
    }

    private func bytes(_ count: Int64) -> String {
        "\(count.formatted())바이트"
    }

    private func refresh() async {
        guard activity == nil else { return }
        activity = "업데이트 확인 중"
        check = nil
        receipt = nil
        failure = nil
        defer { activity = nil }
        do {
            check = try await model.withWorkspaceFurnishingAccess(access) { api in
                try await api.checkDefaultCatalogueUpdate()
            }
        } catch {
            failure = catalogueFailure(error, installation: false)
        }
    }

    private func install() async {
        guard activity == nil, let check else { return }
        activity = "기본 에셋 설치 중"
        failure = nil
        defer { activity = nil }
        do {
            receipt = try await model.withWorkspaceFurnishingAccess(access) { api in
                try await api.installDefaultCatalogueUpdate(check)
            }
            await reload()
        } catch {
            failure = catalogueFailure(error, installation: true)
        }
    }

    private func catalogueFailure(_ error: Error, installation: Bool) -> String {
        switch (error as? AuthoringFailure)?.code {
        case "DEFAULT_CATALOGUE_STALE": "카탈로그 상태가 바뀌었습니다. 업데이트를 다시 확인하세요."
        case "DEFAULT_CATALOGUE_INCOMPATIBLE": "이 버전의 Manager에서 설치할 수 없는 에셋입니다."
        case "DEFAULT_CATALOGUE_INTEGRITY", "DEFAULT_CATALOGUE_UNTRUSTED": "업데이트를 검증하지 못했습니다."
        default:
            installation ? "설치 결과를 확인하지 못했습니다. 업데이트를 다시 확인하세요."
                : "업데이트를 확인하지 못했습니다."
        }
    }
}
