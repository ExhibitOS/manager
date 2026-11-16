// SPDX-License-Identifier: Apache-2.0
import XCTest
import ManagerCore
@testable import ManagerApp

@MainActor
final class PreparedViewerBudgetExportTests: XCTestCase {
    func testOrdinaryExportInspectsItsPublicationAndContinuesWithWarningsOrUnavailableResult() async throws {
        let publicationId = "44444444-4444-4444-8444-444444444444"
        let revision = String(repeating: "a", count: 64)
        let output = URL(fileURLWithPath: "/private/tmp/viewer-budget-export-fixture")
        let wire: [String: String] = ["publicationId": publicationId, "revisionSha256": revision,
            "publicUrl": "/p/" + publicationId, "publishedAt": "2026-11-16T14:00:00Z", "status": "published"]
        let publication = try JSONDecoder().decode(NativePublication.self, from: JSONEncoder().encode(wire))
        let terminal = try JSONDecoder().decode(NativeStaticExportStatus.self, from: JSONSerialization.data(withJSONObject: [
            "format": 1, "jobId": publicationId, "workspaceId": publicationId, "tenantId": publicationId,
            "principalId": publicationId, "publicationId": publicationId, "revisionSha256": revision,
            "output": output.path, "state": "completed",
        ]))
        let warning: DocumentValue = .object(["estimates": .object(["wholeExhibition":
            .object(["evaluation": .object(["status": .string("admission-exceeded")])])])])
        let model = StaticExportModel()
        var events: [String] = []
        await model.connect(read: { nil }, start: { id, destination, privateEditor in
            XCTAssertEqual(id, publicationId)
            XCTAssertEqual(destination, output)
            events.append(privateEditor ? "private" : "export")
            return terminal
        }, cancel: { nil })
        await model.export(publication, to: output, privateEditor: false, isCurrent: { true }, inspectBudget: {
            events.append("inspect")
            return warning
        })
        XCTAssertEqual(events, ["inspect", "export"])
        XCTAssertEqual(model.budgetInspection(for: publication).report, warning)

        await model.export(publication, to: output, privateEditor: false, isCurrent: { true }, inspectBudget: {
            events.append("mismatch")
            throw AuthoringFailure("VIEWER_BUDGET_ANALYSIS_STALE")
        })
        XCTAssertEqual(events, ["inspect", "export", "mismatch", "export"])
        XCTAssertTrue(model.budgetInspection(for: publication).attempted)
        XCTAssertNil(model.budgetInspection(for: publication).report)
        let changed = try JSONDecoder().decode(NativePublication.self,
            from: JSONEncoder().encode(wire.merging(["revisionSha256": String(repeating: "b", count: 64)]) { _, new in new }))
        XCTAssertFalse(model.budgetInspection(for: changed).attempted)

        await model.export(publication, to: output, privateEditor: true, isCurrent: { true }, inspectBudget: { XCTFail("Private export"); return .null })
        XCTAssertEqual(events.last, "private")

        var current = true
        await model.export(publication, to: output, privateEditor: false, isCurrent: { current }, inspectBudget: {
            events.append("source-changed")
            current = false
            throw AuthoringFailure("VIEWER_BUDGET_ANALYSIS_STALE")
        })
        XCTAssertEqual(events.last, "source-changed")
        XCTAssertEqual(model.failure?.code, "NATIVE_PUBLICATION_STATE_CHANGED")
        XCTAssertFalse(model.budgetInspection(for: publication).attempted)

        await model.export(publication, to: output, privateEditor: false, isCurrent: { true }, inspectBudget: {
            events.append("interrupted")
            let stopped = await model.cancelAndWait()
            XCTAssertTrue(stopped)
            return warning
        })
        XCTAssertEqual(events.last, "interrupted")
        XCTAssertFalse(model.budgetInspection(for: publication).attempted)
    }
}
