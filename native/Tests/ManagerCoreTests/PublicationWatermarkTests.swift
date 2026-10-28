// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class PublicationWatermarkTests: XCTestCase {
    func testAbsentDefaultAndOnlyExplicitEditCreatesNoneWithoutChangingOtherData() throws {
        var draft = try NativeExhibitionDraft.new(title: "새 전시")
        let original = draft
        XCTAssertNil(try draft.publicationWatermark())
        XCTAssertFalse(PublicationWatermarkFormBuffer().isDirty)
        try draft.setPublicationWatermark(PublicationWatermark())
        XCTAssertEqual(draft.candidate["extensions"][PublicationWatermark.namespace]["watermark"], .object(["version": .integer(1), "mode": .string("none")]))
        XCTAssertEqual(try BenchViewpoint.list(in: draft), [])
        for key in ["id", "revisionId", "rooms", "placements", "artworks", "accessibility"] { XCTAssertEqual(draft.candidate[key], original.candidate[key]) }
        let version = draft.document["editVersion"]
        try draft.setPublicationWatermark(PublicationWatermark())
        XCTAssertEqual(draft.document["editVersion"], version)
    }

    func testSiblingsRoundtripCopyAndUndoRetainCreditsIdentityAndAuthorCase() throws {
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        let presentation: DocumentValue = .object(["version": .integer(1), "viewpoints": .array([]), "credits": .string("필수 출처 원문"), "startCamera": .object(["opaque": .string("보존")])])
        candidate["extensions"] = .object([PublicationWatermark.namespace: presentation, "org.example/unknown": .integer(9_007_199_254_740_991)])
        try draft.replaceCandidate(candidate)
        let before = draft
        let settings = PublicationWatermark(mode: .embedded, text: "  Author-12 @Museum  ", position: .topLeft, size: 8, opacity: 0.6)
        try draft.setPublicationWatermark(settings)
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(draft))
        XCTAssertEqual(try reopened.publicationWatermark()?.text, "Author-12 @Museum")
        for key in ["viewpoints", "credits", "startCamera"] { XCTAssertEqual(reopened.candidate["extensions"][PublicationWatermark.namespace][key], presentation[key]) }
        XCTAssertEqual(reopened.candidate["extensions"]["org.example/unknown"], before.candidate["extensions"]["org.example/unknown"])
        let copy = try reopened.copyForPublication()
        XCTAssertNotEqual(copy.id, reopened.id)
        XCTAssertEqual(try copy.publicationWatermark(), try reopened.publicationWatermark())
        XCTAssertEqual(copy.candidate["extensions"][PublicationWatermark.namespace]["credits"], .string("필수 출처 원문"))
        var undone = reopened; try undone.replaceCandidate(before.candidate)
        XCTAssertEqual(undone.id, reopened.id); XCTAssertNil(try undone.publicationWatermark())
        try undone.replaceCandidate(reopened.candidate)
        XCTAssertEqual(undone.candidate, reopened.candidate)
    }

    func testUnicodeCodepointBoundsControlsAndExplicitBlankAreRefusedWithoutMutation() throws {
        var draft = try NativeExhibitionDraft.new()
        let text = String(repeating: "e\u{301}", count: 80) // 80 graphemes, 160 shared Unicode codepoints.
        try draft.setPublicationWatermark(.init(mode: .overlay, text: text))
        let saved = draft
        for text in [text+"x", " "+text, text+" ", "\u{FEFF}", "\u{85}앞", "\n앞", "앞\n뒤", "앞\u{85}뒤", "   "] {
            XCTAssertThrowsError(try draft.setPublicationWatermark(.init(mode: .overlay, text: text)))
            XCTAssertEqual(draft, saved)
        }
        XCTAssertThrowsError(try PublicationWatermark(value: .object(["version": .integer(1), "mode": .string("overlay"), "text": .string("")])))
        XCTAssertThrowsError(try PublicationWatermark(value: .object(["version": .integer(1), "mode": .string("overlay"), "text": .string("\u{FEFF}")])))
        try draft.setPublicationWatermark(.init(mode: .overlay, text: "  전시 · 작가  "))
        XCTAssertEqual(try draft.publicationWatermark()?.text, "전시 · 작가")
        try draft.setPublicationWatermark(.init(mode: .overlay, text: "\u{FEFF}전시 · 작가\u{FEFF}"))
        XCTAssertEqual(try draft.publicationWatermark()?.text, "전시 · 작가")
    }

    func testExactEmbeddedGlyphSetAndCasePreservationDoNotInventMultilingualSupport() throws {
        let allowed = "Az09 .,:;!?\"'_/@()+-"
        XCTAssertTrue(PublicationWatermark.supportsEmbeddedText(allowed))
        var draft = try NativeExhibitionDraft.new()
        try draft.setPublicationWatermark(.init(mode: .embedded, text: allowed))
        XCTAssertEqual(try draft.publicationWatermark()?.text, allowed)
        let saved = draft
        for text in ["[test]", "A&B", "A\\B", "é", "작가", String(repeating: "A", count: 81)] {
            XCTAssertThrowsError(try draft.setPublicationWatermark(.init(mode: .embedded, text: text)))
            XCTAssertEqual(draft, saved)
        }
        XCTAssertNoThrow(try PublicationWatermark(mode: .overlay, text: "작가 😀 & 출처").validate())
    }

    func testRawBufferKeepsInvalidInputsAndNoneDropsOnlyInactiveWatermarkFields() throws {
        var fields = PublicationWatermarkFormBuffer()
        fields.mode = .overlay; fields.size = "-"; fields.opacity = "0."
        XCTAssertTrue(fields.isDirty); XCTAssertThrowsError(try fields.settings())
        XCTAssertEqual(fields.size, "-"); XCTAssertEqual(fields.opacity, "0.")
        fields.mode = .none
        XCTAssertFalse(fields.isDirty)
        XCTAssertEqual(try fields.settings().value, .object(["version": .integer(1), "mode": .string("none")]))
        fields.mode = .overlay; fields.size = "24"; fields.opacity = "0.6"
        XCTAssertNoThrow(try fields.settings())
        fields.opacity = "nan"; XCTAssertThrowsError(try fields.settings())
        XCTAssertEqual(fields.opacity, "nan")
    }

    func testFutureMalformedSettingsRemainOpaqueAndCurrentRuntimeCannotEmitThem() throws {
        var draft = try NativeExhibitionDraft.new()
        try draft.setPublicationWatermark(.init(mode: .overlay))
        let valid = draft
        let good = valid.candidate["extensions"][PublicationWatermark.namespace]["watermark"]
        var future = good; future["version"] = .integer(2)
        var extra = good; extra["future"] = .bool(true)
        var wrong = good; wrong["opacity"] = .string("0.25")
        for raw in [DocumentValue.null, future, extra, wrong] {
            var candidate = valid.candidate; candidate["extensions"][PublicationWatermark.namespace]["watermark"] = raw
            var changed = valid; try changed.replaceCandidate(candidate)
            let preserved = changed
            XCTAssertThrowsError(try changed.publicationWatermark())
            XCTAssertThrowsError(try changed.setPublicationWatermark(.init()))
            XCTAssertEqual(changed, preserved)
        }
        XCTAssertFalse(PublicationWatermarkRuntime.supports(manifestSHA256: "74b08c7899d74ad989bbb61e47fc86fe9fa25c8853ab27ce57e9135c4932e96a", ready: true))
        XCTAssertFalse(PublicationWatermarkRuntime.supports(manifestSHA256: String(repeating: "f", count: 64), ready: true))
        XCTAssertFalse(PublicationWatermarkRuntime.supports(manifestSHA256: nil, ready: false))
    }
}
