// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class SpaceLightingSettingsTests: XCTestCase {
    let namespace = SpaceLightingFormBuffer.namespace
    let template = "org.exhibitos.studio/template"
    func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["extensions"] = .object([template: .object(["modified": .bool(false), "future": .integer(9)]), "future-extension": .object(["keep": .string("opaque")])])
        try draft.replaceCandidate(candidate); return draft
    }
    func explicit(_ draft: NativeExhibitionDraft, baseline: String = "0", reflection: String = "0") throws -> SpaceLightingCommand {
        var fields = try SpaceLightingFormBuffer(draft: draft)
        fields.usesOverride = true; fields.baselineIntensity = baseline; fields.reflectionIntensity = reflection
        return try fields.command()
    }
    func testAbsentOpeningAndUnchangedRemovalNeverMaterializeSettingsOrDefaults() throws {
        let draft = try fixture(), fields = try SpaceLightingFormBuffer(draft: draft)
        XCTAssertFalse(fields.usesOverride); XCTAssertEqual(fields.baselineIntensity, ""); XCTAssertEqual(fields.reflectionIntensity, "")
        XCTAssertEqual(try fields.command().proposedCandidate(in: draft), draft.candidate)
        XCTAssertEqual(draft.candidate["extensions"][template]["modified"], .bool(false))
    }
    func testPartialAndInvalidRawInputsRemainRawWithoutDraftMutation() throws {
        let draft = try fixture(); var fields = try SpaceLightingFormBuffer(draft: draft)
        fields.usesOverride = true
        for raw in ["", "-", "NaN", "inf", "1.01", "-0.01"] {
            fields.baselineIntensity = raw; fields.reflectionIntensity = "0.2"
            XCTAssertThrowsError(try fields.command()); XCTAssertEqual(fields.baselineIntensity, raw)
        }
        fields.baselineIntensity = "0.2"; fields.reflectionIntensity = "-"
        XCTAssertThrowsError(try fields.command()); XCTAssertEqual(fields.reflectionIntensity, "-")
        XCTAssertEqual(draft.candidate["extensions"][namespace], .null)
    }
    func testExplicitZeroPreviewApplyOneUndoAndColdDecodePreserveUnrelatedData() throws {
        let base = try fixture(), command = try explicit(base), candidate = try command.proposedCandidate(in: base)
        XCTAssertEqual(candidate["extensions"][namespace]["baselineIntensity"], .integer(0))
        XCTAssertEqual(candidate["extensions"][namespace]["reflectionIntensity"], .integer(0))
        XCTAssertEqual(candidate["extensions"][template]["modified"], .bool(true))
        XCTAssertEqual(candidate["extensions"]["future-extension"], base.candidate["extensions"]["future-extension"])
        let preview = try SpaceLightingPreview(candidate: candidate, base: base, command: command)
        XCTAssertEqual(base.candidate["extensions"][namespace], .null, "Transient preview never persists")
        var next = try preview.apply(to: base, command: command)
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(next))
        XCTAssertEqual(reopened.candidate, candidate)
        try next.replaceCandidate(base.candidate)
        XCTAssertEqual(next.candidate, base.candidate, "Existing single-history undo restores ABSENCE and original template provenance")
        for key in ["lights", "rooms", "artworks", "placements", "surfaces"] { XCTAssertEqual(candidate[key], base.candidate[key]) }
    }
    func testUnsetAndNumericallyUnchangedCommandsKeepExactNamespaceAndProvenance() throws {
        var base = try fixture(); let set = try explicit(base, baseline: "0.125", reflection: "0.25")
        try base.replaceCandidate(set.proposedCandidate(in: base))
        let unchanged = try SpaceLightingFormBuffer(draft: base)
        XCTAssertEqual(try unchanged.command().proposedCandidate(in: base), base.candidate)
        var remove = unchanged; remove.usesOverride = false
        let candidate = try remove.command().proposedCandidate(in: base)
        guard case .object(let extensions) = candidate["extensions"] else { return XCTFail("Extensions preserved") }
        XCTAssertNil(extensions[namespace], "Removal is absence, never a null-valued extension")
        XCTAssertEqual(candidate["extensions"]["future-extension"], base.candidate["extensions"]["future-extension"])
    }
    func testSharedReplyRejectsUnrelatedChangesAndChangedBase() throws {
        let base = try fixture(), command = try explicit(base)
        var candidate = try command.proposedCandidate(in: base); candidate["title"] = .string("tampered")
        XCTAssertThrowsError(try SpaceLightingPreview(candidate: candidate, base: base, command: command))
        candidate = try command.proposedCandidate(in: base)
        let preview = try SpaceLightingPreview(candidate: candidate, base: base, command: command)
        var changed = base; var other = base.candidate; other["title"] = .string("later"); try changed.replaceCandidate(other)
        XCTAssertThrowsError(try preview.apply(to: changed, command: command))
        XCTAssertThrowsError(try preview.apply(to: base, command: explicit(base, baseline: "1")))
    }
    func testQualifiedPresetsRequireExplicitEditAndPersistOnlyTheSameTwoValues() throws {
        let base = try fixture(); var fields = try SpaceLightingFormBuffer(draft: base)
        XCTAssertNil(fields.preset); XCTAssertFalse(fields.usesOverride)
        fields.selectPreset(.calm)
        XCTAssertEqual(fields.preset, .calm)
        let calm = try fields.command().proposedCandidate(in: base)
        XCTAssertEqual(calm["extensions"][namespace], .object(["version": .integer(1), "baselineIntensity": .number(0.15), "reflectionIntensity": .number(0.2)]))
        fields.selectPreset(.bright)
        XCTAssertEqual(fields.preset, .bright)
        let bright = try fields.command().proposedCandidate(in: base)
        XCTAssertEqual(bright["extensions"][namespace], .object(["version": .integer(1), "baselineIntensity": .number(0.2), "reflectionIntensity": .number(0.45)]))
        fields.baselineIntensity = "-"; XCTAssertNil(fields.preset); XCTAssertThrowsError(try fields.command())
        XCTAssertEqual(base.candidate["extensions"][namespace], .null)
    }
    func testBoundReplyAndCapabilityDoNotWidenStrictV1() throws {
        let base = try fixture(), command = try explicit(base)
        let frame = ViewportFrame(epoch: UUID().uuidString, sequence: 7, tenantId: UUID().uuidString, principalId: UUID().uuidString, draft: base, selection: nil, mode: "translate", view: "isometric", disabled: false)
        let request = try SpaceLightingViewportRequest(frame: frame, command: command)
        var reply = request.value
        guard case .object(var fields) = reply else { return XCTFail("Request") }
        fields.removeValue(forKey: "type"); fields.removeValue(forKey: "settings"); fields["candidate"] = try command.proposedCandidate(in: base); reply = .object(fields)
        XCTAssertNoThrow(try request.checkedReply(reply))
        for key in ["epoch", "tenantId", "principalId", "requestId"] { var forged = reply; forged[key] = .string(UUID().uuidString); XCTAssertThrowsError(try request.checkedReply(forged)) }
        var extra = reply; extra["settings"] = .null; XCTAssertThrowsError(try request.checkedReply(extra))
        var stale = reply; stale["sequence"] = .integer(8); XCTAssertThrowsError(try request.checkedReply(stale))
        let disabled = ViewportFrame(epoch: frame.epoch, sequence: 7, tenantId: frame.tenantId, principalId: frame.principalId, draft: base, selection: nil, mode: "translate", view: "isometric", disabled: true)
        XCTAssertThrowsError(try SpaceLightingViewportRequest(frame: disabled, command: command))
        let old = try JSONDecoder().decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"capabilities":["light-inspector-v1"]}"#.utf8))
        XCTAssertFalse(old.supportsSpaceSettings); XCTAssertTrue(old.supportsLightInspector)
        let fresh = try JSONDecoder().decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"capabilities":["space-settings-v1"]}"#.utf8))
        XCTAssertTrue(fresh.supportsSpaceSettings); XCTAssertFalse(fresh.supportsLightInspector)
    }
}
