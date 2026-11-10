// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class FurnishingWalkingCollisionEvaluationTests: XCTestCase {
    private let id = "ad000000-0000-4000-8000-000000000001"

    func testExplicitOffRemainsDistinctFromAnInheritedDefault() throws {
        let draft = try NativeExhibitionDraft.new()
        let inherited = row()
        let inheritedResponse = response([inherited])
        XCTAssertEqual(try FurnishingWalkingCollisionEvaluation(inheritedResponse, in: draft).value, inheritedResponse)

        var edited = draft
        var candidate = draft.candidate
        candidate["extensions"] = .object([
            "org.exhibitos.studio/furnishing-walking-collision": .object([
                "version": .integer(1),
                "overrides": .array([.object(["kind": .string("workspace"), "id": .string(id), "enabled": .bool(false)])])
            ])
        ])
        try edited.replaceCandidate(candidate)
        var off = inherited
        off["override"] = .bool(false)
        let offResponse = response([off])
        XCTAssertEqual(try FurnishingWalkingCollisionEvaluation(offResponse, in: edited).value, offResponse)
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(inheritedResponse, in: edited))
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(offResponse, in: draft))
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([]), in: edited))
    }

    func testRejectsDuplicateTargetsUnknownKindsAndNullOverrides() throws {
        let draft = try NativeExhibitionDraft.new()
        var duplicate = row()
        duplicate["id"] = .string(id.uppercased())
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([row(), duplicate]), in: draft))

        var unknown = row()
        unknown["kind"] = .string("wall")
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([unknown]), in: draft))
        var nullOverride = row()
        nullOverride["override"] = .null
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([nullOverride]), in: draft))
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response(Array(repeating: row(), count: 1025)), in: draft))
    }

    func testProposalWritesOnlyRequestedOverrideAndRestoresViaEditHistory() throws {
        let draft = try NativeExhibitionDraft.new()
        let target = try FurnishingWalkingCollisionTarget(row())
        let set = FurnishingWalkingCollisionProposal(draft: draft, target: target, setting: .off)
        let off = override(id, false)
        let proposed = candidate(draft, rows: [off])
        let edited = try XCTUnwrap(set.applying(reply(proposed, changed: true)))
        XCTAssertEqual(edited.document["editVersion"].integer, 2)
        XCTAssertEqual(edited.candidate, proposed)
        XCTAssertEqual(try FurnishingWalkingCollisionEvaluation(response([rowWithOverride(false)]), in: edited).targets.first?.setting, .off)

        let replay = FurnishingWalkingCollisionProposal(draft: edited, target: target, setting: .off)
        XCTAssertNil(try replay.applying(reply(edited.candidate, changed: false)))
        XCTAssertThrowsError(try replay.applying(reply(edited.candidate, changed: true)))

        let reset = FurnishingWalkingCollisionProposal(draft: edited, target: target, setting: .inherit)
        let cleared = try XCTUnwrap(reset.applying(reply(candidate(edited, rows: []), changed: true)))
        XCTAssertEqual(cleared.document["editVersion"].integer, 3)
        XCTAssertEqual(cleared.candidate["extensions"], .object([:]))
        XCTAssertEqual(try FurnishingWalkingCollisionEvaluation(response([row()]), in: cleared).targets.first?.setting, .inherit)

        var undone = cleared
        try undone.restoreEditHistory(edited)
        XCTAssertEqual(undone.candidate, edited.candidate)
        XCTAssertEqual(undone.document["editVersion"].integer, 4)
    }

    func testProposalRejectsUnrelatedCandidateChangesAndPreservesOverrideOrder() throws {
        let other = override("ad000000-0000-4000-8000-000000000002", true)
        var draft = try NativeExhibitionDraft.new()
        var starting = candidate(draft, rows: [override(id, false), other])
        starting["extensions"]["org.example.opaque"] = .object(["value": .string("keep")])
        try draft.replaceCandidate(starting)
        let proposal = FurnishingWalkingCollisionProposal(draft: draft, target: try .init(row()), setting: .on)
        let exact = candidate(draft, rows: [other, override(id.uppercased(), true)])
        XCTAssertEqual(try proposal.applying(reply(exact, changed: true))?.candidate, exact)
        var otherTitle = exact
        otherTitle["title"] = .string("Changed title")
        XCTAssertThrowsError(try proposal.applying(reply(otherTitle, changed: true)))
        var otherExtension = exact
        otherExtension["extensions"]["org.example.opaque"] = .null
        XCTAssertThrowsError(try proposal.applying(reply(otherExtension, changed: true)))
        XCTAssertThrowsError(try proposal.applying(reply(candidate(draft, rows: [override(id, true)]), changed: true)))
        XCTAssertThrowsError(try proposal.applying(reply(candidate(draft, rows: [override(id, true), other]), changed: true)))
        XCTAssertThrowsError(try proposal.applying(reply(exact, changed: false)))
    }

    func testCommandRequiresAnExplicitBooleanOrNullAndEncodesNull() throws {
        let target = DocumentValue.object(["kind": .string("workspace"), "id": .string(id)])
        let input = DocumentValue.object(["target": target, "enabled": .null])
        let (decoded, setting) = try FurnishingWalkingCollisionProposal.command(input)
        XCTAssertEqual(setting, .inherit)
        let proposal = FurnishingWalkingCollisionProposal(draft: try .new(), target: decoded, setting: setting)
        let value = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(proposal))
        guard case .object(let fields) = value else { return XCTFail("Expected request object") }
        XCTAssertEqual(Set(fields.keys), ["draft", "target", "enabled"])
        XCTAssertEqual(fields["enabled"], .some(.null))
        XCTAssertThrowsError(try FurnishingWalkingCollisionProposal.command(.object(["target": target])))
        XCTAssertThrowsError(try FurnishingWalkingCollisionProposal.command(.object(["target": target, "enabled": .integer(0)])))
        XCTAssertThrowsError(try FurnishingWalkingCollisionProposal.command(.object(["target": row(), "enabled": .bool(false)])))
        XCTAssertEqual(try FurnishingWalkingCollisionProposal.command(.object(["target": target, "enabled": .bool(false)])).1, .off)
    }

    private func candidate(_ draft: NativeExhibitionDraft, rows: [DocumentValue]) -> DocumentValue {
        let namespace = "org.exhibitos.studio/furnishing-walking-collision"
        var candidate = draft.candidate
        var extensions: [String: DocumentValue] = [:]
        if case .object(let fields) = candidate["extensions"] { extensions = fields }
        extensions[namespace] = rows.isEmpty ? nil : .object(["version": .integer(1), "overrides": .array(rows)])
        candidate["extensions"] = .object(extensions)
        return candidate
    }

    private func override(_ id: String, _ enabled: Bool) -> DocumentValue {
        .object(["kind": .string("workspace"), "id": .string(id), "enabled": .bool(enabled)])
    }

    private func rowWithOverride(_ enabled: Bool) -> DocumentValue {
        var value = row()
        value["override"] = .bool(enabled)
        return value
    }

    private func reply(_ candidate: DocumentValue, changed: Bool) -> DocumentValue {
        .object(["candidate": candidate, "changed": .bool(changed)])
    }

    private func row() -> DocumentValue {
        .object(["kind": .string("workspace"), "id": .string(id), "defaultEnabled": .bool(true)])
    }

    private func response(_ rows: [DocumentValue]) -> DocumentValue { .object(["targets": .array(rows)]) }
}
