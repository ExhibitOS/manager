// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct TrackSpotAimForm: Equatable {
    enum Mode: String, CaseIterable { case placement, point, manual }
    var rail = ""
    var mountX = "0", mountY = "0", mountZ = "0", mountYaw = "0"
    var mode: Mode = .placement
    var target = ""
    var offsetX = "0", offsetY = "0", offsetZ = "0"
    var pointX = "0", pointY = "0", pointZ = "0"
    var yaw = "0", pitch = "-45"
    var shadowTarget: String
    var prefersShadow: Bool

    init(binding: DocumentValue, light: DocumentValue) {
        shadowTarget = light["targetPlacementId"].string ?? ""
        prefersShadow = light["castsShadow"] == .bool(true)
        guard binding != .null else { return }
        let mount = binding["mount"], aim = binding["aim"]
        func text(_ value: DocumentValue) -> String { value.double.map { String($0) } ?? "" }
        func degrees(_ value: DocumentValue) -> String { value.double.map { String($0 * 180 / .pi) } ?? "" }
        rail = mount["railPlacementId"].string ?? ""
        if let xyz = mount["position"].items, xyz.count == 3 {
            (mountX, mountY, mountZ) = (text(xyz[0]), text(xyz[1]), text(xyz[2]))
        }
        mountYaw = degrees(mount["yaw"])
        switch aim["kind"].string {
        case "point":
            mode = .point
            if let xyz = aim["position"].items, xyz.count == 3 {
                (pointX, pointY, pointZ) = (text(xyz[0]), text(xyz[1]), text(xyz[2]))
            }
        case "manual":
            mode = .manual
            yaw = degrees(aim["yaw"]); pitch = degrees(aim["pitch"])
        default:
            target = aim["placementId"].string ?? ""
            if let xyz = aim["offset"].items, xyz.count == 3 {
                (offsetX, offsetY, offsetZ) = (text(xyz[0]), text(xyz[1]), text(xyz[2]))
            }
        }
    }

    func binding() throws -> DocumentValue {
        func number(_ text: String) throws -> Double {
            let normalized = text.trimmingCharacters(in: .whitespacesAndNewlines)
                .replacingOccurrences(of: Locale.current.decimalSeparator ?? ".", with: ".")
            guard let value = Double(normalized), value.isFinite else { throw AuthoringFailure("LIGHT_FIELDS_INVALID") }
            return value
        }
        func vector(_ x: String, _ y: String, _ z: String) throws -> DocumentValue {
            .vector(try [number(x), number(y), number(z)])
        }
        func angle(_ text: String) throws -> DocumentValue { .number(try number(text) * .pi / 180) }
        let aim: DocumentValue
        switch mode {
        case .placement:
            aim = try .object(["kind": .string("placement-center"), "placementId": .string(target),
                               "offset": vector(offsetX, offsetY, offsetZ)])
        case .point:
            aim = try .object(["kind": .string("point"), "position": vector(pointX, pointY, pointZ)])
        case .manual:
            aim = try .object(["kind": .string("manual"), "yaw": angle(yaw), "pitch": angle(pitch)])
        }
        return try .object(["version": .integer(1), "profile": .string(NativeTrackSpotAim.profile),
            "mount": .object(["railPlacementId": .string(rail), "position": vector(mountX, mountY, mountZ), "yaw": angle(mountYaw)]),
            "aim": aim])
    }
}

struct TrackSpotAimInspector: View {
    @ObservedObject var model: ExhibitionModel
    let ready: Bool
    let light: DocumentValue

