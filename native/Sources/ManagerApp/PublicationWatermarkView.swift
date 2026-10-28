// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

struct NativePublicationWatermarkView: View {
    @ObservedObject var source: ExhibitionModel
    let mode: Mode
    let ready: Bool
    let preview: () -> Void
    @State private var details = false
    private var fields: PublicationWatermarkFormBuffer { source.publicationWatermark }
    private var editable: Bool { ready && source.canEditPublicationWatermark }
    var body: some View {
        GroupBox("워터마크") {
            VStack(alignment: .leading, spacing: 10) {
                if !source.publicationWatermarkReadable {
                    Label(source.publicationWatermarkStatus, systemImage: "exclamationmark.triangle")
                } else if !source.publicationWatermarkRuntimeAvailable {
                    Text("현재 실행본에서는 워터마크 선택을 지원하지 않습니다. 지원 실행본이 준비되면 설정할 수 있습니다.")
                    Text("기존 전시와 공개본의 표시 방식은 그대로 보존합니다.").foregroundStyle(.secondary)
                } else {
                    Picker("표시 방식", selection: $source.publicationWatermark.mode) {
                        Text("없음").tag(PublicationWatermark.Mode.none)
                        Text("화면에 표시").tag(PublicationWatermark.Mode.overlay)
                        Text("배포 사본에 삽입").tag(PublicationWatermark.Mode.embedded)
                    }.pickerStyle(.menu).disabled(!editable)
                    if fields.mode == .none {
                        Text("워터마크 없는 표시용 사본을 사용합니다.").foregroundStyle(.secondary)
                    } else {
                        TextField(fields.mode == .overlay ? "화면에 표시할 문구" : "사본에 넣을 문구", text: $source.publicationWatermark.text,
                                  prompt: Text(fields.mode == .overlay ? "비우면 전시 제목과 작가" : "비우면 EXHIBITOS"))
                            .textFieldStyle(.roundedBorder).disabled(!editable)
                        if fields.mode == .overlay {
                            Text("화면 위에만 표시하며 작품 파일은 바꾸지 않습니다. 문구는 160자까지 사용할 수 있습니다.").foregroundStyle(.secondary)
                        } else {
                            Text("그림의 배포 사본에만 문구를 넣고 영문은 작은 대문자로 표시합니다. 조각 파일은 워터마크 없는 사본을 사용합니다. 원본은 바꾸지 않습니다.").foregroundStyle(.secondary)
                            Text("영문·숫자와 지원 기호 80자까지: . , : ; ! ? \" ' _ / @ ( ) + -").font(.callout).foregroundStyle(.secondary)
                        }
                        DisclosureGroup("위치·크기·진하기", isExpanded: $details) {
                            VStack(alignment: .leading, spacing: 10) {
                                Picker("위치", selection: $source.publicationWatermark.position) {
                                    Text("왼쪽 위").tag(PublicationWatermark.Position.topLeft)
                                    Text("오른쪽 위").tag(PublicationWatermark.Position.topRight)
                                    Text("왼쪽 아래").tag(PublicationWatermark.Position.bottomLeft)
                                    Text("오른쪽 아래").tag(PublicationWatermark.Position.bottomRight)
                                }.pickerStyle(.menu)
                                HStack {
                                    TextField("문자 크기 (8–24 px)", text: $source.publicationWatermark.size)
                                    TextField("불투명도 (0.1–0.6)", text: $source.publicationWatermark.opacity)
                                }.textFieldStyle(.roundedBorder)
                            }.padding(.top, 6).disabled(!editable)
                        }
                    }
                    if fields.isDirty && !source.publicationWatermarkValid {
                        Label("입력을 확인하세요. 저장하기 전까지 입력은 이 화면에 유지됩니다.", systemImage: "exclamationmark.triangle")
                    }
                    if fields.isDirty {
                        HStack {
                            Button("이 Mac에 보관") { Task { await source.keepLocal() } }
                                .disabled(!editable || !source.publicationWatermarkValid)
                            Button("전시 저장") { Task { await source.save() } }
                                .disabled(!editable || !source.publicationWatermarkValid)
                            Button("워터마크 입력 취소") { source.discardPublicationWatermark() }
                                .disabled(source.busy || source.pending)
                        }
                    }
                    Text("설정을 바꾼 전시를 저장한 뒤 새 공개용 저장본을 준비하세요. 기존 공개본은 바뀌지 않습니다.").foregroundStyle(.secondary)
                    if fields.mode != .embedded {
                        Button("실제 전시 미리보기", systemImage: "eye", action: preview)
                            .disabled(!ready || source.dirty || source.busy || source.pending)
                    } else {
                        Text("삽입된 문구는 아래에서 새 공개용 저장본과 정적 출력을 만든 뒤 실제 사본에서 확인하세요.").foregroundStyle(.secondary)
                    }
                }
                Text("원본 다운로드 허용과 작품의 권리·필수 출처 표시는 별도 설정입니다.").font(.callout).foregroundStyle(.secondary)
            }.frame(maxWidth: .infinity, alignment: .leading).padding(6)
        }
        .onAppear { details = mode == .expert }
        .onChange(of: mode) { _, value in details = value == .expert }
    }
}
