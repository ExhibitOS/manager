// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewerEntranceSettingsTests: XCTestCase {
    private let imageID = "12345678-1234-1234-1234-123456789abc"

    private func imageDraft(bytes: DocumentValue = .integer(8 * 1024 * 1024)) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(title: "원래 전시")
        var candidate = draft.candidate
        candidate["artworks"] = .array([.object([
            "artworkType": .string("image"), "primaryAssetId": .string(imageID),
            "metadata": .object(["title": .string("승인된 기존 PNG")]),
            "rights": .object(["opaque-review": .array([.bool(true), .integer(9_007_199_254_740_991)])]),
            "assets": .array([.object(["id": .string(imageID), "role": .string("image"), "mime": .string("image/png"), "bytes": bytes])])
        ])])
        try draft.replaceCandidate(candidate)
        return draft
    }

    func testDefaultRoundTripAndEditPreserveOpaqueDocumentAndTitlePath() throws {
        var draft = try imageDraft()
        var candidate = draft.candidate
        let opaque: DocumentValue = .object(["future": .array([.null, .integer(9_007_199_254_740_991), .string("원문")])])
        candidate["extensions"] = .object(["org.example/unknown": opaque])
        candidate["scripts"] = .array([opaque])
        try draft.replaceCandidate(candidate)
        XCTAssertEqual(try draft.viewerEntranceSettings(), ViewerEntranceSettings())
        XCTAssertEqual(draft.viewerEntranceImageChoices().map(\.id), [imageID])
        let before = draft
        let settings = ViewerEntranceSettings(artist: "작가", introduction: "전시 소개", imageAssetId: imageID.uppercased(), imageAlt: "정면에서 본 작품", mobilePosition: .top)
        try draft.setViewerEntrance(settings)
        XCTAssertEqual(try draft.viewerEntranceSettings(), settings)
        XCTAssertEqual(draft.id, before.id)
        XCTAssertEqual(draft.candidate["revisionId"], before.candidate["revisionId"])
        XCTAssertEqual(draft.title, before.title)
        XCTAssertEqual(draft.artworks, before.artworks)
        XCTAssertEqual(draft.candidate["scripts"], before.candidate["scripts"])
        XCTAssertEqual(draft.candidate["extensions"]["org.example/unknown"], opaque)
        XCTAssertEqual(draft.document["editVersion"].integer, before.document["editVersion"].integer! + 1)
        let roundTrip = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(draft))
        XCTAssertEqual(roundTrip, draft)
        try draft.rename("기존 제목 수정")
        XCTAssertEqual(try draft.viewerEntranceSettings(), settings)
        XCTAssertEqual(draft.title, "기존 제목 수정")
        var typography = settings; typography.imageAssetId = nil; typography.imageAlt = ""
        try draft.setViewerEntrance(typography)
        XCTAssertEqual(try draft.viewerEntranceSettings(), typography)
        guard case .object(let fields) = draft.candidate["extensions"][ViewerEntranceSettings.namespace] else { return XCTFail("Entrance object required") }
        XCTAssertNil(fields["imageAssetId"])
    }

    func testBoundedProseUsesPlatformUTF16AndInvalidWritesPreserveDraft() throws {
        var draft = try imageDraft()
        var settings = ViewerEntranceSettings(artist: String(repeating: "😀", count: 256), introduction: String(repeating: "a", count: 4096), imageAlt: String(repeating: "a", count: 2048))
        try draft.setViewerEntrance(settings)
        let saved = draft
        settings.artist += "😀"
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
        XCTAssertEqual(draft, saved)
        settings = ViewerEntranceSettings(introduction: String(repeating: "a", count: 4097))
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
        settings = ViewerEntranceSettings(imageAlt: String(repeating: "a", count: 2049))
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
        settings = ViewerEntranceSettings(imageAssetId: imageID, imageAlt: "\u{FEFF}\n ")
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
        XCTAssertEqual(draft, saved)
    }

    func testCoverRequiresUniquePrimaryPNGBoundedBytes() throws {
        let settings = ViewerEntranceSettings(imageAssetId: imageID, imageAlt: "작품 설명")
        let invalidBytes: [DocumentValue] = [.integer(0), .integer(8 * 1024 * 1024 + 1), .number(1.5), .string("1")]
        for bytes in invalidBytes {
            var draft = try imageDraft(bytes: bytes)
            XCTAssertTrue(draft.viewerEntranceImageChoices().isEmpty)
            let before = draft
            XCTAssertThrowsError(try draft.setViewerEntrance(settings))
            XCTAssertEqual(draft, before)
        }
        var draft = try imageDraft()
        var candidate = draft.candidate
        var artwork = draft.artworks[0]
        artwork["assets"] = .array([.object(["id": .string(imageID), "role": .string("image"), "mime": .string("image/jpeg"), "bytes": .integer(1)])])
        candidate["artworks"] = .array([artwork]); try draft.replaceCandidate(candidate)
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
        draft = try imageDraft()
        candidate = draft.candidate; candidate["artworks"] = .array(draft.artworks + draft.artworks)
        try draft.replaceCandidate(candidate)
        XCTAssertTrue(draft.viewerEntranceImageChoices().isEmpty)
        XCTAssertThrowsError(try draft.setViewerEntrance(settings))
    }

    func testMalformedOrFutureEntranceDoesNotSilentlyLoseFields() throws {
        let invalidValues: [DocumentValue] = [.null, .array([]), .object(["version": .integer(2)])]
        for value in invalidValues {
            var draft = try imageDraft(); var candidate = draft.candidate
            candidate["extensions"] = .object([ViewerEntranceSettings.namespace: value]); try draft.replaceCandidate(candidate)
            let before = draft
            XCTAssertThrowsError(try draft.viewerEntranceSettings())
            XCTAssertThrowsError(try draft.setViewerEntrance(ViewerEntranceSettings()))
            XCTAssertEqual(draft, before)
        }
        var draft = try imageDraft(); try draft.setViewerEntrance(ViewerEntranceSettings())
        let valid = draft
        let invalidFields: [(String, DocumentValue)] = [("artist", .integer(1)), ("mobilePosition", .string("left")), ("imageAssetId", .null)]
        for (field, value) in invalidFields {
            var candidate = valid.candidate
            candidate["extensions"][ViewerEntranceSettings.namespace][field] = value
            var malformed = valid; try malformed.replaceCandidate(candidate)
            let before = malformed
            XCTAssertThrowsError(try malformed.viewerEntranceSettings())
            XCTAssertThrowsError(try malformed.setViewerEntrance(ViewerEntranceSettings()))
            XCTAssertEqual(malformed, before)
        }
        var candidate = draft.candidate
        candidate["extensions"][ViewerEntranceSettings.namespace]["future"] = .bool(true)
        try draft.replaceCandidate(candidate)
        let before = draft
        XCTAssertThrowsError(try draft.setViewerEntrance(ViewerEntranceSettings(artist: "수정")))
        XCTAssertEqual(draft, before)
    }
}
