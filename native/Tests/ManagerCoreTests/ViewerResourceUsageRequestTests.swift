// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewerResourceUsageRequestTests: XCTestCase {
    private func request() throws -> ViewerResourceUsageRequest {
        try ViewerResourceUsageRequest(frame: ViewportFrame(epoch: "11111111-1111-4111-8111-111111111111", sequence: 9,
            tenantId: "22222222-2222-4222-8222-222222222222", principalId: "33333333-3333-4333-8333-333333333333",
            draft: NativeExhibitionDraft.new(), selection: nil, mode: "translate", view: "isometric", disabled: false))
    }
    private func reply(_ request: ViewerResourceUsageRequest) -> DocumentValue {
        var response = request.value
        response["candidateSha256"] = .string(String(repeating: "a", count: 64))
        response["policy"] = .null; response["observations"] = .null; response["evaluation"] = .null
        return response
    }
    func testUnavailableObservationRemainsNullWithoutGuessedZeroOrStatus() throws {
        let request = try request(), response = reply(request)
        XCTAssertEqual(try request.checkedReply(response), response)
        XCTAssertEqual(response["evaluation"], .null)
    }
    func testEveryOwnerAndFrameBindingRejectsStaleReply() throws {
        let request = try request()
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "candidateSha256"] {
            var response = reply(request); response[key] = .string("stale")
            XCTAssertThrowsError(try request.checkedReply(response), key)
        }
    }
    func testLiveEditorObservationCannotClaimTargetViewerOrWholeExhibition() throws {
        let request = try request()
        var response = reply(request)
        response["evaluation"] = .object(["profile": .string("standard"), "scope": .string("wholeExhibition"), "status": .string("within")])
        XCTAssertThrowsError(try request.checkedReply(response))
        response["policy"] = .object(["policyVersion": .integer(1), "accountingVersion": .integer(1), "profile": .string("native")])
        response["observations"] = .object(["scope": .string("mounted-scene"), "complete": .bool(false)])
        response["evaluation"] = .object(["policyVersion": .integer(1), "accountingVersion": .integer(1), "profile": .string("native"), "scope": .string("analyzedSection"), "status": .string("unknown")])
        XCTAssertEqual(try request.checkedReply(response)["evaluation"]["status"], .string("unknown"))
    }
}
