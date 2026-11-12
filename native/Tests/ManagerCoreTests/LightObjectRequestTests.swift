// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class LightObjectRequestTests: XCTestCase {
    private let source = "aaaaaaaa-0000-4000-8000-000000000001"
    private let other = "aaaaaaaa-0000-4000-8000-000000000002"
    private let clone = "aaaaaaaa-0000-4000-8000-000000000003"
    private let surface = "bbbbbbbb-0000-4000-8000-000000000001"
    private let asset = "cccccccc-0000-4000-8000-000000000001"
    private let details = "org.exhibitos.studio/light-details"
    private let attachments = "org.exhibitos.studio/light-attachments"
    private let visuals = "org.exhibitos.studio/furnishing-visuals"
    private let template = "org.exhibitos.studio/template"
    private var pose: DocumentValue { .object(["position": .vector([0, 0.15, 0.0755]), "yaw": .number(0), "scale": .number(1)]) }

    private func fixture(visual: Bool = true, shared: Bool = true) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let room = draft.rooms[0]["id"], h = sqrt(0.5)
        func light(_ id: String, _ x: Double) -> DocumentValue {
            .object(["id": .string(id), "roomId": room, "type": .string("spot"), "unit": .string("candela"),
                     "transform": .object(["position": .vector([x, 3, 0]), "rotation": .vector([0, h, h, 0]), "scale": .vector([1, 1, 1])]),
                     "color": .vector([1, 1, 1]), "intensity": .integer(100), "castsShadow": .bool(false), "beamAngle": .number(0.5)])
        }
        candidate["lights"] = .array([light(source, 0), light(other, 1)])
        candidate["surfaces"] = .array([.object(["id": .string(surface), "roomId": room, "type": .string("ceiling"),
            "dimensions": .object(["width": .number(12), "height": .number(8)]),
            "transform": .object(["position": .vector([0, 4, 0]), "rotation": .vector([h, 0, 0, h]), "scale": .vector([1, 1, 1])])])])
        func settings(_ id: String) -> DocumentValue {
            .object(["lightId": .string(id), "fixture": .string("spot-can"), "aim": .object(["kind": .string("rotation")]), "range": .number(3.3), "penumbra": .number(0.35)])
        }
        func attachment(_ id: String, _ x: Double) -> DocumentValue {
            .object(["lightId": .string(id), "surfaceId": .string(surface), "anchor": .vector([x, 0]), "gap": .number(1)])
        }
        candidate["extensions"] = .object([
            details: .object(["version": .integer(1), "lights": .array([settings(source), settings(other)])]),
            attachments: .object(["version": .integer(1), "lights": .array([attachment(source, 0), attachment(other, 1)])]),
            template: .object(["modified": .bool(false)]), "org.example/retained": .object(["content": .string("untouched")])
        ])
        if visual {
            let descriptor = try SharedFurnishingCatalog.asset(catalogId: "museum-track-spot", id: asset)
            func binding(_ id: String) -> DocumentValue {
                .object(["hostKind": .string("light"), "hostId": .string(id), "assetId": .string(asset), "pose": pose])
            }
            candidate["extensions"][visuals] = .object(["version": .integer(1), "assets": .array([descriptor]),
                                                        "bindings": .array(shared ? [binding(source), binding(other)] : [binding(source)])])
        }
        try draft.replaceCandidate(candidate); return draft
    }
    private func frame(_ draft: NativeExhibitionDraft, disabled: Bool = false) -> ViewportFrame {
        ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: UUID().uuidString.lowercased(),
            principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: disabled)
    }
    private func makeRequest(_ type: String, _ draft: NativeExhibitionDraft, fields: [String: DocumentValue] = [:]) throws -> LightObjectRequest {
        var command = fields; command["type"] = .string(type); command["lightId"] = .string(source)
        return try LightObjectRequest(command: .object(command), frame: frame(draft))
    }
    private func reply(_ request: LightObjectRequest, candidate: DocumentValue, lightId: String? = nil) -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { fields[key] = request.value[key] }
        fields["candidate"] = candidate; fields["lightId"] = .string(lightId ?? source); return .object(fields)
    }
    private func changed(_ candidate: DocumentValue) -> DocumentValue {
        var result = candidate; result["extensions"][template]["modified"] = .bool(true); return result
    }
    private func without(_ value: DocumentValue, _ key: String) -> DocumentValue {
        guard case .object(var fields) = value else { return value }; fields.removeValue(forKey: key); return .object(fields)
    }
    private func droppingExtension(_ candidate: DocumentValue, _ key: String) -> DocumentValue {
        var result = candidate; result["extensions"] = without(result["extensions"], key); return result
    }

    func testCloneRemapsOnlyOwnedBindingsAndSharesTheAsset() throws {
        let base = try fixture(), request = try makeRequest("duplicate-light-object", base)
        var expected = base.candidate
        var lights = try XCTUnwrap(expected["lights"].items), copy = lights[0]; copy["id"] = .string(clone); lights.append(copy); expected["lights"] = .array(lights)
        for namespace in [details, attachments] {
            var rows = try XCTUnwrap(expected["extensions"][namespace]["lights"].items), row = rows[0]
            row["lightId"] = .string(clone); rows.append(row); expected["extensions"][namespace]["lights"] = .array(rows)
        }
        var rows = try XCTUnwrap(expected["extensions"][visuals]["bindings"].items), row = rows[0]
        row["hostId"] = .string(clone); rows.append(row); expected["extensions"][visuals]["bindings"] = .array(rows)
        expected = changed(expected)
        let preview = try request.checkedReply(reply(request, candidate: expected, lightId: clone))
        XCTAssertEqual(preview.lightId, clone)
        XCTAssertEqual(preview.candidate["extensions"][visuals]["assets"], base.candidate["extensions"][visuals]["assets"])
        XCTAssertEqual(try preview.apply(to: base).candidate, expected)
        XCTAssertThrowsError(try preview.apply(to: preview.apply(to: base)))
        var unrelated = expected; unrelated["title"] = .string("not this mutation")
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: unrelated, lightId: clone)))
    }

    func testDeleteCleansSubordinatesButKeepsSharedAssetAndOrder() throws {
        let base = try fixture(), request = try makeRequest("remove-light-object", base)
        var expected = base.candidate
        expected["lights"] = .array([expected["lights"].items![1]])
        for namespace in [details, attachments] { expected["extensions"][namespace]["lights"] = .array([expected["extensions"][namespace]["lights"].items![1]]) }
        expected["extensions"][visuals]["bindings"] = .array([expected["extensions"][visuals]["bindings"].items![1]])
        expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
        var orphan = expected; orphan["extensions"][attachments] = base.candidate["extensions"][attachments]
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: orphan)))
    }

    func testCloneDoesNotInventAbsentSubordinateNamespaces() throws {
        var base = try fixture(visual: false)
        try base.replaceCandidate(droppingExtension(droppingExtension(base.candidate, details), attachments))
        let request = try makeRequest("duplicate-light-object", base)
        var expected = base.candidate, rows = expected["lights"].items!, copied = rows[0]
        copied["id"] = .string(clone); rows.append(copied); expected["lights"] = .array(rows); expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected, lightId: clone)).candidate, expected)
        XCTAssertEqual(expected["extensions"][details], .null)
        XCTAssertEqual(expected["extensions"][attachments], .null)
        XCTAssertEqual(expected["extensions"][visuals], .null)
    }

    func testLastRowsRemainEmptyWhileUnusedVisualNamespaceIsRemoved() throws {
        var base = try fixture(shared: false), candidate = base.candidate
        candidate["lights"] = .array([candidate["lights"].items![0]])
        for namespace in [details, attachments] { candidate["extensions"][namespace]["lights"] = .array([candidate["extensions"][namespace]["lights"].items![0]]) }
        try base.replaceCandidate(candidate)
        let request = try makeRequest("remove-light-object", base)
        var expected = base.candidate; expected["lights"] = .array([])
        for namespace in [details, attachments] { expected["extensions"][namespace]["lights"] = .array([]) }
        expected = changed(droppingExtension(expected, visuals))
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
        var droppedTooMuch = droppingExtension(expected, details)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: droppedTooMuch)))
        droppedTooMuch = expected; droppedTooMuch["extensions"][visuals] = base.candidate["extensions"][visuals]
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: droppedTooMuch)))
    }

    func testExplicitInvisibleClearsVisualAndOnlyChangesTheProceduralFixture() throws {
        let base = try fixture(), request = try makeRequest("set-light-fixture", base, fields: ["visual": .null])
        var expected = base.candidate
        var rows = try XCTUnwrap(expected["extensions"][details]["lights"].items); rows[0]["fixture"] = .string("none")
        expected["extensions"][details]["lights"] = .array(rows)
        expected["extensions"][visuals]["bindings"] = .array([expected["extensions"][visuals]["bindings"].items![1]])
        expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
        XCTAssertEqual(expected["lights"], base.candidate["lights"])
        var proceduralOnly = expected; proceduralOnly["extensions"][visuals] = base.candidate["extensions"][visuals]
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: proceduralOnly)))
    }

    func testExplicitInvisibleCreatesAnIntentAndThenExactNoOpPreservesEditVersion() throws {
        var base = try fixture(visual: false), candidate = base.candidate
        candidate = droppingExtension(candidate, details); try base.replaceCandidate(candidate)
        let first = try makeRequest("set-light-fixture", base, fields: ["visual": .null])
        var expected = base.candidate
        expected["extensions"][details] = .object(["version": .integer(1), "lights": .array([.object(["lightId": .string(source), "fixture": .string("none")])])])
        expected = changed(expected)
        let applied = try first.checkedReply(reply(first, candidate: expected)).apply(to: base)
        let again = try makeRequest("set-light-fixture", applied, fields: ["visual": .null])
        XCTAssertEqual(try again.checkedReply(reply(again, candidate: applied.candidate)).apply(to: applied), applied)
    }

    func testSharedCommandDoesNotInventPresentationIntensityOrAreaSizeLimits() throws {
        let base = try fixture(visual: false)
        var light = without(base.candidate["lights"].items![0], "id")
        light["type"] = .string("area"); light["unit"] = .string("lumen")
        light = without(light, "beamAngle")
        light["intensity"] = .number(2_000_000)
        light["dimensions"] = .object(["width": .number(0.001), "height": .number(101)])
        let settings: DocumentValue = .object(["fixture": .string("none")])
        XCTAssertNoThrow(try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings, "attachment": .null]))
        light["intensity"] = .number(-1)
        XCTAssertThrowsError(try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings, "attachment": .null]))
        light["intensity"] = .number(0); light["dimensions"]["width"] = .number(0)
        XCTAssertThrowsError(try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings, "attachment": .null]))
    }

    func testSetObjectPreservesMountAndVisualEvenWhenFixtureIsNone() throws {
        let base = try fixture()
        var light = without(base.candidate["lights"].items![0], "id"); light["intensity"] = .integer(21)
        var settings = without(base.candidate["extensions"][details]["lights"].items![0], "lightId"); settings["fixture"] = .string("none")
        let request = try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings])
        var expected = base.candidate, row = light; row["id"] = .string(source)
        var lights = expected["lights"].items!; lights[0] = row; expected["lights"] = .array(lights)
        var detailsRows = expected["extensions"][details]["lights"].items!; settings["lightId"] = .string(source); detailsRows[0] = settings
        expected["extensions"][details]["lights"] = .array(detailsRows); expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
        XCTAssertEqual(expected["extensions"][attachments], base.candidate["extensions"][attachments])
        XCTAssertEqual(expected["extensions"][visuals], base.candidate["extensions"][visuals])
        var hidden = droppingExtension(expected, visuals)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: hidden)))
        hidden = expected; hidden["extensions"][attachments]["lights"] = .array([])
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: hidden)))
    }

    func testOrdinaryEditPreservesOpaqueExtensionIdentityShapedReferences() throws {
        var base = try fixture(), candidate = base.candidate
        candidate["extensions"]["org.example/retained"]["id"] = .string(source)
        candidate["extensions"]["org.example/retained"]["revisionId"] = .string(other)
        try base.replaceCandidate(candidate)
        var light = without(base.candidate["lights"].items![0], "id"); light["intensity"] = .integer(21)
        let settings = without(base.candidate["extensions"][details]["lights"].items![0], "lightId")
        let request = try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings])
        var expected = base.candidate, row = light; row["id"] = .string(source)
        var lights = expected["lights"].items!; lights[0] = row; expected["lights"] = .array(lights); expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
        XCTAssertEqual(expected["extensions"]["org.example/retained"], base.candidate["extensions"]["org.example/retained"])
        XCTAssertThrowsError(try makeRequest("remove-light-object", base))
    }

    func testExplicitDetachAndMountReplacementDoNotReorderOtherRows() throws {
        let base = try fixture(), light = without(base.candidate["lights"].items![0], "id")
        let settings = without(base.candidate["extensions"][details]["lights"].items![0], "lightId")
        let detached = try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings, "attachment": .null])
        var expected = base.candidate
        expected["extensions"][attachments]["lights"] = .array([expected["extensions"][attachments]["lights"].items![1]])
        expected = changed(expected)
        XCTAssertEqual(try detached.checkedReply(reply(detached, candidate: expected)).candidate, expected)

        var moved = light; moved["transform"]["position"] = .vector([0, 3.5, 0])
        let mount: DocumentValue = .object(["surfaceId": .string(surface), "anchor": .vector([0, 0]), "gap": .number(0.5)])
        let request = try makeRequest("set-light-object", base, fields: ["light": moved, "settings": settings, "attachment": mount])
        expected = base.candidate
        var lights = expected["lights"].items!, row = moved; row["id"] = .string(source); lights[0] = row; expected["lights"] = .array(lights)
        var mounts = expected["extensions"][attachments]["lights"].items!, binding = mount; binding["lightId"] = .string(source); mounts[0] = binding
        expected["extensions"][attachments]["lights"] = .array(mounts); expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
    }

    func testDetachDoesNotInventAnAbsentAttachmentNamespace() throws {
        var base = try fixture()
        try base.replaceCandidate(droppingExtension(base.candidate, attachments))
        let request = try makeRequest("set-light-object", base, fields: [
            "light": without(base.candidate["lights"].items![0], "id"),
            "settings": without(base.candidate["extensions"][details]["lights"].items![0], "lightId"),
            "attachment": .null
        ])
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: base.candidate)).apply(to: base), base)
    }

    func testPurposeIsPreservedAndRoomBaseCloneIsRefused() throws {
        var base = try fixture(), candidate = base.candidate, rows = candidate["extensions"][details]["lights"].items!
        rows[0]["purpose"] = .string("room-base"); candidate["extensions"][details]["lights"] = .array(rows); try base.replaceCandidate(candidate)
        XCTAssertThrowsError(try makeRequest("duplicate-light-object", base))
        let light = without(base.candidate["lights"].items![0], "id")
        let settings = without(without(rows[0], "lightId"), "purpose")
        let request = try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings])
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: base.candidate)).candidate, base.candidate)
        var missing = base.candidate; rows[0] = without(rows[0], "purpose"); missing["extensions"][details]["lights"] = .array(rows)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: missing)))
    }

    func testKnownFixtureSelectionBindsExactServerMintedAssetOnly() throws {
        let base = try fixture(visual: false)
        let visual: DocumentValue = .object(["catalogId": .string("museum-track-spot"), "catalogVersion": .integer(1), "pose": pose])
        let request = try makeRequest("set-light-fixture", base, fields: ["visual": visual])
        let descriptor = try SharedFurnishingCatalog.asset(catalogId: "museum-track-spot", id: asset)
        var expected = base.candidate
        expected["extensions"][visuals] = .object(["version": .integer(1), "assets": .array([descriptor]),
            "bindings": .array([.object(["hostKind": .string("light"), "hostId": .string(source), "assetId": .string(asset), "pose": pose])])])
        expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
        var forged = expected, assets = [descriptor]; assets[0]["bytes"] = .integer(1); forged["extensions"][visuals]["assets"] = .array(assets)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: forged)))
        var unsupported = visual; unsupported["catalogId"] = .string("unreviewed-light-fixture")
        XCTAssertThrowsError(try self.makeRequest("set-light-fixture", base, fields: ["visual": unsupported]))
    }

    func testClosedShapeCorrelationFreshIdsAndOpaqueReferencesAreEnforced() throws {
        let base = try fixture(), request = try makeRequest("duplicate-light-object", base)
        XCTAssertThrowsError(try self.makeRequest("duplicate-light-object", base, fields: ["newLightId": .string(clone)]))
        XCTAssertThrowsError(try LightObjectRequest(command: request.command, frame: frame(base, disabled: true)))
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: base.candidate, lightId: other)))
        var bad = reply(request, candidate: base.candidate, lightId: clone); bad["epoch"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try request.checkedReply(bad))
        bad = reply(request, candidate: base.candidate, lightId: clone); bad["extra"] = .bool(true)
        XCTAssertThrowsError(try request.checkedReply(bad))
        var referenced = base, candidate = base.candidate
        candidate["extensions"]["org.example/retained"]["sourceLight"] = .string(source); try referenced.replaceCandidate(candidate)
        XCTAssertThrowsError(try self.makeRequest("remove-light-object", referenced))
        candidate = base.candidate; candidate["extensions"]["org.example/retained"]["reserved"] = .string(clone); try referenced.replaceCandidate(candidate)
        let reserved = try self.makeRequest("duplicate-light-object", referenced)
        XCTAssertThrowsError(try reserved.checkedReply(reply(reserved, candidate: referenced.candidate, lightId: clone)))
    }

    func testOpaqueReferenceBlocksGarbageCollectionOfTheLastVisualAsset() throws {
        var base = try fixture(shared: false), candidate = base.candidate
        candidate["extensions"]["org.example/retained"]["asset"] = .string(asset); try base.replaceCandidate(candidate)
        let request = try makeRequest("set-light-fixture", base, fields: ["visual": .null])
        var expected = droppingExtension(base.candidate, visuals), rows = expected["extensions"][details]["lights"].items!
        rows[0]["fixture"] = .string("none"); expected["extensions"][details]["lights"] = .array(rows); expected = changed(expected)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: expected)))
    }

    func testMountCommandReusesCeilingTransformAndPreservesSettings() throws {
        let base = try fixture(), viewportFrame = frame(base)
        let attachment: DocumentValue = .object(["surfaceId": .string(surface), "anchor": .vector([0.5, 0.25]), "gap": .number(0.2)])
        let command = try LightObjectRequest.mountCommand(lightId: source, attachment: attachment, frame: viewportFrame)
        XCTAssertEqual(command["type"], .string("set-light-object"))
        XCTAssertEqual(command["lightId"], .string(source))
        XCTAssertEqual(command["attachment"], attachment)
        XCTAssertEqual(command["settings"], without(base.candidate["extensions"][details]["lights"].items![0], "lightId"))
        XCTAssertEqual(command["light"]["id"], .null)
        let position = try XCTUnwrap(command["light"]["transform"]["position"].items)
        for (actual, expected) in zip(position, [0.5, 3.8, 0.25]) { XCTAssertEqual(try XCTUnwrap(actual.double), expected, accuracy: 1e-9) }
        let h = sqrt(0.5)
        XCTAssertEqual(command["light"]["transform"]["rotation"], .vector([0, h, h, 0]))
        var unchanged = command["light"]
        unchanged["transform"] = base.candidate["lights"].items![0]["transform"]
        XCTAssertEqual(unchanged, without(base.candidate["lights"].items![0], "id"))
        XCTAssertNoThrow(try LightObjectRequest(command: command, frame: viewportFrame))
    }

    func testDetachCommandPreservesOriginalPoseAndSettings() throws {
        let base = try fixture(), viewportFrame = frame(base)
        let command = try LightObjectRequest.mountCommand(lightId: source, attachment: .null, frame: viewportFrame)
        guard case .object(let fields) = command else { return XCTFail("Expected closed coupled command") }
        XCTAssertEqual(Set(fields.keys), ["type", "lightId", "light", "settings", "attachment"])
        XCTAssertEqual(command["attachment"], .null)
        XCTAssertEqual(command["light"], without(base.candidate["lights"].items![0], "id"))
        XCTAssertEqual(command["settings"], without(base.candidate["extensions"][details]["lights"].items![0], "lightId"))
        XCTAssertNoThrow(try LightObjectRequest(command: command, frame: viewportFrame))
    }

    func testMountConvenienceRejectsUnsupportedAttachmentShapesAndScope() throws {
        let base = try fixture()
        let attachment: DocumentValue = .object(["surfaceId": .string(surface), "anchor": .vector([0, 0]), "gap": .number(0.2)])
        for value in [DocumentValue.string("ceiling"), .array([]), .bool(false)] {
            XCTAssertThrowsError(try LightObjectRequest.mountCommand(lightId: source, attachment: value, frame: frame(base)))
        }
        var extra = attachment; extra["lightId"] = .string(source)
        XCTAssertThrowsError(try LightObjectRequest.mountCommand(lightId: source, attachment: extra, frame: frame(base)))
        var gap = attachment; gap["gap"] = .number(0)
        XCTAssertThrowsError(try LightObjectRequest.mountCommand(lightId: source, attachment: gap, frame: frame(base)))
        XCTAssertThrowsError(try LightObjectRequest.mountCommand(lightId: source, attachment: .null, frame: frame(base, disabled: true)))
        XCTAssertThrowsError(try LightObjectRequest.mountCommand(lightId: clone, attachment: attachment, frame: frame(base)))
    }
}
