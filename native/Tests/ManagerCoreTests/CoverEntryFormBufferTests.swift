// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class CoverEntryFormBufferTests: XCTestCase {
    func testSparseThemesRoundTripWithoutResolvingDefaultsOrChangingCandidate() throws {
        for theme in CoverEntryTheme.allCases {
            var draft = try NativeExhibitionDraft.new(title: "보관할 제목")
            let original = draft
            var settings = try draft.viewerEntranceSettings()
            settings.artist = "작가"; settings.ticketTitle = "입장권"
            settings.design = CoverEntryDesign(theme: theme)
            try draft.setViewerEntrance(settings)
            let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(draft))
            let decoded = try reopened.viewerEntranceSettings()
            XCTAssertEqual(decoded, settings)
            XCTAssertEqual(decoded.design?.raw, .object(["version": .integer(1), "theme": .string(theme.rawValue)]))
            let buffer = CoverEntryFormBuffer(design: decoded.design)
            XCTAssertFalse(buffer.isDirty)
            XCTAssertEqual(try buffer.value(), decoded.design)
            for key in ["title", "id", "revisionId", "artworks", "rooms", "surfaces", "placements", "accessibility"] {
                XCTAssertEqual(reopened.candidate[key], original.candidate[key], key)
            }
        }
        let legacy = CoverEntryFormBuffer(design: nil)
        XCTAssertFalse(legacy.isDirty); XCTAssertNil(try legacy.value())
    }

    func testFutureAndMalformedDesignRefuseAtomicallyWithRawDocumentPreserved() throws {
        var draft = try NativeExhibitionDraft.new()
        var settings = ViewerEntranceSettings(); settings.design = CoverEntryDesign(theme: .poster)
        try draft.setViewerEntrance(settings)
        let valid = draft
        let bad: [DocumentValue] = [
            .object(["version": .integer(2), "theme": .string("poster")]),
            .object(["version": .integer(1), "theme": .string("future")]),
            .object(["version": .integer(1), "theme": .string("poster"), "future": .bool(true)]),
            .object(["version": .integer(1), "theme": .string("poster"), "image": .object(["future": .integer(1)])]),
            .object(["version": .integer(1), "theme": .string("poster"), "effects": .object(["speed": .string("1")])]), .null
        ]
        for value in bad {
            var candidate = valid.candidate
            candidate["extensions"][ViewerEntranceSettings.namespace]["design"] = value
            var unsupported = valid; try unsupported.replaceCandidate(candidate)
            let before = unsupported
            XCTAssertThrowsError(try unsupported.viewerEntranceSettings())
            XCTAssertThrowsError(try unsupported.setViewerEntrance(settings))
            XCTAssertEqual(unsupported, before)
        }
    }

    func testRawNumericEditsRemainAfterRefusalAndThemeResetHasExplicitLimitedScope() throws {
        var design = CoverEntryDesign(theme: .poster)
        design.set("layout", "copyRatio", .number(0.35))
        design.set("effects", "cover", .string("mask"))
        design.set("image", "fit", .string("cover"))
        design.set("ticket", "showDate", .bool(false))
        design.set(nil, "palette", .string("dark"))
        var buffer = CoverEntryFormBuffer(design: design)
        buffer.numbers[.margin] = "-"
        let editing = buffer
        XCTAssertThrowsError(try buffer.value()); XCTAssertEqual(buffer, editing)
        XCTAssertEqual(buffer.numbers[.margin], "-")
        buffer.numbers[.margin] = "64"
        XCTAssertEqual(try buffer.value()?.raw["layout"]["margin"], .number(64))
        buffer.numbers[.focusX] = "0.25"
        XCTAssertThrowsError(try buffer.value())
        buffer.numbers[.focusY] = "0.75"
        XCTAssertEqual(try buffer.value()?.raw["image"]["focus"], .vector([0.25, 0.75]))
        buffer.selectTheme(.catalogue)
        let changed = try XCTUnwrap(buffer.value())
        XCTAssertEqual(changed.theme, .catalogue)
        XCTAssertEqual(changed.raw["layout"], .null); XCTAssertEqual(changed.raw["effects"], .null)
        XCTAssertEqual(changed.raw["image"]["fit"], .string("cover"))
        XCTAssertEqual(changed.raw["image"]["focus"], .vector([0.25, 0.75]))
        XCTAssertEqual(changed.raw["ticket"], design.raw["ticket"])
        XCTAssertEqual(changed.raw["palette"], design.raw["palette"])
    }

    func testBoundariesThemeCompatibilityLocalesAndEscapedBudget() throws {
        func check(_ group: String?, _ field: String, _ value: DocumentValue, theme: CoverEntryTheme = .poster, accepted: Bool) {
            var design = CoverEntryDesign(theme: theme); design.set(group, field, value)
            if accepted { XCTAssertNoThrow(try design.validate()) } else { XCTAssertThrowsError(try design.validate()) }
        }
        check("layout", "copyRatio", .number(0.3), accepted: true)
        check("layout", "copyRatio", .number(0.7), accepted: true)
        check("layout", "copyRatio", .number(0.3), theme: .catalogue, accepted: false)
        check("layout", "margin", .number(64.01), accepted: false)
        check("layout", "titleScale", .number(0.75), accepted: true)
        check("effects", "strength", .number(.nan), accepted: false)
        check("effects", "speed", .number(0.49), accepted: false)
        check("image", "focus", .vector([0, 1]), accepted: true)
        check("image", "focus", .vector([0, 1, 0]), accepted: false)
        for theme in CoverEntryTheme.allCases {
            for stage in ["cover", "response", "transition"] {
                for effect in theme.effects(for: stage) { check("effects", stage, .string(effect), theme: theme, accepted: true) }
            }
        }
        check("effects", "cover", .string("fold"), accepted: false)
        let korean: DocumentValue = .object(["locale": .string("ko-KR"), "title": .string(String(repeating: "😀", count: 256))])
        check(nil, "translations", .array([korean]), accepted: true)
        var duplicate = korean; duplicate["locale"] = .string("KO-kr")
        check(nil, "translations", .array([korean, duplicate]), accepted: false)
        check(nil, "translations", .array([.object(["locale": .string("ko-x")])]), accepted: false)
        var oversized = korean; oversized["title"] = .string(String(repeating: "😀", count: 257))
        check(nil, "translations", .array([oversized]), accepted: false)
        let escaped = (0..<16).map { i in DocumentValue.object(["locale": .string("ko-\(10 + i)"), "introduction": .string(String(repeating: "\u{0001}", count: 4096))]) }
        check(nil, "translations", .array(escaped), accepted: false)
        let unicode = (0..<16).map { i in DocumentValue.object(["locale": .string("ko-\(10 + i)"), "introduction": .string(String(repeating: "가", count: 4096))]) }
        check(nil, "translations", .array(unicode), accepted: false)
    }
}
