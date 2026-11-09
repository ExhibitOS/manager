// SPDX-License-Identifier: Apache-2.0
import ManagerCore
import SwiftUI

@MainActor
struct WorkspaceBenchSeatEditor: View {
    @ObservedObject var model: ExhibitionModel
    let entry: WorkspaceFurnishingSummary
    let access: UUID
    @State private var placementId = ""
    @State private var form: BenchFormBuffer?
    @State private var applying = false
    @State private var status: String?

    private var placements: [WorkspaceFurnishingPlacement] {
        guard let draft = model.draft else { return [] }
        return (try? WorkspaceBenchSeatAttachment.placements(furnishingId: entry.id, version: entry.version, in: draft)) ?? []
    }

    var body: some View {
        if !placements.isEmpty {
            Section("등록한 벤치에 착석 설정") {
                Picker("배치 선택", selection: $placementId) {
                    Text("배치를 선택하세요").tag("")
                    ForEach(placements, id: \.id) { placement in
                        let room = model.draft?.rooms.first { $0["id"].string == placement.roomId }?["name"].string ?? "공간"
                        Text("\(room) · X \(placement.position.first ?? 0), Z \(placement.position.last ?? 0) m").tag(placement.id)
                    }
                }
                .onChange(of: placementId) { _, id in selectPlacement(id) }
                if let form {
                    text("좌석 너비 · m", \.width)
                    text("좌석 깊이 · m", \.depth)
                    text("좌석 높이 · m", \.height)
                    Toggle("새 감상 시점 만들기", isOn: field(\.usesFreshViewpoint, fallback: true))
                    if form.usesFreshViewpoint {
                        Button("벤치 방향에 시점 맞추기") {
                            do { try self.form?.matchNewViewpointToBench(); status = nil }
                            catch { status = "좌석 치수를 먼저 입력하세요." }
                        }
                        text("감상 시점 이름", \.viewpointName)
                        text("눈 위치 X · m", \.viewX)
                        text("눈 위치 Y · m", \.viewY)
                        text("눈 위치 Z · m", \.viewZ)
                        text("바라볼 위치 X · m", \.targetX)
                        text("바라볼 위치 Y · m", \.targetY)
                        text("바라볼 위치 Z · m", \.targetZ)
                        text("시야각 · °", \.fov)
                    } else {
                        let views = model.draft.flatMap { try? BenchViewpoint.list(in: $0) } ?? []
                        Picker("감상 시점", selection: field(\.viewpointId, fallback: "")) {
                            Text("같은 공간의 시점을 선택하세요").tag("")
                            ForEach(views.filter { $0.roomId == form.roomId }, id: \.id) { Text($0.name).tag($0.id) }
                        }
                    }
                    Text("좌석 치수와 시점을 확인한 뒤 적용하세요. 벤치 모델과 배치 위치는 유지됩니다.")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("착석 설정 적용") { Task { await apply() } }
                        .disabled(!model.canEditWorkspaceBenchSeat || (try? form.settings()) == nil || (form.usesFreshViewpoint && (try? form.newViewpoint()) == nil))
                }
                if let status { Text(status).accessibilityAddTraits(.updatesFrequently) }
            }
            .disabled(applying)
        }
    }

    private func field<Value>(_ key: WritableKeyPath<BenchFormBuffer, Value>, fallback: Value) -> Binding<Value> {
        Binding(get: { form?[keyPath: key] ?? fallback }, set: { form?[keyPath: key] = $0 })
    }

    private func text(_ label: String, _ key: WritableKeyPath<BenchFormBuffer, String>) -> some View {
        TextField(label, text: field(key, fallback: ""))
    }

    private func selectPlacement(_ id: String) {
        form = nil; status = nil
        guard let placement = placements.first(where: { $0.id == id }), let draft = model.draft,
              placement.position.count == 3 else { return }
        do {
            if let bench = try BenchSettings.list(in: draft).first(where: { $0.workspaceVisual?.placementId == id }) {
                form = try BenchFormBuffer(draft: draft, benchId: bench.id)
            } else {
                var fields = try BenchFormBuffer(roomId: placement.roomId, name: entry.name)
                fields.x = String(placement.position[0]); fields.y = String(placement.position[1]); fields.z = String(placement.position[2])
                fields.yawDegrees = String(placement.yaw * 180 / .pi)
                fields.width = ""; fields.depth = ""; fields.height = ""
                fields.viewX = ""; fields.viewY = ""; fields.viewZ = ""
                fields.targetX = ""; fields.targetY = ""; fields.targetZ = ""
                form = fields
            }
        } catch { status = "선택한 배치의 착석 설정을 읽을 수 없습니다." }
    }

    private func apply() async {
        applying = true
        defer { applying = false }
        do {
            guard let form, let draft = model.draft,
                  let hash = draft.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"].items?
                    .first(where: { $0["id"].string == entry.id && $0["version"].integer == Int64(entry.version) })?["model"]["sha256"].string else {
                throw AuthoringFailure("BENCH_SELECTION_CHANGED")
            }
            let settings = try form.settings()
            let attachment = try WorkspaceBenchSeatAttachment(benchId: form.id, placementId: placementId, furnishingId: entry.id,
                furnishingVersion: entry.version, modelSha256: hash, width: settings.width, depth: settings.depth, height: settings.height,
                viewpointId: settings.viewpointId, viewpoint: form.newViewpoint())
            let changed = try await model.applyWorkspaceBenchSeat(attachment, access: access)
            status = changed ? "착석 설정을 적용했습니다. 전시를 저장하세요." : "같은 착석 설정이 이미 저장되어 있습니다."
        } catch {
            status = (error as? AuthoringFailure)?.errorDescription ?? "착석 설정을 확인할 수 없습니다."
        }
    }
}
