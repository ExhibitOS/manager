// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class BenchViewportRequestTests: XCTestCase {
    func fixture() throws -> BenchViewportRequest {
        let draft = try NativeExhibitionDraft.new(), fields = try BenchFormBuffer(roomId: XCTUnwrap(draft.rooms.first?["id"].string))
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 8, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
        return try BenchViewportRequest(frame: frame, command: .add(fields.settings(), viewpoint: fields.newViewpoint()), selectedBenchId: fields.id)
    }
    func reply(_ request: BenchViewportRequest) throws -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { fields[key] = request.value[key] }
        fields["benchId"] = .string(request.command.benchId)
        fields["candidate"] = try request.command.proposedCandidate(in: request.frame.draft, selectedBenchId: request.command.benchId)
        return .object(fields)
    }
    func testOptionalTopLevelCommandUsesClosedBindingAndOldFrameRemainsClosed() throws {
        let request = try fixture()
        guard case .object(let fields) = request.value else { return XCTFail("Expected object") }
        XCTAssertEqual(Set(fields.keys), Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "type", "bench", "viewpoint"]))
        XCTAssertEqual(request.value["type"], .string("add-bench"))
        XCTAssertEqual(request.value["bench"]["viewpointId"], request.value["viewpoint"]["id"])
        XCTAssertEqual(try request.checkedReply(reply(request), selectedBenchId: request.command.benchId).base, request.frame.draft)
        let old = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true}".utf8))
        XCTAssertFalse(old.supportsBenchInspector)
        let current = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"bench-inspector-v1\"]}".utf8))
        XCTAssertTrue(current.supportsBenchInspector)
        XCTAssertFalse(ViewportSelection(kind: "bench", id: request.command.benchId).belongs(to: request.frame.draft))
    }
    func testLinkedHeightUsesClosedEnvelopeAndSeparateOrdinaryCapability() throws {
        let initial = try fixture()
        var base = initial.frame.draft
        try base.replaceCandidate(initial.command.proposedCandidate(in: base, selectedBenchId: initial.command.benchId))
        let view = try XCTUnwrap(BenchSettings.list(in: base).first?.viewpointId)
        let f = initial.frame
        let frame = ViewportFrame(epoch: f.epoch, sequence: f.sequence + 1, tenantId: f.tenantId,
                                  principalId: f.principalId, draft: base, selection: nil,
                                  mode: f.mode, view: f.view, disabled: false)
        let benchId = initial.command.benchId
        let baseline = try BenchFormBuffer(draft: base, benchId: benchId)
        var form = baseline
        form.linkedTargetY = "1.30"
        let command = try XCTUnwrap(form.linkedTargetHeightCommand(comparedTo: baseline))
        XCTAssertEqual(command, .setViewpointTargetHeight(benchId: benchId, viewpointId: view, targetY: 1.3))
        let request = try BenchViewportRequest(frame: frame, command: command, selectedBenchId: command.benchId)
        guard case .object(let fields) = request.value else { return XCTFail("Expected object") }
        XCTAssertEqual(Set(fields.keys), Set(["version", "epoch", "sequence", "tenantId", "principalId",
                                              "requestId", "type", "benchId", "viewpointId", "targetY"]))
        XCTAssertEqual(request.value["type"], .string("set-bench-viewpoint-target-height"))
        var response = try reply(request)
        let preview = try request.checkedReply(response, selectedBenchId: command.benchId)
        response["sequence"] = initial.value["sequence"]
        XCTAssertThrowsError(try request.checkedReply(response, selectedBenchId: command.benchId))

        let decoder = JSONDecoder()
        let previous = try decoder.decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"capabilities":["bench-inspector-v1"]}"#.utf8))
        XCTAssertFalse(previous.supportsBenchViewpointTargetHeight)
        let ordinary = try decoder.decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"boundedReadOnly":false,"capabilities":["bench-inspector-v1","bench-viewpoint-target-height-v1"]}"#.utf8))
        XCTAssertTrue(ordinary.supportsBenchViewpointTargetHeight)
        let readOnly = try decoder.decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"boundedReadOnly":true,"capabilities":["bench-inspector-v1","bench-viewpoint-target-height-v1"]}"#.utf8))
        XCTAssertFalse(readOnly.supportsBenchViewpointTargetHeight)

        // Displaying an authenticated preview disables new requests. Applying it
        // locally still uses the active form and the original draft.
        let disabled = ViewportFrame(epoch: frame.epoch, sequence: frame.sequence + 1, tenantId: frame.tenantId,
                                     principalId: frame.principalId, draft: preview.displayDraft, selection: nil,
                                     mode: frame.mode, view: frame.view, disabled: true)
        let disabledAck = try decoder.decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"boundedReadOnly":false,"capabilities":["bench-inspector-v1"]}"#.utf8))
        XCTAssertTrue(disabledAck.supportsBenchInspector)
        XCTAssertFalse(disabledAck.supportsBenchViewpointTargetHeight)
        XCTAssertThrowsError(try BenchViewportRequest(frame: disabled, command: command, selectedBenchId: benchId))
        let applyCommand = try XCTUnwrap(form.linkedTargetHeightCommand(comparedTo: baseline))
        XCTAssertEqual(try preview.apply(to: base, selectedBenchId: benchId, command: applyCommand).candidate, preview.candidate)
        XCTAssertThrowsError(try preview.apply(to: disabled.draft, selectedBenchId: benchId, command: applyCommand))
    }
    func testSeatPreviewBindsOnlyTheTransientBenchAndClosesToTheSameEditorDraft() throws {
        let initial = try fixture(), f = initial.frame, benchId = initial.command.benchId
        var base = f.draft
        try base.replaceCandidate(initial.command.proposedCandidate(in: base, selectedBenchId: benchId))
        let viewpointId = try XCTUnwrap(BenchSettings.list(in: base).first?.viewpointId)
        let command = BenchCommand.setViewpointTargetHeight(benchId: benchId, viewpointId: viewpointId, targetY: 1.30)
        let preview = try BenchPreview(candidate: command.proposedCandidate(in: base, selectedBenchId: benchId),
                                       base: base, command: command, selectedBenchId: benchId)
        let seat = try ViewportBenchSeatTarget(preview: preview, draft: base, selectedBenchId: benchId, command: command)
        XCTAssertThrowsError(try ViewportBenchSeatTarget(preview: preview, draft: base, selectedBenchId: viewpointId, command: command))
        XCTAssertThrowsError(try ViewportBenchSeatTarget(preview: preview, draft: preview.displayDraft, selectedBenchId: benchId, command: command))
        let wrongLink = BenchCommand.setViewpointTargetHeight(benchId: benchId, viewpointId: benchId, targetY: 1.30)
        XCTAssertThrowsError(try ViewportBenchSeatTarget(preview: preview, draft: base, selectedBenchId: benchId, command: wrongLink))

        func frame(_ draft: NativeExhibitionDraft) -> ViewportFrame {
            ViewportFrame(epoch: f.epoch, sequence: f.sequence + 1, tenantId: f.tenantId, principalId: f.principalId,
                          draft: draft, selection: nil, mode: f.mode, view: f.view, disabled: true)
        }
        func request(_ action: EntrancePreviewAction, frame: ViewportFrame, seat: ViewportBenchSeatTarget? = nil) throws -> ViewportEntrancePreviewRequest {
            try ViewportEntrancePreviewRequest(frame: frame, action: action, device: .desktop,
                                               scenario: .normal, reducedMotion: true, seat: seat)
        }
        func response(_ request: ViewportEntrancePreviewRequest, mode: String) -> DocumentValue {
            var reply: DocumentValue = .object(["mode": .string(mode), "status": .string("ready"),
                "baseDraft": request.frame.draft.document, "renderGeneration": .integer(1), "imageStatus": .string("ready"),
                "viewport": .object(["width": .integer(1024), "height": .integer(768)]),
                "snapshotRect": .object(["x": .integer(0), "y": .integer(0), "width": .integer(1024), "height": .integer(768)])])
            for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "theme", "locale",
                        "device", "scenario", "reducedMotion"] { reply[key] = request.value[key] }
            if request.action == .seat {
                for key in ["benchId", "viewpointId"] { reply[key] = request.value[key] }
            }
            return reply
        }
        let display = frame(preview.displayDraft)
        XCTAssertThrowsError(try request(.seat, frame: frame(base), seat: seat))
        XCTAssertThrowsError(try request(.seat, frame: display))
        XCTAssertThrowsError(try request(.close, frame: display, seat: seat))
        let start = try request(.seat, frame: display, seat: seat)
        XCTAssertEqual(start.value["benchId"], .string(benchId))
        XCTAssertEqual(start.value["viewpointId"], .string(viewpointId))
        XCTAssertNoThrow(try start.checkedReply(response(start, mode: "seated")))
        XCTAssertThrowsError(try start.checkedReply(response(start, mode: "walk")))
        for key in ["sequence", "benchId", "viewpointId", "baseDraft"] {
            var wrong = response(start, mode: "seated")
            wrong[key] = .string("stale")
            XCTAssertThrowsError(try start.checkedReply(wrong))
        }
        let close = try ViewportEntrancePreviewRequest(frame: display, action: .close, device: .desktop, scenario: .normal,
                                                       reducedMotion: true, seatReturn: true)
        var compact = response(close, mode: "editor")
        compact["viewport"]["width"] = .integer(650)
        compact["snapshotRect"]["width"] = .integer(650)
        XCTAssertNoThrow(try close.checkedReply(compact))
        XCTAssertEqual(close.value["benchId"], .null)
        XCTAssertEqual(close.value["viewpointId"], .null)
        XCTAssertNoThrow(try close.checkedReply(response(close, mode: "editor")))
        XCTAssertThrowsError(try close.checkedReply(response(start, mode: "seated")))
        XCTAssertEqual(base, preview.base)
        XCTAssertNotEqual(base, close.frame.draft)

        let decoder = JSONDecoder()
        let old = try decoder.decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"boundedReadOnly":false,"capabilities":["entrance-preview-v1"]}"#.utf8))
        XCTAssertFalse(old.supportsEntranceSeatPreview)
        let capable = try decoder.decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"boundedReadOnly":false,"capabilities":["entrance-preview-v1","entrance-seat-preview-v1"]}"#.utf8))
        XCTAssertTrue(capable.supportsEntranceSeatPreview)
        let readOnly = try decoder.decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"boundedReadOnly":true,"capabilities":["entrance-preview-v1","entrance-seat-preview-v1"]}"#.utf8))
        XCTAssertFalse(readOnly.supportsEntranceSeatPreview)
    }
    func testWrongScopeLateReplyExtraFieldsAndUnrelatedCandidateAreRefused() throws {
        let request = try fixture(), good = try reply(request)
        for key in ["version", "sequence", "epoch", "tenantId", "principalId", "requestId", "benchId"] {
            var changed = good; changed[key] = .string("stale")
            XCTAssertThrowsError(try request.checkedReply(changed, selectedBenchId: request.command.benchId))
        }
        var changed = good; changed["accepted"] = .bool(true)
        XCTAssertThrowsError(try request.checkedReply(changed, selectedBenchId: request.command.benchId))
        changed = good; changed["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try request.checkedReply(changed, selectedBenchId: request.command.benchId))
        XCTAssertThrowsError(try request.checkedReply(good, selectedBenchId: nil))
        let f = request.frame
        let disabled = ViewportFrame(epoch: f.epoch, sequence: f.sequence, tenantId: f.tenantId, principalId: f.principalId, draft: f.draft, selection: nil, mode: "translate", view: "isometric", disabled: true)
        XCTAssertThrowsError(try BenchViewportRequest(frame: disabled, command: request.command, selectedBenchId: request.command.benchId))
    }
}
