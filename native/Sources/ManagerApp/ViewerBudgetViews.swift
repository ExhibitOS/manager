// SPDX-License-Identifier: Apache-2.0
import SwiftUI
import ManagerCore

private enum BudgetLabels {
    static func metric(_ key: String) -> String {
        switch key {
        case "colorDraws": "화면 그리기 수"
        case "displayTriangles": "표시 삼각형 수"
        case "shadowCasterTriangles": "그림자 형상 삼각형 수"
        case "shadowDepthTriangles": "그림자 깊이 작업 삼각형 수"
        case "residentBytes": "상주 메모리 추정 · 바이트"
        case "transitionBytes": "전환 메모리 추정 · 바이트"
        default: key
        }
    }
    static func status(_ value: String?) -> String {
        switch value {
        case "within": "정상"
        case "warning": "주의"
        case "admission-exceeded": "한도 초과"
        default: "알 수 없음"
        }
    }
    static func color(_ value: String?) -> Color {
        switch value {
        case "within": .primary
        case "warning": .orange
        case "admission-exceeded": .red
        default: .secondary
        }
    }
    static func number(_ value: DocumentValue) -> String {
        value.integer.map { $0.formatted() } ?? "알 수 없음"
    }
}

/// Presents authoritative shared decisions. It never infers status from totals
/// or equates mounted scene observations with target Viewer analysis.
struct ViewerBudgetEvaluationView: View {
    let title: String
    let evaluation: DocumentValue?
    var calculating = false
    @State private var expanded = false
    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            if let evaluation, let metrics = evaluation["metrics"].items {
                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 6) {
                    GridRow { Text("항목"); Text("추정"); Text("경고"); Text("한도"); Text("상태") }
                        .foregroundStyle(.secondary)
                    ForEach(metrics.indices, id: \.self) { index in
                        let row = metrics[index]
                        GridRow {
                            Text(BudgetLabels.metric(row["metric"].string ?? ""))
                            Text(BudgetLabels.number(row["value"]))
                            Text(BudgetLabels.number(row["warning"]))
                            Text(BudgetLabels.number(row["admission"]))
                            Text(BudgetLabels.status(row["status"].string)).foregroundStyle(BudgetLabels.color(row["status"].string))
                        }
                    }
                }.font(.caption).monospacedDigit().textSelection(.enabled)
                Text(evaluation["complete"] == .bool(true) ? "표시된 분석 범위의 추정입니다." : "아직 분석하지 못한 항목이 있습니다. 전체 전시의 실행 가능 여부를 확정하지 않습니다.")
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                Text("분석 결과를 아직 확인할 수 없습니다.").font(.caption).foregroundStyle(.secondary)
            }
            Text("메모리는 모델 기반 추정이며 실제 GPU 메모리 측정값이 아닙니다. 경고 중에도 편집과 저장을 계속할 수 있습니다.")
                .font(.caption).foregroundStyle(.secondary)
        } label: {
            HStack(spacing: 8) {
                if calculating { ProgressView().controlSize(.small) }
                Text(title)
                Text(calculating ? "계산 중" : BudgetLabels.status(evaluation?["status"].string))
                    .foregroundStyle(BudgetLabels.color(evaluation?["status"].string))
            }
        }
    }
}

