// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class SharedFurnishingCatalogTests: XCTestCase {
    func testExplicitLightCatalogSelectionBindsPreparedSourceAndCurrentNotice() throws {
        let light = "11111111-1111-4111-8111-111111111111"
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["lights"] = .array([.object(["id": .string(light)])])
        try draft.replaceCandidate(candidate)
        var form = try SharedFurnishingFormBuffer(draft: draft, host: .light, hostId: light)
        form.selected = true; form.catalogId = "museum-wallwasher"
        let command = try form.command()
        XCTAssertEqual(command.catalogId, "museum-wallwasher")
        XCTAssertEqual(command.value["catalogId"], .string("museum-wallwasher"))
        let asset = try SharedFurnishingCatalog.asset(catalogId: command.catalogId, id: light)
        XCTAssertEqual(asset["sha256"], .string("a4b66294c77e206a574905e12680148129f914d368add947f1c8831c64bf3f4d"))
        XCTAssertEqual(asset["provenance"]["sourceSha256"], .string("d85d9d1823c49bef412a23a59e238104f5b8ad342d8e8149efaf8d1110dc3290"))
        XCTAssertEqual(asset["provenance"]["recipe"], .string("omit-unused-tangent-v1"))
        XCTAssertEqual(asset["rights"]["notice"]["sha256"], .string("b40713d9c089649c71aeed2c8fced9d9274612ef376b77d19ab5f6becd5b0cf3"))
        XCTAssertThrowsError(try SharedFurnishingCommand(host: .bench, hostId: light, pose: command.pose, catalogId: "museum-wallwasher"))
        form.catalogId = "unknown"; XCTAssertThrowsError(try form.command())
    }
    func testRecessedDownlightUsesOriginalAndSourceAperturePose() throws {
        let light = "11111111-1111-4111-8111-111111111111"
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["lights"] = .array([.object(["id": .string(light)])])
        try draft.replaceCandidate(candidate)
        var form = try SharedFurnishingFormBuffer(draft: draft, host: .light, hostId: light)
        form.selected = true; form.catalogId = "recessed-downlight"; form.scale = "0.95"
        let command = try form.command(locale: Locale(identifier: "en_US"))
        let pose = try XCTUnwrap(command.pose)
        XCTAssertEqual(pose.position[0], 0); XCTAssertEqual(pose.position[1], 0.0076, accuracy: 1e-12)
        XCTAssertEqual(pose.position[2], 0); XCTAssertEqual(pose.yaw, 0)
        let descriptor = try SharedFurnishingCatalog.descriptor(command.catalogId)
        XCTAssertEqual(descriptor["functional"]["aim"], .vector([0, -1, 0]))
        let asset = try SharedFurnishingCatalog.asset(catalogId: command.catalogId, id: light)
        XCTAssertEqual(asset["sha256"], .string("041c6c2c340a039f1eb82f70d925c1ecbe913276deaba5b447e540d67fc2e7bf"))
        XCTAssertEqual(asset["bytes"], .integer(86480))
        XCTAssertEqual(asset["provenance"]["recipe"], .string("original-v1"))
        XCTAssertEqual(asset["rights"]["notice"]["sha256"], .string("b878f7873013dd45d32bfd7d8003dfb7f57d5680d69e3ed3f24d461692bf7b8e"))
        XCTAssertThrowsError(try SharedFurnishingCommand(host: .bench, hostId: light, pose: pose, catalogId: command.catalogId))
    }
    func testConstrainedHostScaleDerivesPoseAndRetainsInvalidRawInput() throws {
        let light = "11111111-1111-4111-8111-111111111111", opening = "22222222-2222-4222-8222-222222222222"
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["lights"] = .array([.object(["id": .string(light)])])
        candidate["openings"] = .array([.object(["id": .string(opening), "offset": .vector([-4, -0.9])])])
        try draft.replaceCandidate(candidate)
        var spot = try SharedFurnishingFormBuffer(draft: draft, host: .light, hostId: light)
        spot.selected = true; spot.scale = "0,95"
        let spotPose = try XCTUnwrap(spot.command(locale: Locale(identifier: "de_DE")).pose)
        XCTAssertEqual(spotPose.yaw, 0); XCTAssertEqual(spotPose.position[0], 0)
        XCTAssertEqual(spotPose.position[1], 0.1425, accuracy: 1e-12); XCTAssertEqual(spotPose.position[2], 0.071725, accuracy: 1e-12)
        var portal = try SharedFurnishingFormBuffer(draft: draft, host: .opening, hostId: opening)
        portal.selected = true; portal.scale = "0.95"
        let doorPose = try XCTUnwrap(portal.command(locale: Locale(identifier: "en_US")).pose)
        XCTAssertEqual(doorPose.yaw, 0); XCTAssertEqual(doorPose.position[0], -4); XCTAssertEqual(doorPose.position[1], -1.945, accuracy: 1e-12); XCTAssertEqual(doorPose.position[2], 0)
        portal.scale = "-"; XCTAssertThrowsError(try portal.command()); XCTAssertEqual(portal.scale, "-")
        portal.selected = false; XCTAssertNil(try portal.command().pose)
    }
    func testRuntimeCatalogAndClosedIdentityChecks() throws {
        let fixture = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Fixtures/shared-furnishing-catalog-fixture.json")
        var value = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: fixture))
        let rows = try SharedFurnishingCatalog.checkedCatalog(value)
        XCTAssertEqual(Set(rows.compactMap { $0["catalogId"].string }), ["backless-oak-bench", "sculpture-bench-refined", "museum-track-spot", "recessed-downlight", "museum-wallwasher", "museum-wallwasher-rear-relief", "gallery-open-portal", "rectangular-plinth"])
        value["items"] = .array(Array(rows.reversed())); XCTAssertEqual(try SharedFurnishingCatalog.checkedCatalog(value).count, 8)
        var changed = rows; changed[0]["rights"]["licenseId"] = .string("unknown")
        value["items"] = .array(changed); XCTAssertThrowsError(try SharedFurnishingCatalog.checkedCatalog(value))
        var duplicate = rows; duplicate[1] = rows[0]
        value["items"] = .array(duplicate); XCTAssertThrowsError(try SharedFurnishingCatalog.checkedCatalog(value))
        value["items"] = .array(rows); value["extra"] = .bool(true); XCTAssertThrowsError(try SharedFurnishingCatalog.checkedCatalog(value))
        let id = "33333333-3333-4333-8333-333333333333"
        let asset = try SharedFurnishingCatalog.asset(catalogId: "backless-oak-bench", id: id)
        XCTAssertEqual(asset["path"], .string("furnishings/\(id)/model.glb"))
        XCTAssertEqual(asset["rights"]["notice"]["path"], .string("furnishings/\(id)/LICENSE-NOTICE.txt"))
        XCTAssertEqual(asset["rights"]["notice"]["sha256"], .string("aea7aac82234e5f4158a901e84a753b79c75da20b72e859493801dffeaae3de5"))
        XCTAssertThrowsError(try SharedFurnishingCatalog.asset(catalogId: "chair", id: id))
        XCTAssertThrowsError(try SharedFurnishingCatalog.asset(catalogId: "backless-oak-bench", id: "bad"))
    }
    func testPedestalCorrespondenceRejectsUnrelatedMutationAndPreservesHostOnRemoval() throws {
        let host = "11111111-1111-4111-8111-111111111111", assetId = "33333333-3333-4333-8333-333333333333"
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["placements"] = .array([.object(["id": .string(host), "roomId": candidate["rooms"].items!.first!["id"], "transform": NativeExhibitionDraft.identity])])
        candidate["extensions"] = .object(["opaque": .string("keep")])
        try draft.replaceCandidate(candidate)
        let pose = try FurnishingPose(position: [1, 0, -1], yaw: 0, scale: 1)
        let command = try SharedFurnishingCommand(host: .pedestal, hostId: host, pose: pose)
        var after = candidate
        after["extensions"][FurnishingCatalog.namespace] = .object(["version": .integer(1), "assets": .array([try SharedFurnishingCatalog.asset(catalogId: command.catalogId, id: assetId)]), "bindings": .array([.object(["hostKind": .string("pedestal"), "hostId": .string(host), "assetId": .string(assetId), "pose": pose.value])])])
        after["extensions"][InstallationSettings.namespace] = .object(["version": .integer(1), "placements": .array([.object(["placementId": .string(host), "pedestal": .object(["kind": .string("box"), "position": .vector([1, 0, -1]), "yaw": .number(0), "dimensions": .object(["width": .number(0.5), "height": .number(0.9000000059604645), "depth": .number(0.5)]), "material": .object(["preset": .string("paint"), "color": .string("#deded7"), "roughness": .number(0.65), "metalness": .integer(0)])])])])])
        let preview = try SharedFurnishingPreview(candidate: after, base: draft, command: command)
        let applied = try preview.apply(to: draft, command: command)
        var forged = after; forged["title"] = .string("unexpected")
        XCTAssertThrowsError(try SharedFurnishingPreview(candidate: forged, base: draft, command: command))
        forged = after; forged["extensions"][FurnishingCatalog.namespace]["assets"] = .array([])
        XCTAssertThrowsError(try SharedFurnishingPreview(candidate: forged, base: draft, command: command))
        let remove = try SharedFurnishingCommand(host: .pedestal, hostId: host, pose: nil)
        var removed = after
        if case .object(var ext) = removed["extensions"] { ext.removeValue(forKey: FurnishingCatalog.namespace); removed["extensions"] = .object(ext) }
        XCTAssertNoThrow(try SharedFurnishingPreview(candidate: removed, base: applied, command: remove))
        XCTAssertEqual(removed["placements"], after["placements"])
        XCTAssertEqual(removed["extensions"][InstallationSettings.namespace], after["extensions"][InstallationSettings.namespace])
    }
    func testSharedFourHostVectorsWhenProvided() throws {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_FURNISHING_VECTORS"] else { throw XCTSkip("Requires retained shared-domain vectors") }
        let vectors = try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: URL(fileURLWithPath: path)))
        let candidate = vectors["candidate"]
        let document: DocumentValue = .object(["kind": .string("exhibition-draft"), "schemaVersion": .string("1.0.0-draft.1"), "id": .string(UUID().uuidString), "exhibitionId": candidate["id"], "editVersion": .integer(1), "candidate": candidate])
        let draft = try NativeExhibitionDraft(document: document), cases = try XCTUnwrap(vectors["cases"].items)
        XCTAssertEqual(cases.count, 8)
        for entry in cases {
            let raw = entry["command"], host = try XCTUnwrap(raw["hostKind"].string.flatMap(SharedFurnishingCommand.Host.init(rawValue:))), id = try XCTUnwrap(raw["hostId"].string)
            let pose: FurnishingPose? = raw["type"] == .string("set-furnishing-visual") ? try FurnishingPose(raw["pose"]) : nil
            let command = try SharedFurnishingCommand(host: host, hostId: id, pose: pose)
            XCTAssertEqual(command.value, raw)
            let preview = try SharedFurnishingPreview(candidate: entry["candidate"], base: draft, command: command)
            XCTAssertEqual(try preview.apply(to: draft, command: command).candidate, entry["candidate"])
            var forged = entry["candidate"]; forged["title"] = .string("unexpected")
            XCTAssertThrowsError(try SharedFurnishingPreview(candidate: forged, base: draft, command: command))
        }
    }
}