    private struct Choice: Identifiable {
        let id: String
        let title: String
    }
    private var targets: [Choice] {
        guard let draft = model.draft else { return [] }
        return draft.placements.filter { $0["roomId"] == light["roomId"] }.compactMap { placement in
            guard let id = placement["id"].string else { return nil }
            let title = draft.artworks.first { $0["revisionId"] == placement["artworkRevisionId"] }?["metadata"]["title"].string
            return Choice(id: id, title: title ?? "작품")
        }
    }
    private var rails: [Choice] {
        let workspace = model.draft?.candidate["extensions"]["org.exhibitos.studio/workspace-furnishings"] ?? .null
        let fixtures = workspace["furnishings"].items ?? []
        let attachments = workspace["ceilingAttachments"].items ?? []
        let placements = workspace["placements"].items ?? []
        return placements.enumerated().compactMap { index, placement in
            guard placement["roomId"] == light["roomId"], placement["scale"].double == 1,
                  let id = placement["id"].string, attachments.contains(where: { $0["placementId"].string == id }),
                  let fixture = fixtures.first(where: { $0["id"] == placement["furnishingId"] }),
                  fixture["kind"] == .string("ceiling-rail"),
                  fixture["fit"]["geometry"]["profile"] == .string("passive-ceiling-rail-v1") else { return nil }
            return Choice(id: id, title: "\(fixture["name"].string ?? "천장 레일") · \(index + 1)")
        }
    }
    private func field<T>(_ path: WritableKeyPath<TrackSpotAimForm, T>, fallback: T) -> Binding<T> {
        Binding(get: { model.trackSpotAimForm?[keyPath: path] ?? fallback }, set: { value in
            guard var form = model.trackSpotAimForm else { return }
            form[keyPath: path] = value
            model.trackSpotAimForm = form
        })
    }
    private func text(_ title: String, _ path: WritableKeyPath<TrackSpotAimForm, String>) -> some View {
        TextField(title, text: field(path, fallback: ""))
    }
    var body: some View {
        if model.trackSpotAimAvailable, light["type"] == .string("spot") {
            GroupBox("트랙 스포트 조준") {
                VStack(alignment: .leading, spacing: 8) {
                    if model.trackSpotAimForm == nil {
                        Button("조준 조절형 기구 설정") { model.beginTrackSpotAim(light: light) }
                            .disabled(!ready || !model.canPreviewLightObjectAction)
                    } else {
                        Picker("같은 공간의 천장 레일", selection: field(\.rail, fallback: "")) {
                            Text("레일을 선택하세요").tag("")
                            ForEach(rails) { Text($0.title).tag($0.id) }
                        }
                        DisclosureGroup("레일 기준 장착 위치") {
                            text("장착 위치 X · m", \.mountX)
                            text("장착 위치 Y · m", \.mountY)
                            text("장착 위치 Z · m", \.mountZ)
                            text("장착 방향 · °", \.mountYaw)
                        }
                        Picker("조준 방식", selection: field(\.mode, fallback: .placement)) {
                            Text("작품 기준").tag(TrackSpotAimForm.Mode.placement)
                            Text("공간의 지점").tag(TrackSpotAimForm.Mode.point)
                            Text("각도 직접 입력").tag(TrackSpotAimForm.Mode.manual)
                        }
                        switch model.trackSpotAimForm?.mode {
                        case .placement:
                            Picker("비출 작품 배치", selection: field(\.target, fallback: "")) {
                                Text("작품을 선택하세요").tag("")
                                ForEach(targets) { Text($0.title).tag($0.id) }
                            }
                            text("작품 기준 오프셋 X · m", \.offsetX)
                            text("작품 기준 오프셋 Y · m", \.offsetY)
                            text("작품 기준 오프셋 Z · m", \.offsetZ)
                        case .point:
                            text("공간 기준 지점 X · m", \.pointX)
                            text("공간 기준 지점 Y · m", \.pointY)
                            text("공간 기준 지점 Z · m", \.pointZ)
                        case .manual:
                            text("좌우 각도 · −180–180°", \.yaw)
                            text("상하 각도 · −90–0°", \.pitch)
                        case nil: EmptyView()
                        }
                        if model.selectedLightUsesTrackAim {
                            Toggle("그림자 기준 조명으로 우선 사용", isOn: field(\.prefersShadow, fallback: false))
                            Picker("그림자 대상 작품", selection: field(\.shadowTarget, fallback: "")) {
                                Text("대상 지정 없음").tag("")
                                ForEach(targets) { Text($0.title).tag($0.id) }
                            }
                        }
                        Button("조준 미리보기") { Task { await model.previewTrackSpotAim() } }
                            .disabled(!ready || !model.canPreviewTrackSpotAim)
                        if let emitter = model.trackSpotAimPreviewEmitter, emitter.count == 3 {
                            let coordinates = emitter.map { $0.formatted(.number.precision(.fractionLength(0...3))) }
                            LabeledContent("미리보기 발광 위치 · m", value: coordinates.joined(separator: ", "))
                                .accessibilityHint("조명이 속한 공간 기준 X, Y, Z 좌표입니다.")
                        }
                        Button("조명 적용") { Task { await model.applyLight() } }
                            .disabled(!ready || !model.canApplyLight)
                        Button("조준 입력 취소") { model.discardLight() }
                            .disabled(!model.lightDirty || model.busy || model.pending || model.architectureApplying)
                    }
                }.textFieldStyle(.roundedBorder)
            }
        }
    }
}
