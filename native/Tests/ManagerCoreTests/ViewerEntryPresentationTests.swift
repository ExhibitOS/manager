// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewerEntryPresentationTests: XCTestCase {
    func testLegacyAbsenceAndExplicitMixedPresentationRoundTripWithoutOtherChanges() throws {
        var draft = try NativeExhibitionDraft.new(title: "기존 전시")
        var candidate = draft.candidate
        let opaque: DocumentValue = .object(["future": .array([.string("보존 원문"), .integer(9_007_199_254_740_991)])])
        candidate["extensions"] = .object(["org.example/future": opaque])
        try draft.replaceCandidate(candidate)
        var settings = try draft.viewerEntranceSettings()
        XCTAssertNil(settings.presentation)
        XCTAssertTrue(settings.ticketAnimation && settings.coverUnfold && settings.doorEffect)
        settings.artist = "기존 작가"
        try draft.setViewerEntrance(settings)
        guard case .object(let legacyFields) = draft.candidate["extensions"][ViewerEntranceSettings.namespace] else { return XCTFail("Legacy entrance required") }
        XCTAssertNil(legacyFields["presentation"])
        let before = draft
        settings.ticketTitle = "전시 입장권"
        settings.ticketDetail = "안내 <plain text>"
        settings.ticketAnimation = false
        settings.doorEffect = false
        try draft.setViewerEntrance(settings)
        XCTAssertFalse(try draft.viewerEntranceSettings().ticketAnimation)
        XCTAssertTrue(try draft.viewerEntranceSettings().coverUnfold)
        XCTAssertFalse(try draft.viewerEntranceSettings().doorEffect)
        XCTAssertEqual(draft.candidate["extensions"]["org.example/future"], opaque)
        for key in ["id", "revisionId", "rooms", "surfaces", "openings", "placements", "artworks", "mediaAssets", "scripts", "accessibility"] {
            XCTAssertEqual(draft.candidate[key], before.candidate[key], key)
        }
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(draft))
        XCTAssertEqual(try reopened.viewerEntranceSettings(), settings)
        guard case .object(let fields) = reopened.candidate["extensions"][ViewerEntranceSettings.namespace]["presentation"] else { return XCTFail("Explicit presentation required") }
        XCTAssertEqual(Set(fields.keys), Set(["ticketTitle", "ticketDetail", "ticketAnimation", "coverUnfold", "doorEffect"]))
    }

    func testPresentationBoundsUseUTF16AndRefusalKeepsDraftAndRawInput() throws {
        var draft = try NativeExhibitionDraft.new()
        var settings = ViewerEntranceSettings()
        settings.ticketTitle = String(repeating: "😀", count: 80)
        settings.ticketDetail = String(repeating: "😀", count: 256)
        try draft.setViewerEntrance(settings)
        let saved = draft
        settings.ticketTitle += "x"
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
        XCTAssertEqual(settings.ticketTitle.utf16.count, 161)
        XCTAssertEqual(draft, saved)
        settings.ticketTitle.removeLast()
        settings.ticketDetail += "x"
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
        XCTAssertEqual(settings.ticketDetail.utf16.count, 513)
        XCTAssertEqual(draft, saved)
    }

    func testFutureAndMalformedPresentationRefuseWithoutDefaultRewrite() throws {
        var draft = try NativeExhibitionDraft.new()
        var defaults = ViewerEntranceSettings(); defaults.presentation = .init()
        try draft.setViewerEntrance(defaults)
        let valid = draft
        let value = valid.candidate["extensions"][ViewerEntranceSettings.namespace]["presentation"]
        var future = value; future["futureEffect"] = .bool(true)
        var wrongType = value; wrongType["ticketAnimation"] = .string("false")
        var incomplete = value
        if case .object(var fields) = incomplete { fields.removeValue(forKey: "coverUnfold"); incomplete = .object(fields) }
        for invalid in [DocumentValue.null, future, wrongType, incomplete] {
            var changed = valid.candidate
            changed["extensions"][ViewerEntranceSettings.namespace]["presentation"] = invalid
            var unsupported = valid; try unsupported.replaceCandidate(changed)
            let preserved = unsupported
            XCTAssertThrowsError(try unsupported.viewerEntranceSettings())
            XCTAssertThrowsError(try unsupported.setViewerEntrance(defaults))
            XCTAssertEqual(unsupported, preserved)
        }
    }
}
