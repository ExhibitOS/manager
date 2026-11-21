// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct WallFinishSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var viewport: NativeViewportController
    let ready: Bool
    private func field<Value>(_ key: WritableKeyPath<WallFinishFormBuffer, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { model.wallFinish?[keyPath: key] ?? fallback }, set: { value in
            guard var fields = model.wallFinish else { return }; fields[keyPath: key] = value; model.wallFinish = fields
        })
    }
    var body: some View {
        if let fields = model.wallFinish {
            Section("벽 마감") {
                Toggle("선택한 벽에 마감 사용", isOn: field(\.enabled, fallback: false))
                    .disabled(!ready || !model.canEditWallFinish)
                if fields.enabled {
                    VStack(alignment: .leading, spacing: 8) {
                        Picker("마감", selection: field(\.preset, fallback: "plain")) {
                            Text("기본").tag("plain")
                            Text("도장한 테두리").tag("painted-edge")
                            Text("짙은 테두리").tag("dark-edge")
                        }
                        TextField("벽 마감 깊이 · 0.02–0.2 m", text: field(\.depth, fallback: ""))
                        TextField("테두리 폭 · 0.02–0.08 m", text: field(\.trimWidth, fallback: ""))
                        if fields.preset == "plain" {
                            Toggle("걸레받이 표시", isOn: field(\.plainBaseboard, fallback: false))
                                .toggleStyle(.checkbox)
                        }
                        TextField("걸레받이 높이 · 0.04–0.15 m", text: field(\.baseboardHeight, fallback: ""))
                            .disabled(fields.preset == "plain" && !fields.plainBaseboard)
                        Toggle("천장 가장자리 마감", isOn: field(\.ceilingBand, fallback: true))
                    }.textFieldStyle(.roundedBorder).disabled(!ready || !model.canEditWallFinish)
                }
                Text("선택한 벽의 기존 형상에 마감을 더합니다. 미리보기에서 문·작품과의 간격을 확인하세요.")
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("벽 마감 미리보기") { Task { await model.previewWallFinish() } }.disabled(!ready || !model.canPreviewWallFinish)
                    Button("벽 마감 적용") { Task { await model.applyWallFinish() } }.disabled(!ready || !model.canApplyWallFinish)
                }
                Button("벽 마감 입력 취소") { model.discardWallFinish() }
                    .disabled(!model.wallFinishDirty || model.busy || model.pending || model.architectureApplying)
                if !model.viewportController.wallFinishSupported {
                    Text("현재 연결에서는 벽 마감 편집을 지원하지 않습니다. 기존 설정과 입력은 유지됩니다.").font(.caption).foregroundStyle(.secondary)
                }
                if !model.wallFinishStatus.isEmpty { Text(model.wallFinishStatus).font(.callout).foregroundStyle(.secondary) }
            }
        } else if !model.wallFinishStatus.isEmpty { Section("벽 마감") { Text(model.wallFinishStatus).foregroundStyle(.secondary) } }
    }
}

struct PairedWallStructureSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool

    var body: some View {
        ForEach(model.pairedWallTargets) { target in
            PairedWallStructureRow(model: model, viewport: model.viewportController, target: target, ready: ready)
        }
    }
}

private struct PairedWallStructureRow: View {
    @ObservedObject var model: ExhibitionModel
    @ObservedObject var viewport: NativeViewportController
    let target: NativePairedWallStructure.Target
    let ready: Bool
    @State private var depth = ""

    private var meters: Double? {
        guard let millimeters = Double(depth.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: Locale.current.decimalSeparator ?? ".", with: ".")),
              millimeters.isFinite, millimeters > 0,
              millimeters / 1000 <= NativePairedWallStructure.maximumExtensionMeters else { return nil }
        return millimeters / 1000
    }

    var body: some View {
        Section("연결 벽체") {
            LabeledContent("연결된 공간", value: target.peerRoomName)
            if let extent = target.extensionMeters {
                LabeledContent("현재 확장", value: (extent * 1000).formatted() + " mm")
            }
            TextField("이 공간 쪽 확장 (mm, 최대 120)", text: $depth)
                .textFieldStyle(.roundedBorder)
                .disabled(!ready || !model.canPreviewPairedWall)
            if model.pairedWallPreviewTarget == target.id {
                HStack {
                    Button("벽체 변경 적용") { Task { await model.applyPairedWall() } }
                        .disabled(!ready || !viewport.connected || !model.canApplyPairedWall)
                    Button("미리보기 취소") { model.cancelPairedWallPreview() }
                        .disabled(model.busy)
                }
            } else {
                HStack {
                    Button("확장 미리보기") {
                        guard let meters else { return }
                        Task { await model.previewPairedWall(target: target, extensionMeters: meters) }
                    }
                    .disabled(!ready || !model.canPreviewPairedWall || meters == nil)
                    if target.extensionMeters != nil {
                        Button("확장 제거 미리보기") {
                            Task { await model.previewPairedWall(target: target, extensionMeters: nil) }
                        }
                        .disabled(!ready || !model.canPreviewPairedWall)
                    }
                }
            }
        }
    }
}
