// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceFurnishingIntakeTests: XCTestCase {
    private let intakeId = "10000000-0000-4000-8000-000000000001"
    private let requestId = "10000000-0000-4000-8000-000000000002"
    private let furnishingId = "10000000-0000-4000-8000-000000000003"
    private let placementId = "10000000-0000-4000-8000-000000000004"
    private let modelSHA256 = String(repeating: "a", count: 64)

    private func fixture() throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: Data("""
            {
              "intakeId":"\(intakeId)","requestId":"\(requestId)","phase":"registered","retryable":false,"error":null,
              "received":{"bytes":23,"sha256":"\(modelSHA256)"},
              "declaration":{
                "kind":"pedestal","name":"Fixture","provenance":"Synthetic",
                "rights":{"holder":"Fixture","ownership":"owner","licenseId":"CC0-1.0","creditLine":"","permissions":{"display":true,"download":false,"export":false,"commercial":false}},
                "model":{"mime":"model/gltf-binary","bytes":23,"sha256":"\(modelSHA256)"}
              }
            }
            """.utf8))
    }

    private func registered() throws -> DocumentValue {
        var fixture = try fixture()
        var furnishing = fixture["declaration"]
        furnishing["id"] = .string(furnishingId)
        furnishing["version"] = .integer(1)
        fixture["furnishing"] = furnishing
        return fixture
    }

    private func intake(_ value: DocumentValue) throws -> WorkspaceFurnishingIntake {
        try JSONDecoder().decode(WorkspaceFurnishingIntake.self, from: JSONEncoder().encode(value))
    }

    func testCreateAndRegisteredDescriptorUseFlatWireFields() throws {
        let wire = try registered()
        let value = try intake(wire)
        try value.check(id: intakeId, requestId: requestId)
        let request = WorkspaceFurnishingRequest(requestId: requestId, details: value.declaration)
        var expected = wire["declaration"]
        expected["requestId"] = .string(requestId)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)), expected)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(value.furnishing)), wire["furnishing"])
    }

    func testModularFitProfileAndRegisteredReceiptAreSeparate() throws {
        var wire = try registered()
        wire["declaration"]["kind"] = .string("portal-finish")
        wire["furnishing"]["kind"] = .string("portal-finish")
        wire["fitProfile"] = .string("modular-portal-v1")
        wire["furnishing"]["fit"] = .object([
            "version": .integer(1), "sourceBytes": .integer(23), "sourceSha256": .string(modelSHA256),
            "geometry": .object(["profile": .string("modular-portal-v1"), "openingWidth": .number(2.4)])
        ])
        let value = try intake(wire)
        try value.check(requestId: requestId, details: value.declaration, expectedFit: .modularPortal,
                        supportedExtensionVersion: 7)
        XCTAssertThrowsError(try value.check(supportedExtensionVersion: 6))
        let request = WorkspaceFurnishingRequest(requestId: requestId, details: value.declaration, fitProfile: .modularPortal)
        var expected = wire["declaration"]
        expected["requestId"] = .string(requestId)
        expected["fitProfile"] = wire["fitProfile"]
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)), expected)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(value.furnishing)), wire["furnishing"])
        wire["furnishing"]["fit"]["sourceBytes"] = .integer(24)
        XCTAssertThrowsError(try intake(wire))
    }

    private func rectangularPortal() throws -> DocumentValue {
        var wire = try registered()
        wire["declaration"]["kind"] = .string("portal-finish")
        wire["furnishing"]["kind"] = .string("portal-finish")
        wire["fitProfile"] = .string("rectangular-portal-v1")
        wire["furnishing"]["fit"] = .object([
            "version": .integer(1), "sourceBytes": .integer(23), "sourceSha256": .string(modelSHA256),
            "geometry": .object(["profile": .string("rectangular-portal-v1"),
                "openingWidth": .number(2.4), "openingHeight": .number(3),
                "minZ": .number(-0.0225), "maxZ": .number(0.0225)])
        ])
        return wire
    }

    func testRectangularPortalIntakeRequiresDistinctAdmissionAndBoundContinuation() throws {
        let wire = try rectangularPortal(), value = try intake(wire)
        XCTAssertThrowsError(try value.check(supportedExtensionVersion: 7))
        try value.check(requestId: requestId, details: value.declaration, expectedFit: .rectangularPortal,
                        supportedExtensionVersion: 7, admitsRectangularFit: true)
        XCTAssertThrowsError(try value.check(requestId: intakeId, supportedExtensionVersion: 7, admitsRectangularFit: true))
        XCTAssertThrowsError(try value.check(details: value.declaration, expectedFit: .modularPortal,
                                             supportedExtensionVersion: 7, admitsRectangularFit: true))
        let request = WorkspaceFurnishingRequest(requestId: requestId, details: value.declaration, fitProfile: .rectangularPortal)
        let encoded = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request))
        XCTAssertEqual(encoded["fitProfile"], .string("rectangular-portal-v1"))
        XCTAssertEqual(encoded["model"], wire["declaration"]["model"])
        var resumed = wire
        resumed["phase"] = .string("failed"); resumed["furnishing"] = .null; resumed["retryable"] = .bool(true)
        try intake(resumed).check(id: value.intakeId, requestId: value.requestId, details: value.declaration,
                                  expectedFit: .rectangularPortal, supportedExtensionVersion: 7, admitsRectangularFit: true)
        for key in ["sourceSha256", "sourceBytes"] {
            var changed = wire; changed["furnishing"]["fit"][key] = key == "sourceBytes" ? .integer(24) : .string(String(repeating: "b", count: 64))
            XCTAssertThrowsError(try intake(changed))
        }
        var changed = wire; changed["furnishing"]["fit"]["geometry"]["extra"] = .bool(true)
        XCTAssertThrowsError(try intake(changed))
        changed = wire; changed["furnishing"]["fit"]["geometry"]["maxZ"] = .number(-0.0225)
        XCTAssertThrowsError(try intake(changed))
    }

    func testRectangularPortalSourceBoundsMatchSpec29Exactly() throws {
        let ranges: [(String, Double, Double)] = [("openingWidth", 0.9, 3), ("openingHeight", 2, 3.2),
            ("minZ", -0.15, -0.015), ("maxZ", 0.015, 0.15)]
        for (field, minimum, maximum) in ranges {
            for boundary in [minimum, maximum] {
                var wire = try rectangularPortal(); wire["furnishing"]["fit"]["geometry"][field] = .number(boundary)
                XCTAssertNoThrow(try intake(wire))
            }
            for outside in [minimum - 0.00001, maximum + 0.00001] {
                var wire = try rectangularPortal(); wire["furnishing"]["fit"]["geometry"][field] = .number(outside)
                XCTAssertThrowsError(try intake(wire))
            }
        }
    }

    func testRectangularPortalAttachNeedsOwnCapabilityAndPreservesOnePairedHost() throws {
        let portal = try XCTUnwrap(intake(rectangularPortal()).furnishing)
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        let host = "10000000-0000-4000-8000-00000000000a"
        let mate = "10000000-0000-4000-8000-00000000000b"
        let doors: DocumentValue = .array([.object(["id": .string(host), "type": .string("door")]),
                                          .object(["id": .string(mate), "type": .string("door")])])
        candidate["extensions"] = .object(["example": .bool(true)])
        candidate["openings"] = doors; try draft.replaceCandidate(candidate)
        XCTAssertThrowsError(try WorkspacePortalFinish.applying(to: draft, openingId: host, furnishing: portal))
        XCTAssertThrowsError(try WorkspacePortalFinish.applying(to: draft, openingId: host, furnishing: portal, admitsModularFit: true))
        let attached = try XCTUnwrap(WorkspacePortalFinish.applying(to: draft, openingId: host,
            furnishing: portal, admitsRectangularFit: true))
        let namespace = WorkspaceFurnishingPlacement.namespace
        XCTAssertEqual(attached.candidate["openings"], doors)
        XCTAssertEqual(attached.candidate["extensions"][namespace]["version"], .integer(7))
        XCTAssertEqual(attached.candidate["extensions"][namespace]["openingAttachments"].items?.count, 1)
        XCTAssertEqual(attached.candidate["extensions"][namespace]["openingAttachments"].items?.first?["openingId"], .string(host))
        XCTAssertNil(try WorkspacePortalFinish.applying(to: attached, openingId: host, furnishing: portal, admitsRectangularFit: true))
        let detached = try XCTUnwrap(WorkspacePortalFinish.applying(to: attached, openingId: host,
            furnishing: nil, admitsRectangularFit: true))
        XCTAssertEqual(detached.candidate, draft.candidate)
    }

    func testRectangularPortalAdvertisementCannotBorrowModularOrMissingParents() throws {
        var session = AuthoringSession(userId: intakeId, tenantId: requestId, role: "owner", csrfToken: "fixture", expiresAt: "fixture")
        session.workspacePortalRectangularFitProfile = "rectangular-portal-v1"
        session.workspaceFurnishingReusableFitProfile = "source-bound-fit-v1"
        XCTAssertFalse(session.advertisesWorkspaceRectangularPortalFit)
        session.workspaceFurnishingProfile = "workspace-furnishing-intake-v1"
        session.workspaceFurnishingStoolProfile = "workspace-furnishing-stool-v1"
        session.workspaceFurnishingRoomVisualProfile = "workspace-furnishing-room-visual-v1"
        session.workspaceArtworkFrameProfile = "workspace-artwork-frame-v1"
        session.workspaceArtworkFrameModelsProfile = "workspace-artwork-frame-models-v1"
        session.workspacePortalFinishProfile = "workspace-portal-finish-v1"
        XCTAssertTrue(session.advertisesWorkspaceRectangularPortalFit)
        session.workspacePortalRectangularFitProfile = nil
        session.workspacePortalModularFitProfile = "modular-portal-v1"
        XCTAssertFalse(session.advertisesWorkspaceRectangularPortalFit)
        session.workspacePortalRectangularFitProfile = "rectangular-portal-v2"
        XCTAssertFalse(session.advertisesWorkspaceRectangularPortalFit)
        session.workspacePortalRectangularFitProfile = "rectangular-portal-v1"
        session.workspaceFurnishingReusableFitProfile = nil
        XCTAssertFalse(session.advertisesWorkspaceRectangularPortalFit)
    }

    func testResumeAndTerminalReceiptsBindOriginalBytesAndRequest() throws {
        var wire = try registered()
        let issued = try intake(wire)
        XCTAssertThrowsError(try issued.check(requestId: intakeId))
        wire["received"]["sha256"] = .string(String(repeating: "b", count: 64))
        XCTAssertThrowsError(try intake(wire).check())
        wire = try registered()
        wire["furnishing"]["model"]["bytes"] = .integer(24)
        XCTAssertThrowsError(try intake(wire).check())
        wire = try fixture()
        wire["phase"] = .string("failed")
        wire["retryable"] = .bool(true)
        try intake(wire).check()
        wire["received"] = .null
        XCTAssertThrowsError(try intake(wire).check())
        wire["phase"] = .string("uploading")
        wire["retryable"] = .bool(false)
        try intake(wire).check()
    }

    func testRegisteredFrameLineageRequiresSameOwnerHistoryAndDrainedReceipt() throws {
        let source: DocumentValue = .object([
            "mime": .string("model/gltf-binary"), "bytes": .integer(101_768),
            "sha256": .string("17c571accc4ed2554731d4ce89f3cad4faa66cf2bca83321ecd92ad2537093d9")
        ])
        let renderable: DocumentValue = .object([
            "mime": .string("model/gltf-binary"), "bytes": .integer(101_720),
            "sha256": .string("2fb941a9f6ed030638512e917e348a0a3000e29c1133fe3f9670268ff1c4169c")
        ])
        var registeredIntake = try registered()
        for key in ["declaration", "furnishing"] {
            registeredIntake[key]["kind"] = .string("artwork-frame")
            registeredIntake[key]["model"] = renderable
        }
        registeredIntake["received"] = .object(["bytes": renderable["bytes"], "sha256": renderable["sha256"]])
        let history = try intake(registeredIntake)
        try history.check(supportedExtensionVersion: 5)
        let recipe = DocumentValue.string("omit-unused-invalid-tangent-r1")
        var wire: DocumentValue = .object([
            "version": .integer(1), "kind": .string(WorkspaceRegisteredFrameLineage.profile),
            "workspaceId": .string(placementId), "tenantId": .string(requestId), "principalId": .string(furnishingId),
            "intakeId": .string(intakeId), "requestId": .string(requestId), "furnishingId": .string(furnishingId),
            "furnishingVersion": .integer(1), "recipe": recipe, "phase": .string("ready-to-prepare"),
            "sourceModel": source, "renderableModel": renderable,
            "historicalRequestSHA256": .string(modelSHA256), "descriptorSHA256": .string(modelSHA256)
        ])
        func validate(_ wire: DocumentValue, previous: WorkspaceRegisteredFrameLineage? = nil) throws -> WorkspaceRegisteredFrameLineage {
            let lineage = try JSONDecoder().decode(WorkspaceRegisteredFrameLineage.self, from: JSONEncoder().encode(wire))
            try lineage.check(history: history, workspace: placementId, tenant: requestId, principal: furnishingId, previous: previous)
            return lineage
        }
        let staged = try validate(wire)
        wire["phase"] = .string("registered")
        XCTAssertThrowsError(try validate(wire))
        wire["receipt"] = .object(["workspaceId": .string(placementId), "intakeId": .string(intakeId), "recipe": recipe,
                                   "sourceSHA256": source["sha256"], "renderableSHA256": renderable["sha256"]])
        wire["workerDrained"] = .bool(false)
        XCTAssertThrowsError(try validate(wire))
        wire["workerDrained"] = .bool(true)
        XCTAssertEqual(try validate(wire, previous: staged).phase, .registered)
        var changed = wire
        changed["workspaceId"] = .string(intakeId)
        XCTAssertThrowsError(try validate(changed))
        changed = wire
        changed["descriptorSHA256"] = .string(String(repeating: "b", count: 64))
        XCTAssertThrowsError(try validate(changed, previous: staged))
    }

    func testOptInPreparationBindsOriginalAndRegisteredRenderable() throws {
        let policy = WorkspaceArtworkFramePreparationPolicy.omitUnusedInvalidTangent
        let source: DocumentValue = .object(["mime": .string("model/gltf-binary"), "bytes": .integer(101_768),
            "sha256": .string("17c571accc4ed2554731d4ce89f3cad4faa66cf2bca83321ecd92ad2537093d9")])
        let renderable: DocumentValue = .object(["mime": .string("model/gltf-binary"), "bytes": .integer(101_720),
            "sha256": .string("2fb941a9f6ed030638512e917e348a0a3000e29c1133fe3f9670268ff1c4169c")])
        var wire = try registered()
        wire["preparationPolicy"] = .string(policy.rawValue)
        wire["preparation"] = .object(["sourceModel": source, "recipe": .string(policy.rawValue), "renderableModel": renderable])
        wire["declaration"]["kind"] = .string("artwork-frame")
        wire["declaration"]["model"] = source
        wire["received"] = .object(["bytes": source["bytes"], "sha256": source["sha256"]])
        wire["furnishing"]["kind"] = .string("artwork-frame")
        wire["furnishing"]["model"] = renderable
        let value = try intake(wire)
        XCTAssertEqual(WorkspaceArtworkFramePreparationPolicy.matching(value.declaration.model), policy)
        XCTAssertThrowsError(try value.check(supportedExtensionVersion: 5))
        try value.check(details: value.declaration, expectedPolicy: policy, supportedExtensionVersion: 5, admitsPreparation: true)
        XCTAssertThrowsError(try value.check(details: value.declaration, supportedExtensionVersion: 5, admitsPreparation: true))
        let request = WorkspaceFurnishingRequest(requestId: requestId, details: value.declaration, preparationPolicy: policy)
        let encoded = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request))
        XCTAssertEqual(encoded["preparationPolicy"], .string(policy.rawValue))
        XCTAssertEqual(encoded["model"], source)
        let response = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(value))
        XCTAssertEqual(response["preparation"], wire["preparation"])

        var changed = wire
        changed["preparation"]["renderableModel"] = source
        XCTAssertThrowsError(try intake(changed).check(supportedExtensionVersion: 5, admitsPreparation: true))
        changed = wire
        changed["preparation"]["sourceModel"]["extra"] = .bool(true)
        XCTAssertThrowsError(try intake(changed))
        changed = wire
        changed["furnishing"]["provenance"] = .string("Substituted")
        XCTAssertThrowsError(try intake(changed).check(supportedExtensionVersion: 5, admitsPreparation: true))
        changed = wire
        changed["preparation"] = .null
        XCTAssertThrowsError(try intake(changed))
        changed = wire
        changed["phase"] = .string("validating")
        changed["furnishing"] = .null
        XCTAssertThrowsError(try intake(changed).check(supportedExtensionVersion: 5, admitsPreparation: true))
        if case .object(var fields) = changed {
            fields.removeValue(forKey: "preparation")
            try intake(.object(fields)).check(supportedExtensionVersion: 5, admitsPreparation: true)
        }
    }

    func testInfoStandPreparationBindsProducerAndAllowsCompatibleOriginal() throws {
        let policy = WorkspaceFurnishingPreparationPolicy.normalizeLegacySignTexture
        let manifest = String(repeating: "c", count: 64)
        var compatible = try registered()
        compatible["preparationPolicy"] = .string(policy.rawValue)
        compatible["declaration"]["kind"] = .string("info-stand")
        compatible["declaration"]["rights"]["holder"] = .string("Independent owner")
        compatible["declaration"]["rights"]["licenseId"] = .string("CC-BY-4.0")
        compatible["declaration"]["model"]["bytes"] = .integer(128)
        compatible["received"]["bytes"] = .integer(128)
        let source = compatible["declaration"]["model"]
        var descriptor = compatible["declaration"]
        descriptor["id"] = .string(furnishingId)
        descriptor["version"] = .integer(1)
        compatible["furnishing"] = descriptor
        try intake(compatible).check(supportedExtensionVersion: 3, infoStandPreparationManifest: manifest)
        XCTAssertThrowsError(try intake(compatible).check(supportedExtensionVersion: 3))

        var prepared = compatible
        let renderable: DocumentValue = .object([
            "mime": .string("model/gltf-binary"), "bytes": .integer(112),
            "sha256": .string(String(repeating: "b", count: 64)),
        ])
        prepared["furnishing"]["model"] = renderable
        XCTAssertThrowsError(try intake(prepared).check(supportedExtensionVersion: 3, infoStandPreparationManifest: manifest))
        prepared["preparation"] = .object([
            "recipe": .string(policy.rawValue), "sourceModel": source, "renderableModel": renderable,
            "runtimeManifestSHA256": .string(manifest),
        ])
        if case .object(var pendingFields) = prepared {
            pendingFields["phase"] = .string("validating")
            pendingFields["furnishing"] = .null
            pendingFields.removeValue(forKey: "preparation")
            try intake(.object(pendingFields)).check(supportedExtensionVersion: 3,
                infoStandPreparationManifest: manifest, requireCurrentInfoStandProducer: true)
        } else {
            XCTFail("Expected an object fixture")
        }
        let value = try intake(prepared)
        try value.check(details: value.declaration, expectedPolicy: policy,
                        supportedExtensionVersion: 3, infoStandPreparationManifest: manifest, requireCurrentInfoStandProducer: true)
        // An authenticated newer runtime can read a registered receipt from an older
        // producer. A selected operation still checks the expected producer exactly.
        try value.check(supportedExtensionVersion: 3, infoStandPreparationManifest: modelSHA256)
        XCTAssertThrowsError(try value.check(supportedExtensionVersion: 3, infoStandPreparationManifest: modelSHA256,
                                              requireCurrentInfoStandProducer: true))
        let registeredModel = try XCTUnwrap(value.furnishing)
        try value.checkInfoStandApproval(registeredModel, currentProducer: manifest)
        XCTAssertThrowsError(try value.checkInfoStandApproval(registeredModel, currentProducer: modelSHA256))
        XCTAssertThrowsError(try value.checkInfoStandApproval(XCTUnwrap(intake(compatible).furnishing), currentProducer: manifest))
        let encoded = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(value))
        XCTAssertEqual(encoded["preparation"], prepared["preparation"])

        prepared["preparation"]["sourceModel"] = renderable
        XCTAssertThrowsError(try intake(prepared).check(supportedExtensionVersion: 3, infoStandPreparationManifest: manifest))
    }

    func testPortalFinishRoundTripPreservesV5FurnishingsAndDoor() throws {
        var wire = try registered()
        wire["furnishing"]["kind"] = .string("portal-finish")
        wire["furnishing"]["model"]["sha256"] = .string(WorkspacePortalFinish.modelSHA256)
        wire["furnishing"]["model"]["bytes"] = .integer(164_044)
        let portal = try XCTUnwrap(intake(wire).furnishing)
        var pedestal = try registered()["furnishing"]
        pedestal["id"] = .string(intakeId)
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        let host = "10000000-0000-4000-8000-00000000000A"
        let door: DocumentValue = .object(["id": .string(host), "type": .string("door")])
        candidate["openings"] = .array([door])
        let namespace = WorkspaceFurnishingPlacement.namespace
        candidate["extensions"] = .object(["example": .bool(true), namespace: .object([
            "version": .integer(5), "furnishings": .array([pedestal]),
            "placements": .array([]), "frameAttachments": .array([])
        ])])
        try draft.replaceCandidate(candidate)

        let attached = try XCTUnwrap(WorkspacePortalFinish.applying(to: draft, openingId: host, furnishing: portal))
        XCTAssertEqual(attached.candidate["openings"], .array([door]))
        XCTAssertEqual(attached.candidate["extensions"][namespace]["version"], .integer(6))
        XCTAssertEqual(attached.candidate["extensions"][namespace]["openingAttachments"].items?.first?["openingId"], .string(host.lowercased()))
        XCTAssertTrue(WorkspacePortalFinish.hasAttachment(openingId: host, in: attached))
        XCTAssertNil(try WorkspacePortalFinish.applying(to: attached, openingId: host, furnishing: portal))
        XCTAssertThrowsError(try WorkspacePortalFinish.applying(to: attached, openingId: requestId, furnishing: portal))
        let detached = try XCTUnwrap(WorkspacePortalFinish.applying(to: attached, openingId: host, furnishing: nil))
        XCTAssertEqual(detached.candidate, draft.candidate)
        XCTAssertNil(try WorkspacePortalFinish.applying(to: detached, openingId: host, furnishing: nil))
    }

    func testWorkspaceBenchSeatPreservesRegisteredPlacementAndReplaysOnlyExactViewpoint() throws {
        var fixture = try registered()
        fixture["furnishing"]["kind"] = .string("bench")
        let furnishing = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(fixture["furnishing"]))
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0.125, scale: 1)
        let placed = try XCTUnwrap(placement.applying(furnishing, to: draft))
        let benchId = UUID().uuidString.lowercased()
        let cameraId = UUID().uuidString.lowercased()
        let view = try BenchViewpoint(id: cameraId, name: "Seat", roomId: room, position: [-1, 1.16, 0], target: [-1, 1.45, -2], fov: 50)
        func attachment(id: String = benchId, sha: String = modelSHA256, viewpoint: BenchViewpoint = view) throws -> WorkspaceBenchSeatAttachment {
            try .init(benchId: id, placementId: placementId, furnishingId: furnishing.id, furnishingVersion: 1, modelSha256: sha,
                      width: 1.5, depth: 0.48, height: 0.46, viewpointId: cameraId, viewpoint: viewpoint)
        }
        let command = try attachment()
        let next = try XCTUnwrap(command.applying(furnishing, to: placed))
        let seat = try XCTUnwrap(BenchSettings.list(in: next).first)
        XCTAssertEqual(seat.workspaceVisual, command.visual)
        XCTAssertEqual(seat.position, placement.position)
        XCTAssertEqual(seat.yaw, placement.yaw)
        XCTAssertEqual(try BenchFormBuffer(draft: next, benchId: benchId).settings(), seat)
        XCTAssertEqual(try BenchViewpoint.list(in: next), [view])
        XCTAssertThrowsError(try SharedFurnishingPreview.pose(in: next, host: .bench, id: benchId))
        XCTAssertEqual(next.candidate["extensions"][WorkspaceFurnishingPlacement.namespace],
                       placed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace])
        XCTAssertNil(try command.applying(furnishing, to: next))
        XCTAssertThrowsError(try attachment(id: UUID().uuidString).applying(furnishing, to: next))
        XCTAssertThrowsError(try attachment(sha: String(repeating: "b", count: 64)).applying(furnishing, to: placed))
        let changedView = try BenchViewpoint(id: cameraId, name: "Changed", roomId: room, position: view.position, target: view.target, fov: view.fov)
        XCTAssertThrowsError(try attachment(viewpoint: changedView).applying(furnishing, to: next))
    }

    func testWorkspaceBenchAttachmentAddsOnlyTheVisualToAnExistingBackedHost() throws {
        var descriptor = try registered()["furnishing"]
        descriptor["kind"] = .string("bench")
        let furnishing = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let camera = try BenchViewpoint(id: UUID().uuidString.lowercased(), name: "Existing camera", roomId: room,
            position: [-1, 1.2, 0], target: [-1, 1.4, -2], fov: 50)
        let host = try BenchSettings(id: UUID().uuidString.lowercased(), name: "Existing seat", roomId: room, kind: .backed,
            position: [-1, 0, 0], yaw: 0.25, width: 1.5, depth: 0.70, height: 0.45,
            material: .defaults(.wood), viewpointId: camera.id)
        try draft.replaceCandidate(BenchCommand.add(host, viewpoint: camera).proposedCandidate(in: draft, selectedBenchId: host.id))

        func placing(yaw: Double = host.yaw, scale: Double = 1) throws -> NativeExhibitionDraft {
            let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
                furnishingVersion: 1, position: host.position, yaw: yaw, scale: scale)
            return try XCTUnwrap(placement.applying(furnishing, to: draft))
        }
        func command(depth: Double = host.depth, viewpoint: BenchViewpoint? = nil) throws -> WorkspaceBenchSeatAttachment {
            try .init(benchId: host.id, placementId: placementId, furnishingId: furnishing.id, furnishingVersion: 1,
                modelSha256: modelSHA256, width: host.width, depth: depth, height: host.height,
                viewpointId: host.viewpointId, viewpoint: viewpoint)
        }
        let placed = try placing()
        let attachment = try command()
        let linked = try XCTUnwrap(attachment.applying(furnishing, to: placed))
        var expected = placed.candidate
        var row = try XCTUnwrap(expected["extensions"][BenchSettings.namespace]["benches"].items?.first)
        row["workspaceVisual"] = attachment.visual.value
        expected["extensions"][BenchSettings.namespace]["benches"] = .array([row])
        XCTAssertEqual(linked.candidate, expected)
        XCTAssertEqual(linked.document["editVersion"].integer, (placed.document["editVersion"].integer ?? 0) + 1)
        XCTAssertNil(try attachment.applying(furnishing, to: linked))
        XCTAssertThrowsError(try attachment.applying(furnishing, to: placing(yaw: 0)))
        XCTAssertThrowsError(try attachment.applying(furnishing, to: placing(scale: 0.8)))
        XCTAssertThrowsError(try command(depth: 0.68).applying(furnishing, to: placed))
        XCTAssertThrowsError(try command(viewpoint: camera).applying(furnishing, to: placed))
    }

    func testOnlyExactGalleryStoolCanAttachCompactWorkspaceSeat() throws {
        let gallery = "6caca94361b540fb007c6fc605e519e436462eda43f2ec5f99da6a63108bfc60"
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let bench = UUID().uuidString.lowercased()
        let camera = UUID().uuidString.lowercased()
        let view = try BenchViewpoint(id: camera, name: "Seat", roomId: room, position: [-1, 1.16, 0], target: [-1, 1.45, -2], fov: 50)

        func attach(kind: String = "stool", sha: String = gallery, bytes: Int64 = 415_848,
                    width: Double = 0.38, scale: Double = 1) throws -> NativeExhibitionDraft {
            var descriptor = try registered()["furnishing"]
            descriptor["kind"] = .string(kind)
            descriptor["model"]["sha256"] = .string(sha)
            descriptor["model"]["bytes"] = .integer(bytes)
            let furnishing = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
            let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
                furnishingVersion: 1, position: [-1, 0, 0], yaw: 0.125, scale: scale)
            let placed = try XCTUnwrap(placement.applying(furnishing, to: draft, supportedExtensionVersion: 2))
            let command = try WorkspaceBenchSeatAttachment(benchId: bench, placementId: placementId, furnishingId: furnishing.id,
                furnishingVersion: 1, modelSha256: sha, width: width, depth: 0.38, height: 0.45, viewpointId: camera, viewpoint: view)
            let next = try command.applying(furnishing, to: placed)
            return try XCTUnwrap(next)
        }

        let next = try attach()
        let seat = try XCTUnwrap(BenchSettings.list(in: next).first)
        XCTAssertEqual(seat.width, 0.38)
        XCTAssertEqual(seat.workspaceVisual?.modelSha256, gallery)
        XCTAssertEqual(try BenchSettings(value: seat.value), seat)
        XCTAssertThrowsError(try attach(kind: "bench"))
        XCTAssertThrowsError(try attach(sha: String(repeating: "b", count: 64)))
        XCTAssertThrowsError(try attach(bytes: 415_847))
        XCTAssertThrowsError(try attach(width: 0.39))
        XCTAssertThrowsError(try attach(scale: 1.1))
    }

    func testDefaultSeatProposalCombinesProspectivePlacementAndPreservesExistingLink() throws {
        let hash = "6caca94361b540fb007c6fc605e519e436462eda43f2ec5f99da6a63108bfc60"
        var descriptor = try registered()["furnishing"]
        descriptor["kind"] = .string("stool")
        descriptor["model"]["sha256"] = .string(hash)
        descriptor["model"]["bytes"] = .integer(415_848)
        let furnishing = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0.125, scale: 0.5)
        let placed = try XCTUnwrap(placement.applying(furnishing, to: draft, supportedExtensionVersion: 2))
        let proposal = try WorkspaceDefaultSeatProposal(draft: placed, placementIds: [placementId])
        let target = try XCTUnwrap(proposal.targets.first)
        let view = try BenchViewpoint(id: target.viewpointId, name: furnishing.details.name, roomId: room,
            position: [-1, 0.925, 0], target: [-1, 0.925, -2], fov: 50)
        let seat = try BenchSettings(id: target.benchId, name: furnishing.details.name, roomId: room, kind: .slab,
            position: placement.position, yaw: placement.yaw, width: 0.19, depth: 0.19, height: 0.225, material: .defaults(.wood),
            viewpointId: view.id, workspaceVisual: .init(placementId: placementId, modelSha256: hash))
        let addition: DocumentValue = .object(["placementId": .string(placementId), "bench": seat.value, "viewpoint": view.value])
        let reply: DocumentValue = .object(["additions": .array([addition])])
        let combined = try XCTUnwrap(proposal.applying(reply, to: draft))
        XCTAssertEqual(combined.document["editVersion"], placed.document["editVersion"])
        XCTAssertEqual(combined.candidate["extensions"][WorkspaceFurnishingPlacement.namespace],
                       placed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace])
        XCTAssertEqual(try BenchSettings.list(in: combined), [seat])
        XCTAssertThrowsError(try proposal.applying(.object(["additions": .array([addition, addition])]), to: draft))
        XCTAssertThrowsError(try proposal.applying(.object(["additions": .array([])]), to: draft))

        let secondId = UUID().uuidString.lowercased()
        let secondPlacement = WorkspaceFurnishingPlacement(id: secondId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
        let both = try XCTUnwrap(secondPlacement.applying(furnishing, to: placed, supportedExtensionVersion: 2))
        let multi = try WorkspaceDefaultSeatProposal(draft: both, placementIds: [placementId, secondId])
        let first = try XCTUnwrap(multi.targets.first)
        var partial = addition
        partial["bench"]["id"] = .string(first.benchId)
        partial["bench"]["viewpointId"] = .string(first.viewpointId)
        partial["viewpoint"]["id"] = .string(first.viewpointId)
        XCTAssertThrowsError(try multi.applying(.object(["additions": .array([partial])]), to: placed))

        let repeated = try WorkspaceDefaultSeatProposal(draft: combined, placementIds: [placementId])
        XCTAssertNil(try repeated.applying(.object(["additions": .array([])]), to: combined))
        XCTAssertEqual(try BenchViewpoint.list(in: combined), [view])
    }

    func testVisualPlacementIsRoomLocalAndReplaysExactly() throws {
        let furnishing = try XCTUnwrap(intake(registered()).furnishing)
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        candidate["extensions"] = .object(["example": .string("kept")])
        try draft.replaceCandidate(candidate)
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0, scale: 1)
        let changed = try XCTUnwrap(placement.applying(furnishing, to: draft))
        XCTAssertEqual(changed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["version"], .integer(1))
        XCTAssertEqual(changed.candidate["extensions"]["example"], .string("kept"))
        XCTAssertEqual(changed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"].items?.count, 1)
        XCTAssertEqual(changed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"].items?.count, 1)
        XCTAssertNil(try placement.applying(furnishing, to: changed))
        let conflict = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
        XCTAssertThrowsError(try conflict.applying(furnishing, to: changed))
        let outside = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [10, 0, 0], yaw: 0, scale: 1)
        XCTAssertThrowsError(try outside.applying(furnishing, to: draft))
    }

    func testFrameAttachmentSurvivesRoomPlacementAndDetachesOnlyItsDescriptor() throws {
        let pedestal = try XCTUnwrap(intake(registered()).furnishing)
        var frameWire = try registered()["furnishing"]
        frameWire["id"] = .string(intakeId)
        frameWire["kind"] = .string("artwork-frame")
        frameWire["model"]["bytes"] = .integer(Int64(WorkspaceArtworkFrameAttachment.thinBrassBytes))
        frameWire["model"]["sha256"] = .string(WorkspaceArtworkFrameAttachment.thinBrassSHA256)
        let frame = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(frameWire))
        var draft = try NativeExhibitionDraft.new()
        let roomId = try XCTUnwrap(draft.rooms.first?["id"].string)
        let assetId = "10000000-0000-4000-8000-000000000005"
        let artwork: DocumentValue = .object([
            "kind": .string("artwork"), "revisionId": .string(requestId), "primaryAssetId": .string(assetId)
        ])
        try draft.addApprovedArtwork(artwork, roomId: roomId, placementId: placementId, transform: NativeExhibitionDraft.identity)
        var candidate = draft.candidate
        candidate["extensions"] = .object(["example": .string("kept")])
        try draft.replaceCandidate(candidate)

        let attach = WorkspaceArtworkFrameCommand.attach(.init(placementId: placementId,
            artworkRevisionId: requestId, assetId: assetId, furnishingId: frame.id,
            furnishingVersion: 1, modelSha256: WorkspaceArtworkFrameAttachment.thinBrassSHA256))
        let detach = WorkspaceArtworkFrameCommand.detach(placementId: placementId)
        let namespace = WorkspaceFurnishingPlacement.namespace
        let attached = try XCTUnwrap(attach.applying(to: draft, furnishing: frame))
        XCTAssertEqual(attached.placements, draft.placements)
        XCTAssertEqual(attached.candidate["extensions"][namespace]["placements"], .array([]))
        XCTAssertNil(try attach.applying(to: attached, furnishing: frame))
        let empty = try XCTUnwrap(detach.applying(to: attached, furnishing: nil))
        XCTAssertEqual(empty.candidate, draft.candidate)
        XCTAssertThrowsError(try detach.applying(to: empty, furnishing: nil)) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "FRAME_TARGET_STALE")
        }

        let roomPlacement = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006",
            roomId: roomId, furnishingId: pedestal.id, furnishingVersion: 1, position: [0, 0, 0], yaw: 0, scale: 1)
        let mixed = try XCTUnwrap(roomPlacement.applying(pedestal, to: attached, supportedExtensionVersion: 4))
        let mixedExtension = mixed.candidate["extensions"][namespace]
        XCTAssertEqual(mixedExtension["frameAttachments"], attached.candidate["extensions"][namespace]["frameAttachments"])
        let detached = try XCTUnwrap(detach.applying(to: mixed, furnishing: nil))
        let remaining = detached.candidate["extensions"][namespace]
        XCTAssertEqual(remaining["version"], .integer(4))
        XCTAssertEqual(remaining["frameAttachments"], .array([]))
        XCTAssertEqual(remaining["placements"], mixedExtension["placements"])
        XCTAssertEqual(remaining["furnishings"].items?.map { $0["id"].string }, [pedestal.id])

        let walnutHash = "b3f0bf2e6dbe659b1484ea6425848ad96b88d0be4b61ca82f5a4694462a7e03e"
        frameWire["id"] = .string("10000000-0000-4000-8000-000000000007")
        frameWire["model"]["bytes"] = .integer(196_160)
        frameWire["model"]["sha256"] = .string(walnutHash)
        let walnut = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(frameWire))
        let replace = WorkspaceArtworkFrameCommand.attach(.init(placementId: placementId,
            artworkRevisionId: requestId, assetId: assetId, furnishingId: walnut.id,
            furnishingVersion: 1, modelSha256: walnutHash))
        XCTAssertThrowsError(try replace.applying(to: mixed, furnishing: walnut)) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "FRAME_ASSET_UNSUPPORTED")
        }
        let upgraded = try XCTUnwrap(replace.applying(to: mixed, furnishing: walnut, supportedExtensionVersion: 5))
        let v5 = upgraded.candidate["extensions"][namespace]
        XCTAssertEqual(v5["version"], .integer(5))
        XCTAssertEqual(v5["frameAttachments"].items?.map { $0["modelSha256"].string }, [walnutHash])
        XCTAssertEqual(v5["furnishings"].items?.map { $0["id"].string }, [pedestal.id, walnut.id])
        XCTAssertEqual(v5["placements"], mixedExtension["placements"])
        XCTAssertNil(try replace.applying(to: upgraded, furnishing: walnut, supportedExtensionVersion: 5))
        XCTAssertThrowsError(try detach.applying(to: upgraded, furnishing: nil)) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        let restoredThinBrass = try XCTUnwrap(attach.applying(to: upgraded, furnishing: frame, supportedExtensionVersion: 5))
        XCTAssertEqual(restoredThinBrass.candidate["extensions"][namespace]["version"], .integer(5))
        let secondRoomPlacement = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000008",
            roomId: roomId, furnishingId: pedestal.id, furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
        let afterRoomEdit = try XCTUnwrap(secondRoomPlacement.applying(pedestal, to: upgraded, supportedExtensionVersion: 5))
        XCTAssertEqual(afterRoomEdit.candidate["extensions"][namespace]["frameAttachments"], v5["frameAttachments"])
        let v5Remaining = try XCTUnwrap(detach.applying(to: upgraded, furnishing: nil, supportedExtensionVersion: 5))
        XCTAssertEqual(v5Remaining.candidate["extensions"][namespace]["version"], .integer(5))
        XCTAssertEqual(v5Remaining.candidate["extensions"][namespace]["frameAttachments"], .array([]))
        let onlyFrame = try XCTUnwrap(replace.applying(to: draft, furnishing: walnut, supportedExtensionVersion: 5))
        let detachedOnlyFrame = try XCTUnwrap(detach.applying(to: onlyFrame, furnishing: nil, supportedExtensionVersion: 5))
        XCTAssertEqual(detachedOnlyFrame.candidate, draft.candidate)
    }

    func testAdditionalFrameModelsRequireBothSessionAdvertisements() throws {
        var session: DocumentValue = .object([
            "userId": .string(intakeId), "tenantId": .string(requestId), "role": .string("artist"),
            "csrfToken": .string("fixture"), "expiresAt": .string("2026-12-01T00:00:00Z"),
            "workspaceFurnishingProfile": .string("workspace-furnishing-intake-v1"),
            "workspaceFurnishingStoolProfile": .string("workspace-furnishing-stool-v1"),
            "workspaceFurnishingRoomVisualProfile": .string("workspace-furnishing-room-visual-v1"),
            "workspaceArtworkFrameModelsProfile": .string("workspace-artwork-frame-models-v1")
        ])
        func advertised() throws -> Bool {
            try JSONDecoder().decode(AuthoringSession.self, from: JSONEncoder().encode(session))
                .advertisesWorkspaceArtworkFrameModels
        }
        XCTAssertFalse(try advertised())
        session["workspaceArtworkFrameProfile"] = .string("workspace-artwork-frame-v1")
        XCTAssertTrue(try advertised())
        session["workspaceArtworkFrameModelsProfile"] = .string("workspace-artwork-frame-models-v2")
        XCTAssertFalse(try advertised())
    }

    func testStoolUsesExistingIntakeWireAndRequiresExactSessionAdvertisement() throws {
        var session: DocumentValue = .object([
            "userId": .string(intakeId), "tenantId": .string(requestId), "role": .string("artist"),
            "csrfToken": .string("fixture"), "expiresAt": .string("2026-12-01T00:00:00Z"),
            "workspaceFurnishingProfile": .string("workspace-furnishing-intake-v1")
        ])
        func advertised() throws -> Bool {
            try JSONDecoder().decode(AuthoringSession.self, from: JSONEncoder().encode(session))
                .advertisesWorkspaceFurnishingStool
        }
        XCTAssertFalse(try advertised())
        session["workspaceFurnishingStoolProfile"] = .string("workspace-furnishing-stool-v2")
        XCTAssertFalse(try advertised())
        session["workspaceFurnishingStoolProfile"] = .string("workspace-furnishing-stool-v1")
        XCTAssertTrue(try advertised())
        session["workspaceFurnishingProfile"] = .null
        XCTAssertFalse(try advertised())

        var wire = try registered()
        wire["declaration"]["kind"] = .string("stool")
        wire["furnishing"]["kind"] = .string("stool")
        let value = try intake(wire)
        XCTAssertThrowsError(try value.check())
        try value.check(supportedExtensionVersion: 2)
        let request = WorkspaceFurnishingRequest(requestId: requestId, details: value.declaration)
        var expected = wire["declaration"]
        expected["requestId"] = .string(requestId)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)), expected)
        XCTAssertEqual(value.furnishing?.version, 1)
    }

    func testStoolUpgradesPlacementExtensionAndPreservesLegacyRows() throws {
        let pedestal = try XCTUnwrap(intake(registered()).furnishing)
        var wire = try registered()
        wire["furnishing"]["id"] = .string("10000000-0000-4000-8000-000000000005")
        wire["furnishing"]["kind"] = .string("stool")
        let stool = try XCTUnwrap(intake(wire).furnishing)
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let first = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: pedestal.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0, scale: 1)
        let addStool = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006",
            roomId: room, furnishingId: stool.id, furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
        let legacy = try XCTUnwrap(first.applying(pedestal, to: draft, supportedExtensionVersion: 2))
        let namespace = WorkspaceFurnishingPlacement.namespace
        let v1 = legacy.candidate["extensions"][namespace]
        XCTAssertEqual(v1["version"], .integer(1))
        XCTAssertThrowsError(try addStool.applying(stool, to: legacy)) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        let upgraded = try XCTUnwrap(addStool.applying(stool, to: legacy, supportedExtensionVersion: 2))
        let v2 = upgraded.candidate["extensions"][namespace]
        XCTAssertEqual(v2["version"], .integer(2))
        XCTAssertEqual(v2["furnishings"].items?.first, v1["furnishings"].items?.first)
        XCTAssertEqual(v2["placements"].items?.first, v1["placements"].items?.first)
        XCTAssertEqual(v2["furnishings"].items?.last, wire["furnishing"])
        XCTAssertNil(try addStool.applying(stool, to: upgraded, supportedExtensionVersion: 2))
        XCTAssertThrowsError(try first.applying(pedestal, to: upgraded))
        let another = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000007",
            roomId: room, furnishingId: pedestal.id, furnishingVersion: 1, position: [0, 0, 1], yaw: 0, scale: 1)
        let kept = try XCTUnwrap(another.applying(pedestal, to: upgraded, supportedExtensionVersion: 2))
        XCTAssertEqual(kept.candidate["extensions"][namespace]["version"], .integer(2))
    }

    func testUnqualifiedRuntimeRejectsFurnishingReadBeforeNetwork() async throws {
        let api = try AuthoringAPI(port: 49151, tenantId: intakeId, nativeRuntimeManifestSHA256: modelSHA256)
        XCTAssertFalse(api.workspaceFurnishingRuntimeQualified)
        XCTAssertFalse(api.workspaceFramePreparationRuntimeQualified)
        do {
            _ = try await api.workspaceFurnishingIntakes()
            XCTFail("Unqualified runtime admitted")
        } catch let failure as AuthoringFailure {
            XCTAssertEqual(failure.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
        }
    }

    #if DEBUG
    func testFramePreparationRuntimePinDoesNotChangeEarlierFurnishingAdmission() throws {
        let earlier = try AuthoringAPI(port: 49151, tenantId: intakeId,
            nativeRuntimeManifestSHA256: "7f76b19894fe3a9824a640cf1856ba7c828d5bd3201ede0da5d1c53a501bd1e7")
        let preparation = try AuthoringAPI(port: 49151, tenantId: intakeId,
            nativeRuntimeManifestSHA256: "1c1f3cbddae02e2c0a1053da99362f0fc110558aa5b5bd4f00cded2eec948a63")
        XCTAssertTrue(earlier.workspaceFurnishingRuntimeQualified)
        XCTAssertFalse(earlier.workspaceFramePreparationRuntimeQualified)
        XCTAssertTrue(preparation.workspaceFurnishingRuntimeQualified)
        XCTAssertTrue(preparation.workspaceFramePreparationRuntimeQualified)
    }
    func testRequiredModelRuntimeRetainsWorkspaceReadCapabilities() throws {
        let selected = try AuthoringAPI(port: 49151, tenantId: intakeId,
            nativeRuntimeManifestSHA256: "af4e787a5fa5c714d60bf24cba23a1b2e7015946d1f4bea9a5777a3af3c8b2d0")
        let other = try AuthoringAPI(port: 49151, tenantId: intakeId,
            nativeRuntimeManifestSHA256: "af4e787a5fa5c714d60bf24cba23a1b2e7015946d1f4bea9a5777a3af3c8b2d1")
        XCTAssertTrue(selected.workspaceMaterialRuntimeQualified)
        XCTAssertTrue(selected.workspaceFurnishingRuntimeQualified)
        XCTAssertTrue(selected.workspacePortalFinishRuntimeQualified)
        XCTAssertFalse(selected.hayesOriginalRuntimeQualified)
        XCTAssertFalse(other.workspaceMaterialRuntimeQualified)
        XCTAssertFalse(other.workspaceFurnishingRuntimeQualified)
    }

    func testDefaultFrameRuntimePinAdmitsOnlyFurnishingCapability() async throws {
        let frame = try AuthoringAPI(port: 49151, tenantId: intakeId,
            nativeRuntimeManifestSHA256: "a70e062ea7f1d0aa799e4dd924cfe6043bde06ffe11960e2967e60c0ab8285be")
        let other = try AuthoringAPI(port: 49151, tenantId: intakeId,
            nativeRuntimeManifestSHA256: "a70e062ea7f1d0aa799e4dd924cfe6043bde06ffe11960e2967e60c0ab8285bf")
        XCTAssertTrue(frame.workspaceFurnishingRuntimeQualified)
        XCTAssertFalse(frame.workspaceFramePreparationRuntimeQualified)
        XCTAssertFalse(frame.workspacePortalFinishRuntimeQualified)
        XCTAssertFalse(frame.workspaceMaterialRuntimeQualified)
        XCTAssertFalse(frame.hayesOriginalRuntimeQualified)
        XCTAssertFalse(other.workspaceFurnishingRuntimeQualified)
        let admitsFrames = await frame.admitsWorkspaceArtworkFrames
        XCTAssertFalse(admitsFrames)
    }
    #endif

    func testRoomVisualAdvertisementRequiresBothParentProfiles() throws {
        var fields: DocumentValue = .object([
            "userId": .string(intakeId), "tenantId": .string(requestId), "role": .string("artist"),
            "csrfToken": .string("fixture"), "expiresAt": .string("2026-12-01T00:00:00Z"),
            "workspaceFurnishingRoomVisualProfile": .string("workspace-furnishing-room-visual-v1")
        ])
        func advertised() throws -> Bool {
            try JSONDecoder().decode(AuthoringSession.self, from: JSONEncoder().encode(fields))
                .advertisesWorkspaceFurnishingRoomVisual
        }
        XCTAssertFalse(try advertised())
        fields["workspaceFurnishingProfile"] = .string("workspace-furnishing-intake-v1")
        XCTAssertFalse(try advertised())
        fields["workspaceFurnishingStoolProfile"] = .string("workspace-furnishing-stool-v1")
        XCTAssertTrue(try advertised())
        fields["workspaceFurnishingRoomVisualProfile"] = .string("workspace-furnishing-room-visual-v2")
        XCTAssertFalse(try advertised())
    }

    func testRoomVisualKindsKeepTheIntakeWireAndUpgradeOnlyWhenPlaced() throws {
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let namespace = WorkspaceFurnishingPlacement.namespace
        var stoolWire = try registered()
        stoolWire["furnishing"]["kind"] = .string("stool")
        let stool = try XCTUnwrap(intake(stoolWire).furnishing)
        let oldPose = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: stool.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0, scale: 1)
        let oldDraft = try XCTUnwrap(oldPose.applying(stool, to: draft, supportedExtensionVersion: 3))
        let v2 = oldDraft.candidate["extensions"][namespace]
        XCTAssertEqual(v2["version"], .integer(2))
        for name in ["partition", "info-stand", "barrier", "display-case"] {
            var wire = try registered()
            wire["declaration"]["kind"] = .string(name)
            wire["furnishing"]["kind"] = .string(name)
            wire["furnishing"]["id"] = .string("10000000-0000-4000-8000-000000000005")
            let response = try intake(wire)
            XCTAssertThrowsError(try response.check(supportedExtensionVersion: 2))
            try response.check(supportedExtensionVersion: 3)
            let request = WorkspaceFurnishingRequest(requestId: requestId, details: response.declaration)
            var expected = wire["declaration"]
            expected["requestId"] = .string(requestId)
            XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)), expected)
            let furnishing = try XCTUnwrap(response.furnishing)
            let pose = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006",
                roomId: room, furnishingId: furnishing.id, furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
            XCTAssertThrowsError(try pose.applying(furnishing, to: oldDraft, supportedExtensionVersion: 2))
            let upgraded = try XCTUnwrap(pose.applying(furnishing, to: oldDraft, supportedExtensionVersion: 3))
            let v3 = upgraded.candidate["extensions"][namespace]
            XCTAssertEqual(v3["version"], .integer(3))
            XCTAssertEqual(v3["furnishings"].items, [stoolWire["furnishing"], wire["furnishing"]])
            XCTAssertEqual(v3["placements"].items?.first, v2["placements"].items?.first)
            XCTAssertNil(try pose.applying(furnishing, to: upgraded, supportedExtensionVersion: 3))
            XCTAssertThrowsError(try oldPose.applying(stool, to: upgraded, supportedExtensionVersion: 2)) {
                XCTAssertEqual(($0 as? AuthoringFailure)?.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            let another = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000007",
                roomId: room, furnishingId: stool.id, furnishingVersion: 1, position: [0, 0, 1], yaw: 0, scale: 1)
            let kept = try XCTUnwrap(another.applying(stool, to: upgraded, supportedExtensionVersion: 3))
            XCTAssertEqual(kept.candidate["extensions"][namespace]["version"], .integer(3))
        }
    }
}
