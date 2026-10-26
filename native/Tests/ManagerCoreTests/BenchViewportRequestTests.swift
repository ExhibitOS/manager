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
