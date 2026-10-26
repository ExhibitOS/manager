// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

/// Controls edit the existing model-owned candidate. Thumbnail and live entry
/// presentation belong to the shared Viewer host, never a native imitation.
struct CoverEntrySettingsView: View {
    @ObservedObject var model: ExhibitionModel
    let mode: Mode

    private func choice(_ group: String?, _ name: String) -> Binding<String> {
        Binding(get: { model.coverEntry.design?.field(group, name).string ?? "" }, set: { value in
            model.coverEntry.set(group, name, value.isEmpty ? nil : .string(value))
        })
    }
    private func flag(_ group: String, _ name: String) -> Binding<String> {
        Binding(get: {
            guard let value = model.coverEntry.design?.field(group, name), case .bool(let enabled) = value else { return "" }
            return enabled ? "yes" : "no"
        }, set: { model.coverEntry.set(group, name, $0.isEmpty ? nil : .bool($0 == "yes")) })
    }
    private func text(_ field: CoverEntryFormBuffer.NumberField) -> Binding<String> {
        Binding(get: { model.coverEntry.numbers[field] ?? "" }, set: { model.coverEntry.numbers[field] = $0 })
    }
    private func toggleChoice(_ title: String, _ group: String, _ key: String) -> some View {
        Picker(title, selection: flag(group, key)) {
            Text("테마 기본값").tag(""); Text("켜기").tag("yes"); Text("끄기").tag("no")
        }
    }
    private func translated(_ id: UUID, _ key: String) -> Binding<String> {
        Binding(get: { model.coverEntry.translations.first(where: { $0.id == id })?.fields[key].string ?? "" }, set: { text in
            guard let i = model.coverEntry.translations.firstIndex(where: { $0.id == id }) else { return }
            model.coverEntry.translations[i].fields[key] = .string(text)
        })
    }
    private var focus: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("이미지 초점")
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 140), alignment: .leading)], alignment: .leading) {
                    ForEach(0..<9, id: \.self) { index in
                        let row = index / 3, column = index % 3
                        let title = ["위", "가운데", "아래"][row] + " · " + ["왼쪽", "가운데", "오른쪽"][column]
                        Button(title) {
                            model.coverEntry.numbers[.focusX] = String(Double(column) / 2)
                            model.coverEntry.numbers[.focusY] = String(Double(row) / 2)
                        }.help("자르기에 사용할 초점을 " + title + "에 둡니다.")
                    }
            }
            Button("초점 기본값 사용") { model.coverEntry.numbers[.focusX] = ""; model.coverEntry.numbers[.focusY] = "" }
        }
    }
    private var advanced: some View {
        Group {
            DisclosureGroup("배치와 글자 크기") {
                TextField("글 영역 비율 · 0.30–0.70", text: text(.copyRatio))
                    .disabled(model.coverEntry.design?.theme == .catalogue)
                if model.coverEntry.design?.theme == .catalogue { Text("전시 도록은 좌우 절반 비율을 유지합니다.").foregroundStyle(.secondary) }
                TextField("여백 · 0–64", text: text(.margin))
                TextField("제목 배율 · 0.75–1.5", text: text(.titleScale))
                TextField("초점 X · 0–1", text: text(.focusX))
                TextField("초점 Y · 0–1", text: text(.focusY))
                Text("빈 숫자 입력은 테마 기본값을 사용합니다. 초점 좌표는 두 값을 함께 입력하세요.").foregroundStyle(.secondary)
            }
            DisclosureGroup("연출 상세") {
                ForEach(["cover", "response", "transition"], id: \.self) { stage in
                    Picker(["cover": "표지", "response": "입장 응답", "transition": "관람 전환"][stage]!, selection: choice("effects", stage)) {
                        Text("테마 기본값").tag("")
                        ForEach((model.coverEntry.design?.theme ?? .catalogue).effects(for: stage), id: \.self) { effect in
                            Text(effectTitle(effect)).tag(effect)
                        }
                    }
                }
                TextField("연출 강도 · 0–1", text: text(.strength))
                TextField("연출 속도 · 0.5–2", text: text(.speed))
                Picker("모바일 연출", selection: choice("effects", "mobile")) {
                    Text("테마 기본값").tag(""); Text("간단하게").tag("simple"); Text("없음").tag("none")
                }
                Text("관람자의 동작 줄이기 설정이 우선합니다. 저장한 설정은 바뀌지 않습니다.").foregroundStyle(.secondary)
            }
            DisclosureGroup("언어별 글") {
                ForEach(model.coverEntry.translations) { row in
                    VStack(alignment: .leading, spacing: 8) {
                        TextField("언어 코드 · 예: ko-KR", text: translated(row.id, "locale"))
                        TextField("전시 제목", text: translated(row.id, "title"))
                        TextField("짧은 제목", text: translated(row.id, "shortTitle"))
                        TextField("작가", text: translated(row.id, "artist"))
                        TextField("소개", text: translated(row.id, "introduction"), axis: .vertical).lineLimit(2...5)
                        Button("이 언어 입력 삭제") { model.coverEntry.translations.removeAll(where: { $0.id == row.id }) }
                        Divider()
                    }
                }
                Button("언어 추가") { model.coverEntry.translations.append(.init()) }.disabled(model.coverEntry.translations.count >= 16)
            }
        }
    }
    private func effectTitle(_ effect: String) -> String {
        ["none": "없음", "fold": "접힌 표지 펼치기", "mask": "포스터 드러내기", "fade": "천천히 나타내기", "card": "초대장 열기", "tear": "입장권 자르기", "press": "누르기", "confirm": "입장 확인", "doors": "문 열기", "panels": "패널 열기"][effect] ?? effect
    }
    private var hasPresetOverrides: Bool {
        for group in ["layout", "effects"] {
            if case .object(let fields)? = model.coverEntry.design?.raw[group], !fields.isEmpty { return true }
        }
        return [.copyRatio, .margin, .titleScale, .strength, .speed].contains { (field: CoverEntryFormBuffer.NumberField) in
            !(model.coverEntry.numbers[field] ?? "").isEmpty
        }
    }
    var body: some View {
        Section("표지·입장") {
            Picker("테마·기본 배치·연출 적용", selection: Binding(get: { model.coverEntry.design?.theme.rawValue ?? "" }, set: { value in
                if let theme = CoverEntryTheme(rawValue: value) { model.coverEntry.selectTheme(theme) }
            })) {
                if model.coverEntry.design == nil { Text("기존 시작 화면 유지").tag("") }
                ForEach(CoverEntryTheme.allCases, id: \.self) { Text($0.title).tag($0.rawValue) }
            }
            Text("테마를 고르면 배치와 연출이 해당 테마의 기본값으로 바뀝니다. 글·이미지·입장권·언어별 글은 유지됩니다.").foregroundStyle(.secondary)
            if hasPresetOverrides {
                Text("사용자 설정 적용 중").foregroundStyle(.secondary)
                Button("배치·연출을 이 테마 기본값으로") { model.coverEntry.resetLayoutAndEffects() }
            }
            TextField("작가", text: $model.entrance.artist)
            TextField("전시 소개", text: $model.entrance.introduction, axis: .vertical).lineLimit(3...8)
            Picker("표지 이미지", selection: Binding<String?>(get: { model.entrance.imageAssetId?.lowercased() }, set: { model.entrance.imageAssetId = $0 })) {
                Text("제목과 소개만 표시").tag(nil as String?)
                if let draft = model.draft {
                    ForEach(draft.viewerEntranceImageChoices()) { Text($0.title).tag(Optional($0.id.lowercased())) }
                }
            }
            if model.entrance.imageAssetId != nil {
                TextField("이미지 설명", text: $model.entrance.imageAlt, axis: .vertical).lineLimit(2...4)
                Picker("이미지 맞춤", selection: choice("image", "fit")) {
                    Text("테마 기본값").tag(""); Text("전체 보이기").tag("contain"); Text("영역 채우기").tag("cover")
                }
                focus
                Picker("기존 모바일 이미지 위치", selection: $model.entrance.mobilePosition) {
                    Text("가운데").tag(ViewerEntranceSettings.MobilePosition.center)
                    Text("위쪽").tag(ViewerEntranceSettings.MobilePosition.top)
                    Text("아래쪽").tag(ViewerEntranceSettings.MobilePosition.bottom)
                }
                Text("이미지 원본은 바뀌지 않습니다.").foregroundStyle(.secondary)
            }
            Picker("색 구성", selection: choice(nil, "palette")) {
                Text("테마 기본값").tag(""); Text("종이").tag("paper"); Text("밝게").tag("light"); Text("어둡게").tag("dark")
            }
            Picker("글꼴", selection: choice(nil, "type")) {
                Text("테마 기본값").tag(""); Text("시스템").tag("system"); Text("세리프").tag("serif")
            }
            TextField("입장권 제목", text: $model.entrance.ticketTitle)
            TextField("입장권 안내", text: $model.entrance.ticketDetail, axis: .vertical).lineLimit(2...4)
            toggleChoice("입장 날짜", "ticket", "showDate")
            toggleChoice("입장 시간", "ticket", "showTime")
            Text("관람자 기기의 현지 날짜와 분 단위 시간을 표시합니다.").foregroundStyle(.secondary)
            toggleChoice("애니메이션", "effects", "enabled")
            DisclosureGroup("기존 연출 스위치") {
                Toggle("입장권 움직임", isOn: $model.entrance.ticketAnimation)
                Toggle("표지 펼치기", isOn: $model.entrance.coverUnfold)
                Toggle("문 열기 연출", isOn: $model.entrance.doorEffect)
            }
            if mode == .expert { advanced }
            if model.coverEntry.isDirty, (try? model.coverEntry.value()) == nil {
                Text("표지 설정을 적용할 수 없습니다. 숫자 범위·초점 두 좌표·언어 코드의 중복을 확인하세요.").foregroundStyle(.secondary)
            }
            if !model.entranceEditable { Text("지원하지 않는 시작 화면 설정을 보관하고 있습니다. 원래 설정은 변경하지 않습니다.").foregroundStyle(.secondary) }
        }.disabled(!model.entranceEditable)
    }
}

