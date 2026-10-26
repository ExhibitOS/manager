// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class FurnishingVisualsTests: XCTestCase {
    let host = "11111111-1111-4111-8111-111111111111"
    let other = "22222222-2222-4222-8222-222222222222"
    let asset = "33333333-3333-4333-8333-333333333333"
    let ns = FurnishingCatalog.namespace
    let template = "org.exhibitos.studio/template"
    // Correspondence fixtures only. Full geometry/asset admission is shared.
    func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), c = draft.candidate
        c["placements"] = .array([host, other].map { .object(["id": .string($0), "roomId": c["rooms"].items?.first?["id"] ?? .null, "transform": NativeExhibitionDraft.identity]) })
        c["extensions"] = .object([template: .object(["modified": .bool(false), "keep": .integer(9)]), "opaque": .string("keep"),
            InstallationSettings.namespace: .object(["version": .integer(1), "placements": .array([
                .object(["placementId": .string(host), "frame": .object(["kind": .string("none")])]),
                .object(["placementId": .string(other), "pedestal": .object(["kind": .string("none")])])])])])
        try draft.replaceCandidate(c); return draft
    }
    func command(_ id: String? = nil, yaw: Double = 0) throws -> FurnishingCommand {
        try .init(hostId: id ?? host, pose: FurnishingPose(position: [1, 0, -1], yaw: yaw, scale: 1.5))
    }
    func prepared(_ base: NativeExhibitionDraft, command: FurnishingCommand) throws -> NativeExhibitionDraft {
        let candidate = try command.expected(in: base, newAssetId: asset)
        return try FurnishingPreview(candidate: candidate, base: base, command: command).apply(to: base, command: command)
    }
    func frame(_ draft: NativeExhibitionDraft, selected: String? = nil, disabled: Bool = false) -> ViewportFrame {
        .init(epoch: UUID().uuidString.lowercased(), sequence: 5, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            selection: .init(kind: "placement", id: selected ?? host), mode: "translate", view: "front", disabled: disabled)
    }
    func reply(_ request: DocumentValue, candidate: DocumentValue) -> DocumentValue {
        var fields = [String: DocumentValue]()
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { fields[key] = request[key] }
        fields["candidate"] = candidate; return .object(fields)
    }
    func testOpeningAndRawIncompleteTextNeverMaterializeOrGuessPosition() throws {
        let base = try fixture(); var f = try FurnishingFormBuffer(draft: base, hostId: host)
        XCTAssertFalse(f.selected); XCTAssertEqual([f.x,f.y,f.z], ["","",""])
        XCTAssertNil(try f.command(hostId: host).pose)
        f.selected = true; f.y = "0"; f.z = "-1"
        for text in ["", "-", "NaN", "inf", "10001"] { f.x = text; XCTAssertThrowsError(try f.command(hostId: host)); XCTAssertEqual(f.x,text) }
        f.x = "1,25"; XCTAssertEqual(try f.command(hostId: host, locale: Locale(identifier: "de_DE")).pose?.position[0], 1.25)
        f.scale = "4.01"; XCTAssertThrowsError(try f.command(hostId: host, locale: Locale(identifier: "de_DE")))
        XCTAssertEqual(base.candidate["extensions"][ns], .null)
    }
    func testSetPreservesOriginalsFrameOtherBindingsAndUndoRestoresAbsence() throws {
        let base = try fixture(), command = try command(yaw: .pi/3), next = try prepared(base, command: command)
        let before = base.candidate, after = next.candidate
        for key in ["rooms","surfaces","placements","artworks","lights","mediaAssets"] { XCTAssertEqual(before[key],after[key]) }
        XCTAssertEqual(after["extensions"]["opaque"],before["extensions"]["opaque"])
        XCTAssertEqual(after["extensions"][template]["keep"], .integer(9))
        let rows = try XCTUnwrap(after["extensions"][InstallationSettings.namespace]["placements"].items)
        XCTAssertEqual(rows[0]["frame"],before["extensions"][InstallationSettings.namespace]["placements"].items?[0]["frame"])
        XCTAssertEqual(rows[1],before["extensions"][InstallationSettings.namespace]["placements"].items?[1])
        XCTAssertEqual(rows[0]["pedestal"]["dimensions"]["height"].double, FurnishingCatalog.max[1]*1.5)
        XCTAssertNotEqual(command.pose?.point(FurnishingCatalog.support)[0], command.pose?.position[0], "Measured offset must not become centered")
        let cold = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(next)); XCTAssertEqual(cold,next)
        var undone = next; try undone.replaceCandidate(base.candidate); XCTAssertEqual(undone.candidate,base.candidate)
    }
    func testOneAssetReusedRemovalKeepsFallbackAndLastRemovalDropsNamespace() throws {
        let base = try fixture(), first = try prepared(base, command: command()), second = try prepared(first, command: command(other))
        XCTAssertEqual(second.candidate["extensions"][ns]["assets"].items?.count,1)
        let remove = try FurnishingCommand(hostId: host, pose: nil), once = try prepared(second, command: remove)
        XCTAssertEqual(once.candidate["extensions"][ns]["bindings"].items?.count,1)
        XCTAssertEqual(once.candidate["extensions"][InstallationSettings.namespace], second.candidate["extensions"][InstallationSettings.namespace])
        let last = try prepared(once, command: FurnishingCommand(hostId: other, pose: nil))
        guard case .object(let extensions) = last.candidate["extensions"] else { return XCTFail() }
        XCTAssertNil(extensions[ns]); XCTAssertEqual(last.candidate["extensions"][InstallationSettings.namespace],second.candidate["extensions"][InstallationSettings.namespace])
        XCTAssertEqual(try remove.expected(in: base,newAssetId: nil),base.candidate,"Absent removal is exact no-op including template provenance")
    }
    func testMintedIdentityCannotAliasEntitiesAndExistingAssetCannotBeReplaced() throws {
        let base = try fixture(), command = try command()
        XCTAssertThrowsError(try command.expected(in: base,newAssetId: host))
        let next = try prepared(base,command:command)
        var bad = try command.expected(in: next,newAssetId: other)
        bad["extensions"][ns]["assets"] = .array([try FurnishingCatalog.asset(other)])
        XCTAssertThrowsError(try FurnishingPreview(candidate:bad,base:next,command:command))
    }
    func testReplyRejectsCrossRequestWrongSelectionRightsAndUnrelatedChanges() throws {
        let base = try fixture(), command = try command(), candidate = try command.expected(in: base,newAssetId:asset)
        let request = try FurnishingViewportRequest(frame:frame(base),command:command)
        XCTAssertNoThrow(try request.checkedReply(reply(request.value,candidate:candidate)))
        var wrong = reply(request.value,candidate:candidate); wrong["requestId"] = .string(UUID().uuidString)
        XCTAssertThrowsError(try request.checkedReply(wrong))
        wrong = reply(request.value,candidate:candidate); wrong["extra"] = .bool(true); XCTAssertThrowsError(try request.checkedReply(wrong))
        XCTAssertThrowsError(try FurnishingViewportRequest(frame:frame(base,selected:other),command:command))
        XCTAssertThrowsError(try FurnishingViewportRequest(frame:frame(base,disabled:true),command:command))
        for key in ["title","lights","artworks"] {
            var bad = candidate; bad[key] = .string("tampered")
            XCTAssertThrowsError(try request.checkedReply(reply(request.value,candidate:bad)))
        }
        var bad = candidate; var assets = try XCTUnwrap(bad["extensions"][ns]["assets"].items)
        assets[0]["rights"]["licenseId"] = .string("unknown"); bad["extensions"][ns]["assets"] = .array(assets)
        XCTAssertThrowsError(try request.checkedReply(reply(request.value,candidate:bad)))
        let preview = try FurnishingPreview(candidate:candidate,base:base,command:command)
        var changed = base; try changed.rename("changed"); XCTAssertThrowsError(try preview.apply(to:changed,command:command))
    }
    func testOnlySelectedFallbackCenterAllowsRoundingAndOtherInstallationCannotChange() throws {
        let base = try fixture(), command = try command(yaw:0.7)
        var candidate = try command.expected(in:base,newAssetId:asset)
        var rows = try XCTUnwrap(candidate["extensions"][InstallationSettings.namespace]["placements"].items)
        var p = try XCTUnwrap(rows[0]["pedestal"]["position"].items); p[0] = .number(try XCTUnwrap(p[0].double)+1e-12)
        rows[0]["pedestal"]["position"] = .array(p); candidate["extensions"][InstallationSettings.namespace]["placements"] = .array(rows)
        XCTAssertNoThrow(try FurnishingPreview(candidate:candidate,base:base,command:command))
        rows[1]["pedestal"] = .object(["kind":.string("box")]); candidate["extensions"][InstallationSettings.namespace]["placements"] = .array(rows)
        XCTAssertThrowsError(try FurnishingPreview(candidate:candidate,base:base,command:command))
    }
    func testCatalogAndCapabilitiesAreExactOptionalAndCannotAdmitChair() throws {
        XCTAssertTrue(try FurnishingCatalog.checkedCatalog(.object(["version":.integer(1),"items":.array([FurnishingCatalog.descriptor])])))
        var bad = FurnishingCatalog.descriptor; bad["id"] = .string("chair")
        XCTAssertThrowsError(try FurnishingCatalog.checkedCatalog(.object(["version":.integer(1),"items":.array([bad])])))
        XCTAssertFalse(try JSONDecoder().decode(ViewportAcceptance.self,from:Data("{\"accepted\":true}".utf8)).supportsFurnishingVisuals)
        XCTAssertTrue(try JSONDecoder().decode(ViewportAcceptance.self,from:Data("{\"accepted\":true,\"capabilities\":[\"furnishing-visuals-v1\"]}".utf8)).supportsFurnishingVisuals)
    }
    func testThumbnailUsesOwnSourceIdentityAndRejectsForgedDimensions() throws {
        let request = try FurnishingThumbnailRequest(frame:frame(fixture()))
        var png = Data([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,1,0,0,0,1,0]); png.append(Data(repeating:0,count:9))
        var response = request.value; response["width"] = .integer(256); response["height"] = .integer(256); response["pngBase64"] = .string(png.base64EncodedString())
        XCTAssertEqual(try request.pngData(response),png,"Header correspondence only; native ImageIO still decodes before display")
        var bad = response; bad["sha256"] = .string(String(repeating:"0",count:64)); XCTAssertThrowsError(try request.pngData(bad))
        bad = response; bad["artworkId"] = .string(host); XCTAssertThrowsError(try request.pngData(bad))
        bad = response; bad["width"] = .integer(128); XCTAssertThrowsError(try request.pngData(bad))
    }
}
