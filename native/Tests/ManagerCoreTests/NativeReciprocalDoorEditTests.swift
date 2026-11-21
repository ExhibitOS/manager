// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeReciprocalDoorEditTests: XCTestCase {
    private let first = "10000000-0000-4000-8000-000000000001"
    private let second = "10000000-0000-4000-8000-000000000002"
    private let frame = "10000000-0000-4000-8000-000000000003"
    private let token = "10000000-0000-4000-8000-000000000004"
    private let ns = WorkspaceFurnishingPlacement.namespace

    private func draft(frameAttached: Bool = false) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new()
        var scene = draft.candidate
        scene["openings"] = .array([(first, second, 2.0), (second, first, 3.5)].map { id, peer, center in
            .object(["id": .string(id), "type": .string("door"), "connectsToOpeningId": .string(peer),
                     "dimensions": .object(["width": .number(2.4), "height": .number(3)]), "offset": .vector([center, -0.5])])
        })
        var extensions: [String: DocumentValue] = ["retained": .object(["tag": .string("unchanged")])]
        if frameAttached {
            extensions[ns] = .object(["version": .integer(6), "placements": .array([]), "frameAttachments": .array([]),
                "furnishings": .array([.object(["id": .string(frame), "kind": .string("portal-finish")])]),
                "openingAttachments": .array([.object(["openingId": .string(first), "furnishingId": .string(frame)])])])
        }
        scene["extensions"] = .object(extensions)
        try draft.replaceCandidate(scene)
        return draft
    }

    private func proposal(_ edit: NativeReciprocalDoorEdit, base: NativeExhibitionDraft) throws -> DocumentValue {
        let state = NativeDocumentState(format: 1, revision: 5, workspaceId: token, tenantId: token,
                                        principalId: token, draft: base, etag: nil, savedEditVersion: nil, pending: nil)
        return try edit.request(in: state, requestId: token)
    }

    private func reply(_ request: DocumentValue, candidate: DocumentValue, reversed: Bool = false) -> DocumentValue {
        var fields = Dictionary(uniqueKeysWithValues:
            ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"].map { ($0, request[$0]) })
        fields["candidate"] = candidate
        fields["openingIds"] = .array((reversed ? [second, first] : [first, second]).map(DocumentValue.string))
        return .object(fields)
    }

    func testFromBothEndpointsChecksSamePairAndRetainsFloor() throws {
        let base = try draft()
        var candidate = base.candidate
        candidate["openings"] = .array(try XCTUnwrap(candidate["openings"].items).map { row in
            var next = row
            next["dimensions"] = .object(["width": .number(2.5), "height": .number(2.5)])
            next["offset"] = .array([row["offset"].items![0], .number(-0.75)])
            return next
        })
        for id in [first, second] {
            let edit = try NativeReciprocalDoorEdit(openingId: id, dimensions: .init(width: 2.5, height: 2.5))
            let request = try proposal(edit, base: base)
            XCTAssertEqual(request["openingId"], .string(id))
            let accepted = try edit.checkedReply(reply(request, candidate: candidate), request: request, draft: base, approvedFrame: nil)
            XCTAssertEqual(accepted?.candidate, candidate)
            XCTAssertThrowsError(try edit.checkedReply(reply(request, candidate: candidate, reversed: true), request: request, draft: base, approvedFrame: nil))
            var oneSided = candidate
            oneSided["openings"] = .array([candidate["openings"].items![0], base.candidate["openings"].items![1]])
            XCTAssertThrowsError(try edit.checkedReply(reply(request, candidate: oneSided), request: request, draft: base, approvedFrame: nil))
        }
    }

    func testFrameRemovalOnThePeerPreservesOtherExtensions() throws {
        let base = try draft(frameAttached: true)
        XCTAssertTrue(NativeReciprocalDoorEdit.hasFrame(second, in: base))
        let edit = try NativeReciprocalDoorEdit(openingId: second, frame: .remove)
        let request = try proposal(edit, base: base)
        guard case .object(let fields) = request else { return XCTFail("Missing request envelope") }
        XCTAssertEqual(fields["frame"], .null)
        XCTAssertNil(fields["dimensions"])
        var candidate = base.candidate
        candidate["extensions"] = .object(["retained": base.candidate["extensions"]["retained"]])
        let accepted = try edit.checkedReply(reply(request, candidate: candidate), request: request, draft: base, approvedFrame: nil)
        XCTAssertEqual(accepted?.candidate, candidate)
        candidate["extensions"]["retained"]["tag"] = .string("tampered")
        XCTAssertThrowsError(try edit.checkedReply(reply(request, candidate: candidate), request: request, draft: base, approvedFrame: nil))
    }

    func testDiagnosticAcceptsOmittedIDsButRejectsUnrecognizedFieldsAndInvalidIDs() throws {
        func parse(_ value: String) throws -> NativeDoorPairViolation {
            try JSONDecoder().decode(NativeDoorPairViolation.self, from: Data(value.utf8))
        }
        XCTAssertEqual(try parse(#"{"constraint":"wall-bounds"}"#).constraint, .wallBounds)
        XCTAssertEqual(try parse("{\"constraint\":\"alignment\",\"openingId\":\"\(first)\"}").openingId, first)
        for invalid in [#"{"constraint":"wall-bounds","unknown":true}"#, #"{"constraint":"frame-fit","openingId":"unknown"}"#,
                        #"{"constraint":"alignment","roomId":null}"#, #"{"constraint":"unknown"}"#] {
            XCTAssertThrowsError(try parse(invalid))
        }
    }
}