/// Uses the existing producer report verbatim for values and evaluations. It
/// adds no Native cost calculation or export admission decision.
private struct ViewerBudgetAnalysisDetailsView: View {
    let analysis: DocumentValue
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            let contributors = analysis["estimates"]["wholeExhibition"]["usage"]["contributors"].items ?? []
            DisclosureGroup("항목별 기여") {
                if contributors.isEmpty {
                    Text("항목별 기여 정보가 없습니다.").foregroundStyle(.secondary)
                }
                ForEach(contributors.indices, id: \.self) { index in
                    let item = contributors[index]
                    DisclosureGroup(item["label"].string ?? "항목") {
                        ForEach(ViewerBudgetSettings.metrics, id: \.self) { metric in
                            if item["usage"][metric] != .null {
                                LabeledContent(BudgetLabels.metric(metric), value: BudgetLabels.number(item["usage"][metric]))
                            }
                        }
                    }
                }
                Text("기여도를 알 수 있는 값만 표시합니다.").foregroundStyle(.secondary)
            }
            let sections = analysis["estimates"]["analyzedSections"].items ?? []
            DisclosureGroup("분석한 구간") {
                if sections.isEmpty { Text("구간별 분석 정보가 없습니다.").foregroundStyle(.secondary) }
                ForEach(sections.indices, id: \.self) { index in
                    ViewerBudgetEvaluationView(title: "분석 구간 \(index + 1)", evaluation: sections[index]["evaluation"])
                }
                Text("구간별 범위는 겹칠 수 있습니다. 전시 전체 추정과 함께 확인하세요.").foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("조정 방법").fontWeight(.medium)
                if analysis["analysisComplete"] != .bool(true) {
                    Text("분석하지 못한 항목은 제작 단계의 리소스 분석을 확인하세요.")
                }
                Text("현재 화면과 무거운 구간을 따로 살펴보세요.")
                Text("이 전시의 예산을 조정하거나 권장값으로 초기화할 수 있습니다.")
            }
            Text("전시 전체는 숨겨진 오브젝트를 포함한 선택 시나리오의 추정입니다. 메모리는 모든 선택 에셋을 함께 사용하는 경우를 가정합니다. 실제 화면 점유량·다운로드량·GPU 시간의 측정값은 각 관측 범위에서 따로 확인하세요.")
                .foregroundStyle(.secondary)
            if analysis["exportConfiguration"]["performanceAdvisoryOnly"] == .bool(true) {
                Text("성능 예산 경고 중에도 저장과 내보내기를 계속할 수 있습니다. 작품 권리와 파일 형식 등 공개 준비 항목은 별도로 확인합니다.")
                    .foregroundStyle(.secondary)
            }
        }.font(.caption).textSelection(.enabled)
    }
}

struct ViewerBudgetSettingsView: View {
    @Binding var form: ViewerBudgetSettings
    let policy: DocumentValue?
    let busy: Bool
    let apply: () -> Void
    let resetProfile: () -> Void
    let resetAll: () -> Void
    var canEditStoredPolicy = true
    private func field(_ metric: String, warning: Bool) -> Binding<String> {
        Binding(get: { warning ? form.warning[metric] ?? "" : form.admission[metric] ?? "" }, set: { value in
            if warning { form.warning[metric] = value } else { form.admission[metric] = value }
        })
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("목표 Viewer", selection: $form.targetProfile) {
                Text("기기 자동 선택").tag("auto")
                Text("표준").tag("standard")
                Text("간소").tag("compact")
            }
            .disabled(!canEditStoredPolicy)
            Text("전시에 저장되어 내보내기와 Viewer에 함께 적용됩니다. Native 편집 화면의 분석 프로필은 별도입니다.")
                .font(.caption).foregroundStyle(.secondary)
            Text("설정 프로필: \(form.selectedProfile)")
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 8) {
                GridRow { Text("항목"); Text("권장 한도"); Text("경고 재정의"); Text("한도 재정의") }.font(.caption).foregroundStyle(.secondary)
                ForEach(ViewerBudgetSettings.metrics, id: \.self) { metric in
                    GridRow {
                        Text(BudgetLabels.metric(metric))
                        Text(BudgetLabels.number(policy?["recommended"][metric] ?? .null)).monospacedDigit()
                        TextField("권장값 사용", text: field(metric, warning: true)).accessibilityLabel("\(BudgetLabels.metric(metric)) 경고 재정의")
                        TextField("권장값 사용", text: field(metric, warning: false)).accessibilityLabel("\(BudgetLabels.metric(metric)) 한도 재정의")
                    }
                }
            }
            .disabled(!canEditStoredPolicy)
            Text("빈칸은 공통 정책의 권장값을 사용합니다. 한도를 높여도 기기 성능이나 보안·형식의 고정 제한은 바뀌지 않습니다. 원본이나 품질을 자동으로 줄이지 않습니다.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("설정 저장", action: apply).disabled(!canEditStoredPolicy)
                Button("이 프로필 권장값으로 복원", action: resetProfile).disabled(!canEditStoredPolicy)
                Button("전시 예산 설정 전체 초기화", action: resetAll)
            }
            Text("일반 전시 저장으로 설정을 보관합니다.").font(.caption).foregroundStyle(.secondary)
        }.disabled(busy)
    }
}

