// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct PlacedExitDoorSettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    let targets: [PlacedExitDoorSettings.Target]
    @State private var placement = ""
    @State private var wall = ""
    @State private var primary = DestinationFields()
    @State private var additional: [DestinationFields] = []

    private struct DestinationFields: Identifiable {
        let id = UUID()
        var name = ""
        var url = ""
        init() {}
        init(_ destination: PlacedExitDoorSettings.Destination) {
            name = destination.name
            url = destination.url
        }
        func validated() throws -> PlacedExitDoorSettings.Destination { try .init(name: name, url: url) }
    }

    private var selected: PlacedExitDoorSettings.Target? { targets.first { $0.id == placement } }
    private var editable: Bool { ready && model.canEditPlacedExitDoor }
    private var settings: PlacedExitDoorSettings? {
        try? .init(placementId: placement, surfaceId: wall, primary: primary.validated(), additional: additional.map { try $0.validated() })
    }

    var body: some View {
        Section("출구 문 목적지") {
            Picker("배치된 출구 문", selection: $placement) {
                ForEach(Array(targets.enumerated()), id: \.element.id) { index, target in
                    Text("출구 문 \(index + 1)").tag(target.id)
                }
            }
            Picker("연결된 벽", selection: $wall) {
                ForEach(Array((selected?.wallIds ?? []).enumerated()), id: \.element) { index, id in
                    Text("벽 \(index + 1)").tag(id)
                }
            }
            destinationFields($primary, label: "기본 목적지")
            ForEach($additional) { $destination in
                destinationFields($destination, label: "추가 목적지")
                Button("이 목적지 제거", role: .destructive) {
                    let id = destination.id
                    additional.removeAll { $0.id == id }
                }
            }
            Button("추가 목적지") { additional.append(DestinationFields()) }.disabled(additional.count >= 4)
            Button("목적지 적용") {
                guard let settings, let revision = model.state?.revision else { return }
                Task { await model.applyPlacedExitDoor(settings, revision: revision) }
            }.disabled(settings == nil)
        }
        .disabled(!editable)
        .onChange(of: targets, initial: true) {
            if selected == nil { placement = targets.first?.id ?? "" }
            load()
        }
        .onChange(of: placement) { load() }
        .onChange(of: model.state?.revision) { load() }
    }

    private func destinationFields(_ fields: Binding<DestinationFields>, label: String) -> some View {
        Group {
            TextField("\(label) 이름", text: fields.name)
            TextField("\(label) HTTPS 주소", text: fields.url, prompt: Text("https://example.org/"))
        }
    }

    private func load() {
        let current = model.draft.flatMap { try? PlacedExitDoorSettings.read(in: $0, placementId: placement) }
        wall = current.flatMap { row in selected?.wallIds.contains(row.surfaceId) == true ? row.surfaceId : nil }
            ?? selected?.wallIds.first ?? ""
        primary = current.map { DestinationFields($0.primary) } ?? DestinationFields()
        additional = current?.additional.map(DestinationFields.init) ?? []
    }
}
