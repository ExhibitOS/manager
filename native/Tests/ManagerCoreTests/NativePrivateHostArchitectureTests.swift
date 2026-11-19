// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativePrivateHostArchitectureTests: XCTestCase {
    private func uuid() -> String { UUID().uuidString.lowercased() }
    private func room(_ id: String, type: String = "room-add", position: [Double] = [0,0,0]) -> DocumentValue {
        .object(["type": .string(type), "id": .string(id), "name": .string("Synthetic room"),
                 "width": .number(12), "height": .number(4), "depth": .number(8), "position": .vector(position)])
    }
    private func cube(_ roomID: String, _ ids: [String]) -> DocumentValue {
        .object(["type": .string("white-cube"), "roomId": .string(roomID), "surfaceIds": .array(ids.map { .string($0) })])
    }
    private func door(_ id: String, wall: String, width: Double = 1) -> DocumentValue {
        .object(["type": .string("opening-add"), "id": .string(id), "surfaceId": .string(wall),
                 "width": .number(width), "height": .number(2), "offset": .vector([0,-1])])
    }
    private func connect(_ first: String, _ second: String) -> DocumentValue {
        .object(["type": .string("connect-openings"), "firstId": .string(first), "secondId": .string(second)])
    }
    func testDefiniteFirstSaveRefusalRestoresArchitectureForCorrection() throws {
        let baseline = try NativeExhibitionDraft.new()
        let staged = try NativePrivateHostArchitecture.applying(room(uuid()), to: baseline)
        let recovery = NativeArchitectureSaveRecovery(before: baseline, staged: staged)
        for etag in [nil, "studio-r1-test"] as [String?] {
            let initial = NativeDocumentState(format: 1, revision: 2, workspaceId: uuid(), tenantId: uuid(),
                principalId: uuid(), draft: staged, etag: etag,
                savedEditVersion: etag == nil ? nil : baseline.document["editVersion"].integer, pending: nil)
            var pending = initial
            pending.revision += 1
            pending.pending = .init(etag: etag, draft: staged)
            let refusal = AuthoringFailure("DRAFT_INVALID", status: 422)
            let correction = try XCTUnwrap(recovery.correction(after: refusal, initial: initial, pending: pending))
            XCTAssertEqual(correction.draft, baseline)
            XCTAssertEqual(correction.etag, initial.etag)
            XCTAssertNil(correction.pending)
            XCTAssertNoThrow(try NativePrivateHostArchitecture.applying(room(uuid()), to: XCTUnwrap(correction.draft)))
            XCTAssertNil(recovery.correction(after: AuthoringFailure("RESPONSE_INVALID"), initial: initial, pending: pending))
            XCTAssertNil(recovery.correction(after: refusal, initial: pending, pending: pending))
        }
    }

    func testRetainedFirstSaveOnlyReplaysTheOriginalRequest() throws {
        let staged = try NativePrivateHostArchitecture.applying(room(uuid()), to: NativeExhibitionDraft.new())
        let pending = NativeDocumentState.Pending(etag: nil, draft: staged)
        let document = NativeDocumentState(format: 1, revision: 3, workspaceId: uuid(), tenantId: uuid(),
            principalId: uuid(), draft: staged, etag: nil, savedEditVersion: nil, pending: pending)
        XCTAssertEqual(NativeArchitectureSaveRecovery.retainedFirstSave(in: document), staged)
        var request: DocumentValue = .object(["requestId": .string(pending.requestId),
            "expectedRevision": .integer(3), "expectedEditVersion": staged.document["editVersion"]])
        XCTAssertNoThrow(try NativeAutomationSession.checkPendingSaveRequest(request, document, knownRevision: nil, ordinary: true))
        request["requestId"] = .string(uuid())
        XCTAssertThrowsError(try NativeAutomationSession.checkPendingSaveRequest(request, document, knownRevision: nil, ordinary: true))
        var altered = document
        altered.draft = try NativePrivateHostArchitecture.applying(room(uuid()), to: staged)
        XCTAssertNil(NativeArchitectureSaveRecovery.retainedFirstSave(in: altered))
    }

    func testRoomCubeDoorsAndRoundTripPreserveUnrelatedData() throws {
        var base = try NativeExhibitionDraft.new(), seed = base.candidate
        seed["extensions"] = .object(["example/preserved": .object(["value": .string("keep")])])
        try base.replaceCandidate(seed)
        let roomA = uuid(), roomB = uuid(), idsA = (0..<6).map { _ in uuid() }, idsB = (0..<6).map { _ in uuid() }
        let firstDoor = uuid(), secondDoor = uuid()
        var current = try NativePrivateHostArchitecture.applying(room(roomA), to: base)
        current = try NativePrivateHostArchitecture.applying(room(roomB, position: [12,0,0]), to: current)
        current = try NativePrivateHostArchitecture.applying(cube(roomA, idsA), to: current)
        current = try NativePrivateHostArchitecture.applying(cube(roomB, idsB), to: current)
        let surfaces = try XCTUnwrap(current.candidate["surfaces"].items)
        XCTAssertEqual(surfaces.count, 12)
        XCTAssertEqual(surfaces.prefix(6).map { $0["id"].string! }, idsA)
        XCTAssertEqual(surfaces.prefix(6).map { $0["type"].string! }, ["wall", "wall", "wall", "wall", "floor", "ceiling"])
        XCTAssertEqual(surfaces[0]["transform"]["position"], .vector([0,2,-4]))
        XCTAssertEqual(surfaces[4]["dimensions"]["height"], .number(8))
        current = try NativePrivateHostArchitecture.applying(door(firstDoor, wall: idsA[1]), to: current)
        current = try NativePrivateHostArchitecture.applying(door(secondDoor, wall: idsB[0]), to: current)
        current = try NativePrivateHostArchitecture.applying(connect(firstDoor, secondDoor), to: current)
        let openings = try XCTUnwrap(current.candidate["openings"].items)
        XCTAssertEqual(openings[0]["connectsToOpeningId"], .string(secondDoor))
        XCTAssertEqual(openings[1]["connectsToOpeningId"], .string(firstDoor))
        XCTAssertEqual(openings[0]["exterior"], .null)
        XCTAssertEqual(current.candidate["extensions"], base.candidate["extensions"])
        XCTAssertEqual(current.candidate["artworks"], base.candidate["artworks"])
        XCTAssertEqual(current.document["editVersion"].integer, base.document["editVersion"].integer! + 7)
        XCTAssertEqual(try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(current)), current)
    }

    func testNativeDoorPairProposalStagesOnlyTwoReciprocalOpenings() throws {
        let firstRoom = uuid(), secondRoom = uuid(), first = uuid(), second = uuid()
        let firstSurfaces = (0..<6).map { _ in uuid() }, secondSurfaces = (0..<6).map { _ in uuid() }
        var draft = try NativeExhibitionDraft.new()
        draft = try NativePrivateHostArchitecture.applying(room(firstRoom), to: draft)
        draft = try NativePrivateHostArchitecture.applying(room(secondRoom, position: [0,0,8]), to: draft)
        draft = try NativePrivateHostArchitecture.applying(cube(firstRoom, firstSurfaces), to: draft)
        draft = try NativePrivateHostArchitecture.applying(cube(secondRoom, secondSurfaces), to: draft)
        let dims: DocumentValue = .object(["width": .number(2.4), "height": .integer(3)])
        func side(_ wall: String) -> DocumentValue {
            .object(["surfaceId": .string(wall), "offset": .vector([0,-0.5])])
        }
        let proposal = try NativeDoorPairProposal(command: .object([
            "dimensions": dims, "first": side(firstSurfaces[1]), "second": side(secondSurfaces[0])
        ]), draft: draft, tenantId: uuid(), principalId: uuid(), requestId: uuid())
        func opening(_ id: String, _ wall: String, _ peer: String) -> DocumentValue {
            .object(["id": .string(id), "type": .string("door"), "surfaceId": .string(wall),
                     "dimensions": dims, "offset": .vector([0,-0.5]), "connectsToOpeningId": .string(peer)])
        }
        let ids = [DocumentValue.string(first), .string(second)]
        var candidate = draft.candidate
        candidate["openings"] = .array([opening(first, firstSurfaces[1], second), opening(second, secondSurfaces[0], first)])
        var reply = proposal.value
        guard case .object(var fields) = reply else { return XCTFail("Expected a proposal object") }
        for key in ["type", "dimensions", "first", "second"] { fields.removeValue(forKey: key) }
        fields["candidate"] = candidate
        fields["openingIds"] = .array(ids)
        reply = .object(fields)
        let accepted = try proposal.checkedReply(reply)
        XCTAssertEqual(accepted.openingIds, .array(ids))
        XCTAssertEqual(accepted.draft.candidate, candidate)
        XCTAssertEqual(accepted.draft.document["editVersion"].integer, draft.document["editVersion"].integer! + 1)
        var changed = reply
        changed["candidate"]["title"] = .string("Unexpected replacement")
        XCTAssertThrowsError(try proposal.checkedReply(changed))
        changed = reply
        changed["openingIds"] = .array(Array(ids.reversed()))
        XCTAssertThrowsError(try proposal.checkedReply(changed))
        changed = reply
        changed["requestId"] = .string(uuid())
        XCTAssertThrowsError(try proposal.checkedReply(changed))
    }
    func testRoomUpdatePreservesUnknownFieldsAndRejectsInvalidInput() throws {
        var base = try NativeExhibitionDraft.new(), candidate = base.candidate
        var original = base.rooms[0]; original["extensions"] = .object(["retained": .bool(true)])
        original["dimensions"]["future"] = .number(7); candidate["rooms"] = .array([original])
        try base.replaceCandidate(candidate)
        let id = try XCTUnwrap(original["id"].string)
        let updated = try NativePrivateHostArchitecture.applying(room(id, type: "room-update", position: [1,2,3]), to: base)
        XCTAssertEqual(updated.rooms[0]["dimensions"]["future"], .number(7))
        XCTAssertEqual(updated.rooms[0]["extensions"], original["extensions"])
        XCTAssertEqual(updated.rooms[0]["transform"]["position"], .vector([1,2,3]))
        XCTAssertEqual(updated.document["editVersion"].integer, base.document["editVersion"].integer! + 1)
        var bad = room(id, type: "room-update"); bad["width"] = .number(.nan)
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(bad, to: base))
        bad = room(id, type: "room-update"); bad["extra"] = .bool(true)
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(bad, to: base))
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(room(uuid(), type: "room-update"), to: base))
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(room(id), to: base))
    }
    func testGeometryIdentityAndConnectionFailuresLeaveOriginalUntouched() throws {
        let base = try NativeExhibitionDraft.new(), idA = uuid(), idB = uuid(), ids = (0..<6).map { _ in uuid() }
        var draft = try NativePrivateHostArchitecture.applying(room(idA), to: base)
        draft = try NativePrivateHostArchitecture.applying(room(idB), to: draft)
        var duplicateIDs = ids; duplicateIDs[5] = ids[0]
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(cube(idA, duplicateIDs), to: draft))
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(cube(uuid(), ids), to: draft))
        draft = try NativePrivateHostArchitecture.applying(cube(idA, ids), to: draft)
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(cube(idA, (0..<6).map { _ in uuid() }), to: draft))
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(door(uuid(), wall: ids[4]), to: draft))
        let first = uuid(), second = uuid()
        draft = try NativePrivateHostArchitecture.applying(door(first, wall: ids[0]), to: draft)
        draft = try NativePrivateHostArchitecture.applying(door(second, wall: ids[1]), to: draft)
        let before = draft
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(connect(first, first), to: draft))
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(connect(first, uuid()), to: draft))
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(connect(first, second), to: draft))
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(.object(["type": .string("replace-draft"), "candidate": draft.candidate]), to: draft))
        XCTAssertEqual(draft, before)
    }
    func testConnectionRequiresMatchingDimensionsAndDistinctExistingHosts() throws {
        let base = try NativeExhibitionDraft.new(), roomA = uuid(), roomB = uuid()
        let wallsA = (0..<6).map { _ in uuid() }, wallsB = (0..<6).map { _ in uuid() }
        let a = uuid(), b = uuid()
        var draft = try NativePrivateHostArchitecture.applying(room(roomA), to: base)
        draft = try NativePrivateHostArchitecture.applying(room(roomB), to: draft)
        draft = try NativePrivateHostArchitecture.applying(cube(roomA, wallsA), to: draft)
        draft = try NativePrivateHostArchitecture.applying(cube(roomB, wallsB), to: draft)
        draft = try NativePrivateHostArchitecture.applying(door(a, wall: wallsA[0]), to: draft)
        draft = try NativePrivateHostArchitecture.applying(door(b, wall: wallsB[0], width: 1.5), to: draft)
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(connect(a, b), to: draft))
        var notFloor = door(uuid(), wall: wallsA[0]); notFloor["offset"] = .vector([0,0])
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(notFloor, to: draft))
    }
    func testSurfaceUpdatePreservesOrientationOwnershipAndOtherFields() throws {
        let initial = try NativeExhibitionDraft.new(), roomID = try XCTUnwrap(initial.rooms[0]["id"].string)
        let ids = (0..<6).map { _ in uuid() }
        var base = try NativePrivateHostArchitecture.applying(cube(roomID, ids), to: initial)
        var candidate = base.candidate, surfaces = try XCTUnwrap(candidate["surfaces"].items)
        surfaces[0]["extensions"] = .object(["example/preserved": .string("keep")])
        candidate["surfaces"] = .array(surfaces); try base.replaceCandidate(candidate)
        let original = surfaces[0]
        let command: DocumentValue = .object(["type": .string("surface-update"), "id": .string(ids[0]),
            "width": .number(10), "height": .number(3), "position": .vector([0,1.5,-4])])
        let result = try NativePrivateHostArchitecture.applying(command, to: base)
        let changed = try XCTUnwrap(result.candidate["surfaces"].items?.first)
        XCTAssertEqual(changed["roomId"], original["roomId"])
        XCTAssertEqual(changed["type"], original["type"])
        XCTAssertEqual(changed["transform"]["rotation"], original["transform"]["rotation"])
        XCTAssertEqual(changed["transform"]["scale"], original["transform"]["scale"])
        XCTAssertEqual(changed["extensions"], original["extensions"])
        XCTAssertEqual(changed["dimensions"]["width"], .number(10))
        XCTAssertEqual(changed["transform"]["position"], .vector([0,1.5,-4]))
        XCTAssertEqual(result.candidate["surfaces"].items?.dropFirst().map { $0 }, Array(surfaces.dropFirst()))
        var bad = command; bad["id"] = .string(uuid())
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(bad, to: base))
        bad = command; bad["height"] = .number(.nan)
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(bad, to: base))
        bad = command; bad["rotation"] = .vector([0,0,0,1])
        XCTAssertThrowsError(try NativePrivateHostArchitecture.applying(bad, to: base))
    }
}