private struct ViewerBudgetEditorPresentation: Identifiable {
    let id = UUID()
    let source: NativeDocumentState
    let profile: String
}

private struct ViewerBudgetPolicyEditor: View {
    @ObservedObject var model: ExhibitionModel
    let source: NativeDocumentState
    let profile: String
    let canEditStoredPolicy: Bool
    @State private var form: ViewerBudgetSettings?
    @State private var analysis: DocumentValue?
    @State private var saving = false
    @State private var status = ""
    @Environment(\.dismiss) private var dismiss

    init(model: ExhibitionModel, source: NativeDocumentState, profile: String) {
        self.model = model; self.source = source; self.profile = profile
        let parsed = source.draft.flatMap { try? ViewerBudgetSettings(candidate: $0.candidate, selectedProfile: profile) }
        canEditStoredPolicy = parsed != nil
        _form = State(initialValue: parsed)
    }

    private func save(resetProfile: Bool = false, resetAll: Bool = false) {
        do {
            let command: DocumentValue
            if resetAll { command = ViewerBudgetSettings.resetCommand }
            else {
                guard let form else { return }
                command = try form.command(resetSelectedProfile: resetProfile)
            }
            saving = true; status = ""
            Task {
                defer { saving = false }
                if await model.saveViewerBudgetPolicy(command, expected: source) { dismiss() }
                else { status = "설정을 저장하지 못했습니다. 입력을 유지했습니다. 전시 상태를 확인한 뒤 다시 시도하세요." }
            }
        } catch { status = "경고에는 0 이상의 정수, 한도에는 1 이상의 정수를 입력하세요. 빈칸은 권장값을 사용합니다." }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("전시 성능 예산 설정").font(.headline)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    if !canEditStoredPolicy {
                        Text("저장된 예산 설정을 읽을 수 없습니다. 전시의 다른 내용은 유지하고 예산 설정 전체를 초기화할 수 있습니다.")
                    }
                    if let fields = Binding($form) {
                        ViewerBudgetSettingsView(form: fields, policy: analysis?["resolvedPolicy"], busy: saving || model.pending,
                            apply: { save() }, resetProfile: { save(resetProfile: true) }, resetAll: { save(resetAll: true) })
                    } else {
                        Button("전시 예산 설정 전체 초기화") { save(resetAll: true) }.disabled(saving || model.pending)
                    }
                    if !status.isEmpty { Text(status).foregroundStyle(.secondary) }
                    if model.pending {
                        Text("저장 요청은 보존돼 있습니다. 새 요청을 만들지 않고 기존 응답을 다시 확인합니다.").font(.caption)
                        Button("저장 응답 다시 확인") {
                            saving = true
                            Task {
                                defer { saving = false }
                                await model.save()
                                if model.state?.pending == nil && model.state?.dirty == false { dismiss() }
                            }
                        }.disabled(saving || model.busy)
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack {
                if saving { ProgressView().controlSize(.small); Text("설정 저장 중…").font(.caption) }
                Spacer()
                Button("닫기") { dismiss() }.keyboardShortcut(.cancelAction).disabled(saving)
            }
        }
        .padding(20).frame(minWidth: 760, minHeight: 460)
        .interactiveDismissDisabled(saving)
        .task {
            do { analysis = try await model.viewerBudgetSettingsAnalysis(profile: profile, source: source) }
            catch { status = "권장값 분석을 확인하지 못했습니다. 값을 추측하지 않으며 설정 입력과 초기화는 계속할 수 있습니다." }
        }
    }
}