struct CoverEntryPreviewControls: View {
    @ObservedObject var model: ExhibitionModel
    @Environment(\.accessibilityReduceMotion) private var reducedMotion
    let ready: Bool
    let mode: Mode
    var body: some View {
        GroupBox("표지·입장 미리보기") {
            VStack(alignment: .leading, spacing: 12) {
                Picker("화면", selection: $model.entrancePreviewDevice) {
                    Text("데스크톱").tag(EntrancePreviewDevice.desktop)
                    Text("모바일").tag(EntrancePreviewDevice.mobile)
                }.disabled(model.entrancePreviewBusy || model.entrancePreviewActive)
                if mode == .expert {
                    Picker("상황 확인", selection: $model.entrancePreviewScenario) {
                        Text("일반").tag(EntrancePreviewScenario.normal)
                        Text("느린 로딩").tag(EntrancePreviewScenario.slow)
                        Text("이미지 실패").tag(EntrancePreviewScenario.imageFailed)
                    }.disabled(model.entrancePreviewBusy || model.entrancePreviewActive)
                    Toggle("동작 줄이기 확인", isOn: $model.entrancePreviewReducedMotion).disabled(model.entrancePreviewBusy || model.entrancePreviewActive)
                    Picker("미리보기 언어", selection: $model.entrancePreviewLocale) {
                        Text("관람 환경에 맞추기").tag("")
                        ForEach(Array(Set(model.coverEntry.translations.compactMap { $0.fields["locale"].string }.filter { !$0.isEmpty })).sorted(), id: \.self) { Text($0).tag($0) }
                    }.disabled(model.entrancePreviewBusy || model.entrancePreviewActive)
                    Text("상황 확인은 미리보기에만 적용합니다. 관람객의 동작 줄이기 설정이 우선합니다.").foregroundStyle(.secondary)
                }
                ViewThatFits(in: .horizontal) {
                    HStack { previewButtons }
                    VStack(alignment: .leading) { previewButtons }
                }
                if model.entrancePreviewActive {
                    Button("편집 복귀 · 준비 취소") { model.closeEntrancePreview() }
                        .keyboardShortcut(.cancelAction)
                        .help("진행 중인 준비를 취소하고 같은 입력·선택·실행 취소 이력으로 돌아갑니다.")
                }
                if model.entrancePreviewBusy { ProgressView().controlSize(.small) }
                if !model.viewportController.entrancePreviewSupported {
                    Text("현재 연결은 표지 미리보기를 지원하지 않습니다. 저장된 설정과 편집 입력은 유지됩니다.").foregroundStyle(.secondary)
                }
                if !model.entrancePreviewStatus.isEmpty { Text(model.entrancePreviewStatus).foregroundStyle(.secondary) }
                if !model.entranceThumbnails.isEmpty {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 160), alignment: .leading)], alignment: .leading) {
                        ForEach(CoverEntryTheme.allCases, id: \.self) { theme in
                            if let data = model.entranceThumbnails[theme], let image = NSImage(data: data) {
                                Button { model.coverEntry.selectTheme(theme) } label: {
                                    VStack(alignment: .leading) {
                                        Image(nsImage: image).resizable().scaledToFit().frame(height: 120).accessibilityHidden(true)
                                        Text(theme.title)
                                    }
                                }.disabled(model.entrancePreviewActive || model.busy || model.pending || model.immutable)
                                    .help("이 테마와 기본 배치·연출을 적용합니다. 글·이미지·입장권은 유지됩니다.")
                            }
                        }
                    }
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        .onAppear { model.entranceSystemReducedMotion = reducedMotion }
        .onChange(of: reducedMotion) { _, value in model.entranceSystemReducedMotion = value }
    }
    @ViewBuilder private var previewButtons: some View {
        Button("실제 테마 썸네일 불러오기") { model.previewEntrance(thumbnails: true, systemReducedMotion: reducedMotion) }
            .disabled(!available || model.entrancePreviewActive)
        Button(model.entrancePreviewActive ? "처음부터 재생" : "표지 미리보기") {
            model.previewEntrance(model.entrancePreviewActive ? .replay : .show, systemReducedMotion: reducedMotion)
        }.disabled(!available)
        Button("걸어보기") { model.previewEntrance(.walk, systemReducedMotion: reducedMotion) }.disabled(!available)
    }
    private var available: Bool {
        ready && model.entranceEditable && model.viewportController.connected && model.viewportController.entrancePreviewSupported && !model.entrancePreviewBusy && !model.busy && !model.pending
    }
}
