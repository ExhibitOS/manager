// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class CoverEntryFormBufferTests: XCTestCase {
    func testCredentialRenewalRefusesOldTokenForeignIdentityAndNoncanonicalSecrets() throws {
        let tenant = UUID().uuidString, principal = UUID().uuidString
        let token: (UInt8) -> String = { byte in Data(repeating: byte, count: 32).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "") }
        func credentials(_ value: String, tenantId: String? = nil, principalId: String? = nil, origin: String = "http://127.0.0.1:13200") throws -> LocalAuthoringSession {
            try JSONDecoder().decode(LocalAuthoringSession.self, from: JSONSerialization.data(withJSONObject: ["tenantId": tenantId ?? tenant, "principalId": principalId ?? principal, "origin": origin, "token": value]))
        }
        let old = try credentials(token(1)), renewed = try credentials(token(2))
        XCTAssertTrue(renewed.isRenewal(of: old))
        XCTAssertFalse(old.isRenewal(of: old))
        XCTAssertFalse(try credentials(token(2), tenantId: UUID().uuidString).isRenewal(of: old))
        XCTAssertFalse(try credentials(token(2), principalId: UUID().uuidString).isRenewal(of: old))
        XCTAssertFalse(try credentials(token(2), origin: "http://127.0.0.1:13202").isRenewal(of: old))
        XCTAssertFalse(try credentials(String(repeating: "_", count: 43)).isRenewal(of: old))
        XCTAssertFalse(try credentials("expired").isRenewal(of: old))
    }
    private func previewRequest() throws -> ViewportEntrancePreviewRequest {
        let frame = ViewportFrame(epoch: UUID().uuidString, sequence: 2, tenantId: UUID().uuidString, principalId: UUID().uuidString,
                                  draft: try NativeExhibitionDraft.new(), selection: nil, mode: "translate", view: "isometric", disabled: true)
        return try ViewportEntrancePreviewRequest(frame: frame, action: .thumbnail, device: .mobile, scenario: .imageFailed, reducedMotion: true, theme: .poster, locale: "ko-KR")
    }
    private func previewReply(_ request: ViewportEntrancePreviewRequest) -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "device", "scenario", "reducedMotion", "theme", "locale"] { fields[key] = request.value[key] }
        fields["baseDraft"] = request.frame.draft.document
        fields["status"] = .string("ready"); fields["mode"] = .string("cover"); fields["renderGeneration"] = .integer(7)
        fields["imageStatus"] = .string("failed"); fields["viewport"] = .object(["width": .integer(360), "height": .integer(740)])
        fields["snapshotRect"] = .object(["x": .integer(0), "y": .integer(0), "width": .integer(360), "height": .integer(740)])
        return .object(fields)
    }
    func testEntrancePreviewRejectsStaleBindingDraftGenerationAndCandidate() throws {
        let request = try previewRequest(), response = previewReply(request)
        let checked = try request.checkedReply(response)
        XCTAssertNoThrow(try checked.recheck(response, request: request))
        for key in ["epoch", "tenantId", "principalId", "requestId"] {
            var stale = response; stale[key] = .string(UUID().uuidString)
            XCTAssertThrowsError(try request.checkedReply(stale), key)
        }
        var stale = response; stale["baseDraft"]["candidate"]["title"] = .string("다른 전시")
        XCTAssertThrowsError(try request.checkedReply(stale))
        stale = response; stale["renderGeneration"] = .integer(8)
        XCTAssertThrowsError(try checked.recheck(stale, request: request))
        stale = response; stale["candidate"] = request.frame.draft.candidate
        XCTAssertThrowsError(try request.checkedReply(stale))
    }
    func testEntrancePreviewSignalsAreReadOnlyAndRequireActualMobileViewport() throws {
        let request = try previewRequest(), response = previewReply(request)
        let original = request.frame.draft
        XCTAssertTrue(request.frame.disabled) // Read-only preview never grants geometry editing.
        XCTAssertEqual(request.value["scenario"], .string("image-failed"))
        XCTAssertEqual(request.value["reducedMotion"], .bool(true))
        XCTAssertEqual(try request.checkedReply(response).snapshotRect.width, 360)
        var desktopWrapper = response; desktopWrapper["viewport"]["width"] = .integer(1024)
        XCTAssertThrowsError(try request.checkedReply(desktopWrapper))
        var overflow = response; overflow["snapshotRect"]["x"] = .integer(1)
        XCTAssertThrowsError(try request.checkedReply(overflow))
        var wrongMode = response; wrongMode["mode"] = .string("walk")
        XCTAssertThrowsError(try request.checkedReply(wrongMode))
        XCTAssertEqual(request.frame.draft, original)
        XCTAssertEqual(request.frame.draft.candidate["extensions"][ViewerEntranceSettings.namespace], .null)
    }
    func testLateThumbnailContextRejectsChangedOptionsAndDraft() throws {
        let request = try previewRequest(), draft = request.frame.draft
        func matches(_ draft: NativeExhibitionDraft, device: EntrancePreviewDevice = .mobile, scenario: EntrancePreviewScenario = .imageFailed,
                     reduced: Bool = true, theme: CoverEntryTheme? = .poster, locale: String? = "ko-KR") -> Bool {
            request.matchesContext(draft: draft, device: device, scenario: scenario, reducedMotion: reduced, theme: theme, locale: locale)
        }
        XCTAssertTrue(matches(draft))
        XCTAssertFalse(matches(draft, device: .desktop))
        XCTAssertFalse(matches(draft, scenario: .slow))
        XCTAssertFalse(matches(draft, reduced: false))
        XCTAssertFalse(matches(draft, theme: nil))
        XCTAssertFalse(matches(draft, locale: "en-US"))
        XCTAssertFalse(matches(draft, locale: nil))
        XCTAssertFalse(matches(try NativeExhibitionDraft.new(title: "다른 전시")))
    }
    func testCloseRequiresEditorAckAndExactNullableThemeLocaleEcho() throws {
        let original = try previewRequest()
        let close = try ViewportEntrancePreviewRequest(frame: original.frame, action: .close, device: .mobile, scenario: .normal, reducedMotion: false)
        var ready = previewReply(close); ready["mode"] = .string("editor")
        XCTAssertEqual(ready["theme"], .null); XCTAssertEqual(ready["locale"], .null)
        XCTAssertNoThrow(try close.checkedReply(ready))
        var notClosed = ready; notClosed["mode"] = .string("cover")
        XCTAssertThrowsError(try close.checkedReply(notClosed))
        var resolvedInsteadOfEchoed = ready; resolvedInsteadOfEchoed["theme"] = .string("catalogue")
        XCTAssertThrowsError(try close.checkedReply(resolvedInsteadOfEchoed))
        var wrongLocale = ready; wrongLocale["locale"] = .string("ko-KR")
        XCTAssertThrowsError(try close.checkedReply(wrongLocale))
        if case .object(var missing) = ready {
            missing.removeValue(forKey: "locale")
            XCTAssertThrowsError(try close.checkedReply(.object(missing)))
        }
    }
    func testDeadHostRecoveryRequiresNewEpochOrdinaryDraftAcknowledgement() throws {
        let old = try previewRequest().frame, base = old.draft, newEpoch = UUID().uuidString
        func frame(epoch: String, draft: NativeExhibitionDraft, tenant: String? = nil, principal: String? = nil) -> ViewportFrame {
            ViewportFrame(epoch: epoch, sequence: 1, tenantId: tenant ?? old.tenantId, principalId: principal ?? old.principalId,
                          draft: draft, selection: old.selection, mode: old.mode, view: old.view, disabled: true, cameraSequence: old.cameraSequence)
        }
        func confirms(_ frame: ViewportFrame, owner: String?, sequence: Int = 1) -> Bool {
            frame.confirmsHostRecovery(replacing: old.epoch, ownerEpoch: owner, acceptedSequence: sequence,
                                       expectedDraft: base, tenantId: old.tenantId, principalId: old.principalId)
        }
        let ordinary = frame(epoch: newEpoch, draft: base)
        XCTAssertTrue(confirms(ordinary, owner: newEpoch))
        XCTAssertFalse(confirms(old, owner: newEpoch, sequence: old.sequence)) // Late old-host ACK.
        XCTAssertFalse(confirms(old, owner: old.epoch, sequence: old.sequence)) // Not recreated yet.
        XCTAssertFalse(confirms(ordinary, owner: old.epoch))
        XCTAssertFalse(confirms(ordinary, owner: newEpoch, sequence: 0))
        XCTAssertFalse(confirms(frame(epoch: newEpoch, draft: base, tenant: UUID().uuidString), owner: newEpoch))
        XCTAssertFalse(confirms(frame(epoch: newEpoch, draft: base, principal: UUID().uuidString), owner: newEpoch))
        var display = base
        var raw = try display.viewerEntranceSettings(); raw.introduction = "아직 적용하지 않은 소개"
        try display.setViewerEntrance(raw)
        XCTAssertFalse(confirms(frame(epoch: newEpoch, draft: display), owner: newEpoch))
        XCTAssertEqual(ordinary.draft, base)
        XCTAssertEqual(ordinary.view, old.view); XCTAssertEqual(ordinary.cameraSequence, old.cameraSequence)
        XCTAssertEqual(raw.introduction, "아직 적용하지 않은 소개")
    }
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

    func testResetThenRetypeOriginalNumberUsesVisibleValue() throws {
        var saved = CoverEntryDesign(theme: .poster)
        saved.set("layout", "margin", .number(12))
        saved.set("effects", "strength", .number(0.25))
        saved.set("image", "focus", .vector([0.25, 0.75]))
        var fields = CoverEntryFormBuffer(design: saved)
        let oldMargin = fields.numbers[.margin], oldStrength = fields.numbers[.strength]
        fields.resetLayoutAndEffects()
        XCTAssertEqual(try fields.value()?.raw["layout"], .null)
        fields.numbers[.margin] = oldMargin
        fields.numbers[.strength] = oldStrength
        let result = try XCTUnwrap(fields.value())
        XCTAssertEqual(result.raw["layout"]["margin"], .number(12))
        XCTAssertEqual(result.raw["effects"]["strength"], .number(0.25))
        XCTAssertEqual(result.raw["image"], saved.raw["image"])
        XCTAssertEqual(fields.numbers[.margin], oldMargin)
        // The same comparison rebase is required for explicit theme changes.
        fields.selectTheme(.invitation)
        fields.numbers[.margin] = oldMargin
        XCTAssertEqual(try fields.value()?.raw["layout"]["margin"], .number(12))
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