struct NativeEditorBudgetStatusView: View {
    @ObservedObject var controller: NativeViewportController
    private var refreshKey: String {
        "\(controller.acceptedSequence):\(controller.connected):\(controller.editingContextActive):\(controller.resourceUsageSupported):\(String(describing: controller.sceneProgress))"
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ViewerBudgetEvaluationView(title: "Native 편집 화면 추정", evaluation: controller.resourceUsage?["evaluation"], calculating: controller.readingResourceUsage)
            if let observations = controller.resourceUsage?["observations"], observations != .null {
                Text("현재 불러온 장면의 분석입니다. 목표 Viewer의 전체 전시 분석과 구분합니다.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Button("편집 화면 추정 새로 확인") { Task { await controller.refreshResourceUsage() } }
                .disabled(!controller.connected || !controller.resourceUsageSupported || controller.readingResourceUsage)
        }
        .task(id: refreshKey) { await controller.refreshResourceUsage() }
    }
}

struct TargetViewerBudgetStatusView: View {
    @ObservedObject var model: ExhibitionModel
    let mode: Mode
    @State private var inventoryExpanded = false
    @State private var settings: ViewerBudgetEditorPresentation?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ViewerBudgetEvaluationView(title: "목표 Viewer 추정", evaluation: model.viewerBudgetAnalysis?["estimates"]["wholeExhibition"]["evaluation"], calculating: model.readingViewerBudget)
            if let analysis = model.viewerBudgetAnalysis {
                let profile = analysis["selection"]["effectiveProfile"].string
                Text(profile == "compact" ? "간소 프로필 · 전시 전체 범위" : profile == "native" ? "Native 프로필 · 전시 전체 범위" : "표준 프로필 · 전시 전체 범위")
                    .font(.caption).foregroundStyle(.secondary)
                DisclosureGroup("파일 등록 정보", isExpanded: $inventoryExpanded) {
                    LabeledContent("등록된 파일 참조 수", value: BudgetLabels.number(analysis["inventory"]["declaredResourceCount"]))
                    LabeledContent("고유 콘텐츠 크기 · 바이트", value: BudgetLabels.number(analysis["inventory"]["uniqueContentBytes"]))
                    LabeledContent("다운로드 크기 · 바이트", value: BudgetLabels.number(analysis["inventory"]["downloadBytes"]))
                    Text("파일의 등록 정보입니다. 다운로드나 디코딩 후 메모리의 측정값과 구분합니다.").foregroundStyle(.secondary)
                }.font(.caption)
                DisclosureGroup("전체 분석 상세와 조정 방법") {
                    ViewerBudgetAnalysisDetailsView(analysis: analysis)
                }.font(.caption)
            }
            if !model.viewerBudgetStatus.isEmpty { Text(model.viewerBudgetStatus).font(.caption).foregroundStyle(.secondary) }
            Button("목표 Viewer 추정 새로 확인") { Task { await model.refreshViewerBudgetAnalysis(force: true) } }
                .disabled(model.viewerBudgetReadKey == nil || model.readingViewerBudget)
            if mode == .expert {
                Menu("전시 예산 설정…") {
                    ForEach(ViewerBudgetSettings.profiles, id: \.self) { profile in
                        Button(profile == "native" ? "Native 편집 화면" : profile == "compact" ? "간소 Viewer" : "표준 Viewer") {
                            if let source = model.state { settings = ViewerBudgetEditorPresentation(source: source, profile: profile) }
                        }
                    }
                }.disabled(!model.canConfigureViewerBudget)
                if model.dirty { Text("현재 변경을 저장한 뒤 예산 설정을 변경할 수 있습니다.").font(.caption).foregroundStyle(.secondary) }
            }
        }
        .task(id: model.viewerBudgetReadKey) { await model.refreshViewerBudgetAnalysis() }
        .sheet(item: $settings) { value in ViewerBudgetPolicyEditor(model: model, source: value.source, profile: value.profile) }
    }
}
