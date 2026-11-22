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

    func testFixtureFirstAdditionChecksOneLightAndCompleteFixtureGraph() throws {
        let base = try fixture(visual: false)
        let roomId = try XCTUnwrap(base.rooms.first?["id"].string)
        let bound = frame(base)
        for special in ["museum-track-spot-fixed-64p1", "museum-wallwasher-rear-relief", NativeTrackSpotAim.catalogue] {
            XCTAssertThrowsError(try LightObjectRequest.fixtureAddition(roomId: roomId, fixture: .none,
                                                                        catalogueId: special, in: base))
        }
        for catalog in [nil, "museum-track-spot"] as [String?] {
            let command = try LightObjectRequest.fixtureAddition(roomId: roomId, fixture: catalog == nil ? .spotCan : .none,
                                                                catalogueId: catalog, in: base)
            let request = try LightObjectRequest(command: command, frame: bound)
            XCTAssertEqual(command["settings"]["aim"]["kind"], .string("point"))
            var expected = changed(base.candidate)
            var newLight = command["light"]; newLight["id"] = .string(clone)
            expected["lights"] = .array(try XCTUnwrap(base.candidate["lights"].items) + [newLight])
            var setting = command["settings"]; setting["lightId"] = .string(clone); setting["fixture"] = .string("spot-can")
            expected["extensions"][details]["lights"] = .array(try XCTUnwrap(base.candidate["extensions"][details]["lights"].items) + [setting])
            if let catalog {
                XCTAssertEqual(command["visual"]["pose"]["position"], .vector([0, 0.15, 0.0755]))
                XCTAssertNotEqual(command["visual"]["pose"]["position"], command["light"]["transform"]["position"])
                let binding: DocumentValue = .object(["hostKind": .string("light"), "hostId": .string(clone),
                    "assetId": .string(asset), "pose": command["visual"]["pose"]])
                expected["extensions"][visuals] = .object(["version": .integer(1),
                    "assets": .array([try SharedFurnishingCatalog.asset(catalogId: catalog, id: asset)]),
                    "bindings": .array([binding])])
                let orphan = droppingExtension(expected, visuals)
                XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: orphan, lightId: clone)))
            }
            let checked = try request.checkedReply(reply(request, candidate: expected, lightId: clone))
            XCTAssertEqual(checked.lightId, clone)
            XCTAssertEqual(try checked.apply(to: base).candidate, expected)
            XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: expected, lightId: source)))
            var altered = expected; altered["title"] = .string("Unrelated edit")
            XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: altered, lightId: clone)))
            var bare = without(command, "visual"); bare["settings"]["fixture"] = .string("none")
            XCTAssertThrowsError(try LightObjectRequest(command: bare, frame: bound)) {
                XCTAssertEqual(($0 as? AuthoringFailure)?.code, "LIGHT_FIXTURE_REQUIRED")
            }
        }
    }

    func testBeamFitAcceptsOnlySelectedSpotWidthAndBoundedEvidence() throws {
        var draft = try fixture()
        let placement = UUID().uuidString.lowercased()
        var initial = draft.candidate
        initial["placements"] = .array([.object(["id": .string(placement), "roomId": draft.rooms[0]["id"]])])
        let point: DocumentValue = .object(["kind": .string("point"), "position": .vector([0, 1.3, 2.6])])
        var detailsRows = try XCTUnwrap(initial["extensions"][details]["lights"].items)
        detailsRows[0]["aim"] = point
        initial["extensions"][details]["lights"] = .array(detailsRows)
        try draft.replaceCandidate(initial)
        let command = try LightObjectRequest.beamFitCommand(lightId: source, placementId: placement, in: draft)
        let request = try LightObjectRequest(command: command, frame: frame(draft))
        var expected = changed(initial)
        var lights = expected["lights"].items!
        lights[0]["beamAngle"] = .number(0.65)
        expected["lights"] = .array(lights)
        var result = reply(request, candidate: expected)
        XCTAssertThrowsError(try request.checkedReply(result))
        result["beamFit"] = .object(["placementId": .string(placement), "bounds": .string("display-plane"),
            "maxDistanceMeters": .number(2.0), "maxPolarAngleRadians": .number(0.20),
            "minImageIncidence": .number(0.9), "floorSpill": .string("unknown")])
        XCTAssertEqual(try request.checkedReply(result).beamFit?.placementId, placement)
        XCTAssertEqual(try request.checkedReply(result).candidate, expected)

        var mutated = result
        mutated["beamFit"]["maxPolarAngleRadians"] = .number(.pi / 2)
        XCTAssertThrowsError(try request.checkedReply(mutated))
        mutated = result
        mutated["beamFit"]["placementId"] = .string(UUID().uuidString)
        XCTAssertThrowsError(try request.checkedReply(mutated))
        mutated = result
        mutated["beamFit"]["minImageIncidence"] = .null
        XCTAssertThrowsError(try request.checkedReply(mutated))
        mutated["beamFit"]["bounds"] = .string("sculpture-containment-envelope")
        XCTAssertEqual(try request.checkedReply(mutated).beamFit?.bounds, .sculptureContainmentEnvelope)
        lights[1]["beamAngle"] = .number(0.4)
        mutated = result
        mutated["candidate"]["lights"] = .array(lights)
        XCTAssertThrowsError(try request.checkedReply(mutated))
        mutated = result
        lights = expected["lights"].items!
        lights[0]["transform"]["position"] = .vector([0, 3.1, 0])
        mutated["candidate"]["lights"] = .array(lights)
        XCTAssertThrowsError(try request.checkedReply(mutated))
        var mixedEdit = command
        mixedEdit["light"]["intensity"] = .integer(220)
        XCTAssertThrowsError(try LightObjectRequest(command: mixedEdit, frame: frame(draft)))
        mixedEdit = command
        mixedEdit["settings"]["aim"] = .object(["kind": .string("placement-center")])
        XCTAssertThrowsError(try LightObjectRequest(command: mixedEdit, frame: frame(draft)))

        let fitted = try request.checkedReply(result).apply(to: draft)
        var manual = command
        manual = without(manual, "fitBeam")
        manual["light"]["beamAngle"] = .number(0.44)
        let manualRequest = try LightObjectRequest(command: manual, frame: frame(fitted))
        XCTAssertEqual(manualRequest.value["fitBeam"], .null)
        var manualCandidate = expected
        lights = expected["lights"].items!
        lights[0]["beamAngle"] = .number(0.44)
        manualCandidate["lights"] = .array(lights)
        let manualResult = reply(manualRequest, candidate: manualCandidate)
        XCTAssertNil(try manualRequest.checkedReply(manualResult).beamFit)
        let manuallyAdjusted = try manualRequest.checkedReply(manualResult).apply(to: fitted)
        let refit = try LightObjectRequest.beamFitCommand(lightId: source, placementId: placement, in: manuallyAdjusted)
        XCTAssertEqual(refit["fitBeam"], command["fitBeam"])
        XCTAssertEqual(refit["light"]["beamAngle"], .number(0.44))
    }

    @MainActor
    func testBeamFitRequiresExactNegotiatedProfileBeforeTransport() async throws {
        var draft = try fixture(), base = draft.candidate
        base["placements"] = .array([.object(["id": .string(clone), "roomId": draft.rooms[0]["id"]])])
        var settings = try XCTUnwrap(base["extensions"][details]["lights"].items)
        settings[0]["aim"] = .object(["kind": .string("point"), "position": .vector([0, 1, 2])])
        base["extensions"][details]["lights"] = .array(settings)
        try draft.replaceCandidate(base)
        let owner = UUID().uuidString.lowercased(), epoch = UUID().uuidString.lowercased()
        let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\""
        let document = NativeDocumentState(format: 1, revision: 1, workspaceId: UUID().uuidString.lowercased(),
            tenantId: owner, principalId: owner, draft: draft, etag: etag,
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let ack = ViewportFrame(epoch: epoch, sequence: 1, tenantId: owner, principalId: owner,
            draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
        let bound = try LightObjectRequest.nativeFrame(acknowledged: ack, document: document, ownerEpoch: epoch,
            acceptedSequence: 1, readOnly: false, boundedEdit: nil)
        let command = try LightObjectRequest.beamFitCommand(lightId: source, placementId: clone, in: draft)
        let request = try LightObjectRequest(command: command, frame: bound)
        let saved = NativeStudioResponse(revision: 1, etag: etag, draft: draft)
        var expected = changed(base), lights = try XCTUnwrap(base["lights"].items)
        lights[0]["beamAngle"] = .number(0.63)
        expected["lights"] = .array(lights)
        var response = reply(request, candidate: expected)
        response["beamFit"] = .object(["placementId": .string(clone), "bounds": .string("display-plane"),
            "maxDistanceMeters": .number(2), "maxPolarAngleRadians": .number(0.2),
            "minImageIncidence": .number(1), "floorSpill": .string("unknown")])

        let supported = "saved-head-spot-beam-fit-v1"
        defer { LightTransportProtocol.reset() }
        for profile: String? in [nil, supported + "-unknown", supported] {
            try LightTransportProtocol.configure(owner: owner, reply: response, etag: etag, profile: true, fitProfile: profile)
            let api = try AuthoringAPI(port: 18091, tenantId: owner, nativeRuntimeManifestSHA256: String(repeating: "b", count: 64),
                                       protocolClasses: [LightTransportProtocol.self])
            _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
                origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
            do {
                let preview = try await api.prepareNativeLightObject(request, in: document, saved: saved)
                XCTAssertEqual(profile, supported)
                XCTAssertEqual(preview.candidate, expected)
            } catch let failure as AuthoringFailure {
                XCTAssertNotEqual(profile, supported)
                XCTAssertEqual(failure.code, "NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE")
            }
            let calls = LightTransportProtocol.requests()
            XCTAssertEqual(calls.map(\.httpMethod), profile == supported ? ["GET", "POST"] : ["GET"])
            if profile == supported {
                XCTAssertEqual(calls.last?.value(forHTTPHeaderField: "If-Match"), etag)
                let body = try JSONDecoder().decode(DocumentValue.self, from: LightTransportProtocol.body())
                XCTAssertEqual(body["request"]["fitBeam"], command["fitBeam"])
            }
            await api.endLocalAuthoringSession()
        }
    }

    func testBeamFitPreservesSavedArticulatedAimAndUpperFraming() throws {
        var draft = try fixture(visual: false), base = draft.candidate
        base["placements"] = .array([.object(["id": .string(clone), "roomId": draft.rooms[0]["id"],
            "transform": .object(["position": .vector([1.5, 1.5, -1.919])])])])
        let aim: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeTrackSpotAim.profile),
            "mount": .object(["railPlacementId": .string(surface), "position": .vector([0, 0, 0]), "yaw": .integer(0)]),
            "aim": .object(["kind": .string("placement-center"), "placementId": .string(clone), "offset": .vector([0, 0, 0])])])
        let profile: DocumentValue = .object(["version": .integer(1), "kind": .string("upper-framing"),
            "cutoffAngle": .number(0.16), "softnessAngle": .number(0.25)])
        var rows = try XCTUnwrap(base["extensions"][details]["lights"].items)
        rows[0] = without(rows[0], "aim")
        rows[0]["fixture"] = .string("none")
        rows[0]["trackAim"] = aim
        rows[0]["angularProfile"] = profile
        base["extensions"][details]["lights"] = .array(rows)
        base["extensions"][attachments]["lights"] = .array([try XCTUnwrap(base["extensions"][attachments]["lights"].items?.last)])
        base["extensions"][visuals] = .object(["version": .integer(1),
            "assets": .array([try SharedFurnishingCatalog.asset(catalogId: NativeTrackSpotAim.catalogue, id: asset)]),
            "bindings": .array([.object(["hostKind": .string("light"), "hostId": .string(source),
                "assetId": .string(asset), "pose": NativeTrackSpotAim.pose])])])
        try draft.replaceCandidate(base)
        let command = try LightObjectRequest.beamFitCommand(lightId: source, placementId: clone, in: draft)
        XCTAssertEqual(command["settings"]["angularProfile"], profile)
        let request = try LightObjectRequest(command: command, frame: frame(draft))
        var expected = changed(base), lights = try XCTUnwrap(base["lights"].items)
        lights[0]["beamAngle"] = .number(0.66)
        expected["lights"] = .array(lights)
        var result = reply(request, candidate: expected)
        result["beamFit"] = .object(["placementId": .string(clone), "bounds": .string("display-plane"),
            "maxDistanceMeters": .number(2), "maxPolarAngleRadians": .number(0.2),
            "minImageIncidence": .number(1), "floorSpill": .string("unknown")])
        var fitted = try request.checkedReply(result).apply(to: draft)
        XCTAssertEqual(fitted.candidate, expected)

        var changedMask = command
        changedMask["settings"]["angularProfile"]["cutoffAngle"] = .number(0.17)
        XCTAssertThrowsError(try LightObjectRequest(command: changedMask, frame: frame(draft)))
        rows[0]["angularProfile"]["cutoffAngle"] = .number(0.17)
        var changedReply = result
        changedReply["candidate"]["extensions"][details]["lights"] = .array(rows)
        XCTAssertThrowsError(try request.checkedReply(changedReply))
        lights[0]["transform"]["rotation"] = .vector([0, 0, 0, 2])
        result["candidate"]["lights"] = .array(lights)
        XCTAssertThrowsError(try request.checkedReply(result))

        var moved = fitted.candidate, placements = try XCTUnwrap(moved["placements"].items)
        placements[0]["transform"]["position"] = .vector([1.5, 1.62, -1.919])
        moved["placements"] = .array(placements)
        try fitted.replaceCandidate(moved)
        let refitCommand = try LightObjectRequest.beamFitCommand(lightId: source, placementId: clone, in: fitted)
        XCTAssertEqual(refitCommand["light"]["beamAngle"], .number(0.66))
        XCTAssertEqual(refitCommand["settings"]["trackAim"], aim)
        let refitRequest = try LightObjectRequest(command: refitCommand, frame: frame(fitted))
        var refitCandidate = moved
        lights = try XCTUnwrap(moved["lights"].items)
        lights[0]["beamAngle"] = .number(0.71)
        lights[0]["transform"]["position"] = .vector([0, 3.00075, -0.0013])
        lights[0]["transform"]["rotation"] = .vector([-0.4946986459514279, 0, 0, 0.8690645831546835])
        refitCandidate["lights"] = .array(lights)
        var refitReply = reply(refitRequest, candidate: refitCandidate)
        refitReply["beamFit"] = result["beamFit"]
        XCTAssertEqual(try refitRequest.checkedReply(refitReply).apply(to: fitted).candidate, refitCandidate)
        var changedAim = refitReply, alteredDetails = try XCTUnwrap(moved["extensions"][details]["lights"].items)
        alteredDetails[0]["trackAim"]["aim"]["offset"] = .vector([0.1, 0, 0])
        changedAim["candidate"]["extensions"][details]["lights"] = .array(alteredDetails)
        XCTAssertThrowsError(try refitRequest.checkedReply(changedAim))
    }

    func testNativeTransportBindsOrdinaryACKWithoutInventingWebCapability() throws {
        let draft = try fixture(), ack = frame(draft)
        var document = NativeDocumentState(format: 1, revision: 1, workspaceId: UUID().uuidString.lowercased(),
            tenantId: ack.tenantId, principalId: ack.principalId, draft: draft, etag: "\"studio-r1-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        func bind(_ frame: ViewportFrame? = nil, owner: String? = nil, sequence: Int = 1, readOnly: Bool? = false,
                  bounded: NativeBoundedEditCapability? = nil) throws -> ViewportFrame {
            try LightObjectRequest.nativeFrame(acknowledged: frame ?? ack, document: document, ownerEpoch: owner ?? ack.epoch,
                acceptedSequence: sequence, readOnly: readOnly, boundedEdit: bounded)
        }
        let bound = try bind()
        XCTAssertNil(ack.workspaceId); XCTAssertNil(ack.baseETag)
        XCTAssertEqual(bound.workspaceId, document.workspaceId); XCTAssertEqual(bound.baseETag, document.etag)
        XCTAssertEqual(bound.draft, draft); XCTAssertEqual(bound.sequence, ack.sequence)
        XCTAssertThrowsError(try bind(owner: UUID().uuidString.lowercased()))
        XCTAssertThrowsError(try bind(sequence: 2)); XCTAssertThrowsError(try bind(sequence: 0))
        XCTAssertThrowsError(try bind(readOnly: nil)); XCTAssertThrowsError(try bind(readOnly: true))
        let bounded = NativeBoundedEditCapability(version: 1, kind: "native-bounded-edit-v1", tenantId: ack.tenantId,
            principalId: ack.principalId, workspaceId: document.workspaceId, exhibitionId: draft.id, revision: 1, etag: document.etag!)
        XCTAssertThrowsError(try bind(bounded: bounded))
        func changedFrame(draft candidate: NativeExhibitionDraft? = nil, tenant: String? = nil, principal: String? = nil,
                          disabled: Bool = false, workspace: String? = nil, etag: String? = nil) -> ViewportFrame {
            ViewportFrame(epoch: ack.epoch, sequence: 1, tenantId: tenant ?? ack.tenantId, principalId: principal ?? ack.principalId,
                draft: candidate ?? draft, selection: nil, mode: "translate", view: "isometric", disabled: disabled, workspaceId: workspace, baseETag: etag)
        }
        XCTAssertThrowsError(try bind(changedFrame(disabled: true)))
        XCTAssertThrowsError(try bind(changedFrame(tenant: UUID().uuidString.lowercased())))
        XCTAssertThrowsError(try bind(changedFrame(principal: UUID().uuidString.lowercased())))
        XCTAssertThrowsError(try bind(changedFrame(draft: NativeExhibitionDraft.new())))
        XCTAssertThrowsError(try bind(changedFrame(workspace: document.workspaceId)))
        XCTAssertThrowsError(try bind(changedFrame(workspace: UUID().uuidString.lowercased(), etag: document.etag)))
        XCTAssertThrowsError(try bind(changedFrame(workspace: document.workspaceId, etag: "stale")))
        _ = try bind(changedFrame(workspace: document.workspaceId, etag: document.etag))
        let clean = document
        document.etag = nil; XCTAssertThrowsError(try bind()); document = clean
        document.savedEditVersion = -1; XCTAssertThrowsError(try bind()); document = clean
        document.pending = .init(etag: clean.etag, draft: draft); XCTAssertThrowsError(try bind())
    }

    @MainActor
    func testNativeSDKTransportChecksSavedAuthorityHTTPAndCompleteReplyWithoutSaving() async throws {
        let base = try fixture(), owner = UUID().uuidString.lowercased(), epoch = UUID().uuidString.lowercased()
        let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\""
        let document = NativeDocumentState(format: 1, revision: 1, workspaceId: UUID().uuidString.lowercased(),
            tenantId: owner, principalId: owner, draft: base, etag: etag,
            savedEditVersion: base.document["editVersion"].integer, pending: nil)
        let ack = ViewportFrame(epoch: epoch, sequence: 3, tenantId: owner, principalId: owner, draft: base,
            selection: nil, mode: "translate", view: "isometric", disabled: false)
        let bound = try LightObjectRequest.nativeFrame(acknowledged: ack, document: document, ownerEpoch: epoch,
            acceptedSequence: 3, readOnly: false, boundedEdit: nil)
        let proposal = try LightObjectRequest(command: .object(["type": .string("set-light-fixture"),
            "lightId": .string(source), "visual": .null]), frame: bound)
        var expected = base.candidate, settings = try XCTUnwrap(base.candidate["extensions"][details]["lights"].items)
        settings[0]["fixture"] = .string("none"); expected["extensions"][details]["lights"] = .array(settings)
        expected["extensions"][visuals]["bindings"] = .array([base.candidate["extensions"][visuals]["bindings"].items![1]])
        expected = changed(expected)
        let saved = NativeStudioResponse(revision: 1, etag: etag, draft: base)
        defer { LightTransportProtocol.reset() }
        for scenario in ["valid", "unrelated-graph", "stale-response-etag", "dirty-document", "wrong-principal", "stale-saved", "missing-profile"] {
            var response = reply(proposal, candidate: expected)
            if scenario == "unrelated-graph" { response["candidate"]["title"] = .string("unrelated mutation") }
            try LightTransportProtocol.configure(owner: owner, reply: response,
                etag: scenario == "stale-response-etag" ? "stale" : etag, profile: scenario != "missing-profile")
            let api = try AuthoringAPI(port: 18091, tenantId: owner,
                nativeRuntimeManifestSHA256: "f27857e0acce017f8e9594b9b5cb6cba3a1fd9d12168cc000af8efa762f25f26",
                protocolClasses: [LightTransportProtocol.self])
            _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
                origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
            var input = document
            if scenario == "dirty-document" { input.savedEditVersion = -1 }
            if scenario == "wrong-principal" {
                input = NativeDocumentState(format: 1, revision: 1, workspaceId: document.workspaceId, tenantId: owner,
                    principalId: UUID().uuidString.lowercased(), draft: base, etag: etag,
                    savedEditVersion: document.savedEditVersion, pending: nil)
            }
            let comparison = scenario == "stale-saved" ? NativeStudioResponse(revision: 1, etag: "stale", draft: base) : saved
            do {
                let preview = try await api.prepareNativeLightObject(proposal, in: input, saved: comparison)
                XCTAssertEqual(scenario, "valid"); XCTAssertEqual(preview.candidate, expected)
                XCTAssertEqual(try preview.apply(to: base).candidate, expected)
            } catch { XCTAssertNotEqual(scenario, "valid", "\(error)") }
            let requests = LightTransportProtocol.requests()
            let admitted = ["valid", "unrelated-graph", "stale-response-etag"].contains(scenario)
            XCTAssertEqual(requests.map(\.httpMethod), admitted ? ["GET", "POST"] : ["GET"])
            if let post = requests.last, admitted {
                XCTAssertEqual(post.value(forHTTPHeaderField: "If-Match"), etag)
                XCTAssertEqual(post.value(forHTTPHeaderField: "x-csrf-token"), "synthetic-csrf")
                XCTAssertEqual(post.value(forHTTPHeaderField: "Origin"), "http://127.0.0.1:18091")
                XCTAssertTrue(post.url!.path.hasSuffix("/exhibitions/\(base.id)/native-authoring-proposal"))
                let body = try JSONDecoder().decode(DocumentValue.self, from: LightTransportProtocol.body())
                XCTAssertEqual(body["expectedRevision"], .integer(1)); XCTAssertEqual(body["request"], proposal.value)
            }
            XCTAssertEqual(document.draft, base); XCTAssertEqual(document.etag, etag); XCTAssertFalse(document.dirty)
            await api.endLocalAuthoringSession()
        }
    }

    func testArticulatedFixtureAndSettingsAllowOnlyTheServerSolvedSelectedTransform() throws {
        let base = try fixture()
        let binding: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeTrackSpotAim.profile),
            "mount": .object(["railPlacementId": .string(surface), "position": .vector([0, 0, 0]), "yaw": .integer(0)]),
            "aim": .object(["kind": .string("placement-center"), "placementId": .string(other), "offset": .vector([0.1, 0, 0])])])
        let command = try NativeTrackSpotAim.fixtureCommand(lightId: source, binding: binding)
        let request = try LightObjectRequest(command: command, frame: frame(base))
        let newAsset = "cccccccc-0000-4000-8000-000000000002"
        var expected = changed(base.candidate)
        var lights = expected["lights"].items!
        lights[0]["transform"]["position"] = .vector([0.2, 3.4, 0.1])
        expected["lights"] = .array(lights)
        var settings = expected["extensions"][details]["lights"].items!
        settings[0] = without(settings[0], "aim")
        settings[0]["fixture"] = .string("none")
        settings[0]["trackAim"] = binding
        expected["extensions"][details]["lights"] = .array(settings)
        expected["extensions"][attachments]["lights"] = .array([expected["extensions"][attachments]["lights"].items![1]])
        var assets = expected["extensions"][visuals]["assets"].items!
        assets.append(try SharedFurnishingCatalog.asset(catalogId: NativeTrackSpotAim.catalogue, id: newAsset))
        expected["extensions"][visuals]["assets"] = .array(assets)
        var bindings = expected["extensions"][visuals]["bindings"].items!
        bindings[0]["assetId"] = .string(newAsset)
        bindings[0]["pose"] = NativeTrackSpotAim.pose
        expected["extensions"][visuals]["bindings"] = .array(bindings)
        let entry = try request.checkedReply(reply(request, candidate: expected))
        XCTAssertEqual(entry.candidate, expected)

        var bad = expected
        lights[1]["intensity"] = .integer(500)
        bad["lights"] = .array(lights)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: bad)))
        bad = expected
        bad["extensions"][details]["lights"] = base.candidate["extensions"][details]["lights"]
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: bad)))
        bad = expected
        lights = expected["lights"].items!
        lights[0]["transform"]["rotation"] = .vector([0, 0, 0, 2])
        bad["lights"] = .array(lights)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: bad)))

        let attached = try entry.apply(to: base)
        var manual = binding
        manual["aim"] = .object(["kind": .string("manual"), "yaw": .integer(0), "pitch": .number(-1)])
        let update = try NativeTrackSpotAim.updateCommand(in: attached, lightId: source, binding: manual,
            shadowPriority: other, prefersShadow: true)
        XCTAssertEqual(update["light"]["targetPlacementId"], .string(other))
        XCTAssertEqual(update["settings"]["trackAim"]["aim"]["placementId"], .null)
        let updateRequest = try LightObjectRequest(command: update, frame: frame(attached))
        settings[0]["trackAim"] = manual
        expected["extensions"][details]["lights"] = .array(settings)
        lights = expected["lights"].items!
        lights[0]["targetPlacementId"] = .string(other)
        lights[0]["castsShadow"] = .bool(true)
        lights[0]["transform"]["position"] = .vector([0.2, 3.35, 0.12])
        expected["lights"] = .array(lights)
        XCTAssertEqual(try updateRequest.checkedReply(reply(updateRequest, candidate: expected)).candidate, expected)
        XCTAssertThrowsError(try makeRequest("set-light-fixture", attached, fields: ["visual": .null]))
        var missingBinding = command
        missingBinding["visual"] = without(command["visual"], "trackAim")
        XCTAssertThrowsError(try LightObjectRequest(command: missingBinding, frame: frame(base)))
    }

    @MainActor
    func testArticulatedDiagnosticRequiresExactCapabilityAndArticulatedProposal() async throws {
        let base = try fixture()
        let owner = UUID().uuidString.lowercased()
        let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\""
        let document = NativeDocumentState(format: 1, revision: 1, workspaceId: UUID().uuidString.lowercased(),
            tenantId: owner, principalId: owner, draft: base, etag: etag,
            savedEditVersion: base.document["editVersion"].integer, pending: nil)
        let scope = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: owner,
            principalId: owner, draft: base, selection: nil, mode: "translate", view: "isometric", disabled: false,
            workspaceId: document.workspaceId, baseETag: etag)
        let binding: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeTrackSpotAim.profile),
            "mount": .object(["railPlacementId": .string(surface), "position": .vector([0, 0, 0]), "yaw": .integer(0)]),
            "aim": .object(["kind": .string("manual"), "yaw": .integer(0), "pitch": .number(0.1)])])
        let articulated = try NativeTrackSpotAim.fixtureCommand(lightId: source, binding: binding)
        let ordinary: DocumentValue = .object(["type": .string("set-light-fixture"), "lightId": .string(source), "visual": .null])
        let error: DocumentValue = .object(["code": .string("AUTHORING_PROPOSAL_INVALID"),
            "message": .string("TRACK_SPOT_AIM_OUTSIDE_ENVELOPE: Choose joint angles inside the verified source envelope.")])
        defer { LightTransportProtocol.reset() }
        for (profile, command, expectedRequests, hasDetail) in [
            (nil as String?, articulated, ["GET"], false),
            ("unknown", articulated, ["GET"], false),
            (NativeTrackSpotAim.profile, articulated, ["GET", "POST"], true),
            (NativeTrackSpotAim.profile, ordinary, ["GET", "POST"], false),
        ] {
            try LightTransportProtocol.configure(owner: owner, reply: error, etag: etag, profile: true,
                                                 aimProfile: profile, postStatus: 422)
            let api = try AuthoringAPI(port: 18091, tenantId: owner, nativeRuntimeManifestSHA256: String(repeating: "b", count: 64),
                                       protocolClasses: [LightTransportProtocol.self])
            _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
                origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
            let proposal = try LightObjectRequest(command: command, frame: scope)
            do {
                _ = try await api.prepareNativeLightObject(proposal, in: document, saved: .init(revision: 1, etag: etag, draft: base))
                XCTFail("Expected the proposal to be refused")
            } catch let failure as AuthoringFailure {
                XCTAssertEqual(failure.trackSpotAimDiagnostic?.reason, hasDetail ? "TRACK_SPOT_AIM_OUTSIDE_ENVELOPE" : nil)
                XCTAssertEqual(failure.code, expectedRequests.count == 1 ? "NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE" : "AUTHORING_PROPOSAL_INVALID")
            }
            XCTAssertEqual(LightTransportProtocol.requests().map { $0.httpMethod ?? "" }, expectedRequests)
        }
    }

    func testCLIEmitsOnlyExactTrackAimDiagnosticPairs() throws {
        let reason = "TRACK_SPOT_AIM_OUTSIDE_ENVELOPE"
        let remedy = "Choose joint angles inside the verified source envelope."
        let issue = try XCTUnwrap(NativeTrackSpotAimDiagnostic.parse(reason + ": " + remedy))
        let code = "AUTHORING_PROPOSAL_INVALID"
        XCTAssertEqual(NativeAutomationFailure.value(AuthoringFailure(code, status: 422, trackSpotAimDiagnostic: issue)),
            .object(["code": .string(code), "trackAim": .object(["reason": .string(reason), "remedy": .string(remedy)])]))
        XCTAssertNil(NativeTrackSpotAimDiagnostic.parse(reason + ": arbitrary transport text"))
        XCTAssertNil(NativeTrackSpotAimDiagnostic.parse("TRACK_SPOT_AIM_UNKNOWN: " + remedy))
        XCTAssertEqual(NativeAutomationFailure.value(AuthoringFailure(code, status: 422)), .object(["code": .string(code)]))
    }

    func testAuthoredShadowUsesExistingSetObjectAndPreservesTransportFences() throws {
        let base = try fixture()
        let settings = without(base.candidate["extensions"][details]["lights"].items![0], "lightId")
        var light = without(base.candidate["lights"].items![0], "id")
        light["castsShadow"] = .bool(true)
        let request = try makeRequest("set-light-object", base, fields: ["light": light, "settings": settings])
        var expected = base.candidate, rows = expected["lights"].items!
        rows[0]["castsShadow"] = .bool(true); expected["lights"] = .array(rows); expected = changed(expected)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected)).candidate, expected)
        for variant in 0..<4 {
            var bad = light
            switch variant {
            case 0: bad["castsShadow"] = .string("true")
            case 1: bad["roomId"] = .string(other)
            case 2: bad["transform"]["scale"] = .vector([2,2,2])
            default: bad = without(bad, "beamAngle"); bad["type"] = .string("point")
            }
            XCTAssertThrowsError(try makeRequest("set-light-object", base, fields: ["light": bad, "settings": settings]))
        }
        for type in ["point", "area"] {
            var legacy = base, candidate = base.candidate, legacyRows = candidate["lights"].items!
            var row = without(legacyRows[0], "beamAngle")
            row["type"] = .string(type); row["unit"] = .string(type == "area" ? "lumen" : "candela")
            row["castsShadow"] = .bool(true)
            if type == "area" { row["dimensions"] = .object(["width": .number(1), "height": .number(1)]) }
            legacyRows[0] = row; candidate["lights"] = .array(legacyRows); try legacy.replaceCandidate(candidate)
            var proposed = without(row, "id"); proposed["intensity"] = .integer(21)
            XCTAssertNoThrow(try makeRequest("set-light-object", legacy, fields: ["light": proposed, "settings": settings]))
            proposed["castsShadow"] = .bool(false)
            XCTAssertNoThrow(try makeRequest("set-light-object", legacy, fields: ["light": proposed, "settings": settings]))
            proposed["castsShadow"] = .bool(true)
            legacyRows[0]["castsShadow"] = .bool(false); candidate["lights"] = .array(legacyRows); try legacy.replaceCandidate(candidate)
            XCTAssertThrowsError(try makeRequest("set-light-object", legacy, fields: ["light": proposed, "settings": settings]))
        }
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

    func testRoomLocalDefaultPreservesRetainedUpperFraming() throws {
        var draft = try NativeExhibitionDraft.new(roomBaseLight: true, defaultCeilingFill: true)
        var candidate = draft.candidate
        let baseLight = try XCTUnwrap(candidate["lights"].items?.first)
        let lightID = try XCTUnwrap(baseLight["id"].string)
        var spot = try XCTUnwrap(fixture(visual: false).candidate["lights"].items?.last)
        spot["roomId"] = baseLight["roomId"]
        candidate["lights"] = .array([baseLight, spot])
        var detailsRows = try XCTUnwrap(candidate["extensions"][details]["lights"].items)
        let profile: DocumentValue = .object(["version": .integer(1), "kind": .string("upper-framing"),
            "cutoffAngle": .number(0.16), "softnessAngle": .number(0.25)])
        let aim: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeTrackSpotAim.profile),
            "mount": .object(["railPlacementId": .string(surface), "position": .vector([0, 0, 0]), "yaw": .integer(0)]),
            "aim": .object(["kind": .string("placement-center"), "placementId": .string(clone), "offset": .vector([0, 0, 0])])])
        detailsRows.append(.object(["lightId": .string(other), "fixture": .string("none"), "trackAim": aim, "angularProfile": profile]))
        candidate["extensions"][details]["lights"] = .array(detailsRows)
        try draft.replaceCandidate(candidate)

        let command: DocumentValue = .object(["type": .string("set-room-ceiling-fill-brightness"), "lightId": .string(lightID),
            "brightness": .number(0.5), "mode": .string("default"), "spaceLightingMode": RoomCeilingFill.roomLocalMode])
        let request = try LightObjectRequest(command: command, frame: frame(draft))
        let expected = SpaceLightingFormBuffer.addingRoomLocal(to: candidate)
        XCTAssertNotEqual(expected, candidate)
        XCTAssertEqual(try request.checkedReply(reply(request, candidate: expected, lightId: lightID)).apply(to: draft).candidate, expected)

        var tampered = expected, tamperedRows = detailsRows
        tamperedRows[1]["angularProfile"]["cutoffAngle"] = .number(0.17)
        tampered["extensions"][details]["lights"] = .array(tamperedRows)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: tampered, lightId: lightID)))

        var injected = command
        injected["angularProfile"] = profile
        XCTAssertThrowsError(try LightObjectRequest(command: injected, frame: frame(draft)))
        XCTAssertThrowsError(try LightObjectRequest(command: .object(["type": .string("remove-light-object"), "lightId": .string(other)]), frame: frame(draft)))
        var outOfRange = detailsRows[1], unexpectedField = detailsRows[1]
        outOfRange["angularProfile"]["softnessAngle"] = .number(0)
        unexpectedField["angularProfile"]["unsupported"] = .bool(true)
        let malformedRows = [without(detailsRows[1], "trackAim"), outOfRange, unexpectedField]
        for malformed in malformedRows {
            var invalid = draft, invalidCandidate = candidate
            invalidCandidate["extensions"][details]["lights"] = .array([detailsRows[0], malformed])
            try invalid.replaceCandidate(invalidCandidate)
            XCTAssertThrowsError(try LightObjectRequest(command: command, frame: frame(invalid)))
        }
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
    private func legacyPointAndSupplementalFill() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        let room = draft.rooms[0], roomId = room["id"]
        let point: DocumentValue = .object(["id": .string(source), "roomId": roomId, "type": .string("point"),
            "unit": .string("candela"), "intensity": .number(16), "color": .vector([1, 1, 1]),
            "castsShadow": .bool(false), "transform": NativeExhibitionDraft.identity])
        var area = try RoomCeilingFill.emitter(dimensions: RoomCeilingFill.dimensions(in: candidate, room: room), brightness: 0.5)
        area["id"] = .string(other); area["roomId"] = roomId; area["type"] = .string("area")
        area["unit"] = .string("lumen"); area["color"] = .vector([1, 1, 1]); area["castsShadow"] = .bool(false)
        candidate["lights"] = .array([point, area])
        candidate["extensions"][details] = .object(["version": .integer(1), "lights": .array([
            .object(["lightId": .string(source), "fixture": .string("none"), "purpose": .string("room-base"), "range": .number(10)]),
            .object(["lightId": .string(other), "fixture": .string("none"), "purpose": .string("room-supplemental-fill"),
                "basePlacement": .string("auto"), "ceilingFill": .object(["version": .integer(1), "brightness": .number(0.5)])])])])
        candidate["extensions"][template] = .object(["modified": .bool(false)])
        candidate["extensions"]["example.unrelated/keep"] = .object(["value": .string("untouched")])
        try draft.replaceCandidate(candidate); return draft
    }
    private func deletingPoint(_ base: NativeExhibitionDraft, promote: Bool = true) -> DocumentValue {
        var candidate = base.candidate
        candidate["lights"] = .array(base.candidate["lights"].items!.filter { $0["id"] != .string(source) })
        var rows = base.candidate["extensions"][details]["lights"].items!.filter { $0["lightId"] != .string(source) }
        if promote, let i = rows.firstIndex(where: { $0["lightId"] == .string(other) }) { rows[i]["purpose"] = .string("room-base") }
        candidate["extensions"][details]["lights"] = .array(rows)
        return changed(candidate)
    }
    func testLegacyBasePointRemovalPromotesOnlyCanonicalAreaPurpose() throws {
        let base = try legacyPointAndSupplementalFill(), request = try makeRequest("remove-light-object", base)
        let expected = deletingPoint(base), preview = try request.checkedReply(reply(request, candidate: expected))
        XCTAssertEqual(preview.candidate, expected)
        XCTAssertEqual(preview.candidate["lights"].items, [base.candidate["lights"].items![1]])
        XCTAssertEqual(preview.candidate["extensions"][details]["lights"].items?.first?["ceilingFill"],
                       base.candidate["extensions"][details]["lights"].items?[1]["ceilingFill"])
        var altered = expected, lights = altered["lights"].items!
        lights[0]["intensity"] = .number(1); altered["lights"] = .array(lights)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: altered)))
        altered = expected; var rows = altered["extensions"][details]["lights"].items!
        rows[0]["ceilingFill"]["brightness"] = .number(0.4); altered["extensions"][details]["lights"] = .array(rows)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: altered)))
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: deletingPoint(base, promote: false))))
    }
    func testLegacyPointRemovalRejectsAmbiguousManualDimmedAndNoncanonicalPromotion() throws {
        for mode in ["ambiguous", "manual", "dimmed", "noncanonical"] {
            var base = try legacyPointAndSupplementalFill(), candidate = base.candidate
            var rows = candidate["extensions"][details]["lights"].items!, lights = candidate["lights"].items!
            if mode == "ambiguous" {
                var extra = rows[1]; extra["lightId"] = .string(clone); rows.append(extra)
                var area = lights[1]; area["id"] = .string(clone); lights.append(area)
            }
            if mode == "manual" { rows[1]["basePlacement"] = .string("manual") }
            if mode == "dimmed" { rows[1]["ceilingFill"]["brightness"] = .number(0.4) }
            if mode == "noncanonical" { lights[1]["intensity"] = .number(1) }
            candidate["extensions"][details]["lights"] = .array(rows); candidate["lights"] = .array(lights)
            try base.replaceCandidate(candidate)
            do {
                let request = try makeRequest("remove-light-object", base)
                XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: deletingPoint(base))), mode)
            } catch { /* Refusal during typed request validation is also closed. */ }
        }
    }
    func testLegacyPointRemovalWithoutSupplementalCreatesNoReplacementLight() throws {
        var base = try legacyPointAndSupplementalFill(), candidate = base.candidate
        candidate["lights"] = .array([candidate["lights"].items![0]])
        candidate["extensions"][details]["lights"] = .array([candidate["extensions"][details]["lights"].items![0]])
        try base.replaceCandidate(candidate)
        let request = try makeRequest("remove-light-object", base)
        let preview = try request.checkedReply(reply(request, candidate: deletingPoint(base, promote: false)))
        XCTAssertEqual(preview.candidate["lights"], .array([]))
        XCTAssertEqual(preview.candidate["extensions"][details]["lights"], .array([]))
    }

}

private final class LightTransportProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var session = Data(), reply = Data(), etag = ""
    nonisolated(unsafe) private static var observed: [URLRequest] = []
    nonisolated(unsafe) private static var postedBody = Data()
    nonisolated(unsafe) private static var postStatus = 200
    static func configure(owner: String, reply value: DocumentValue, etag responseETag: String, profile: Bool, aimProfile: String? = nil,
                          fitProfile: String? = nil, postStatus: Int = 200) throws {
        var fields = ["tenantId": owner, "userId": owner, "role": "owner", "csrfToken": "synthetic-csrf",
            "expiresAt": "2099-01-01T00:00:00Z", "nativeSceneAuthoringProposalProfile": "saved-head-scene-v1"]
        if profile { fields["nativeLightObjectAuthoringProposalProfile"] = LightObjectAuthoring.profile }
        fields["nativeTrackSpotAimProfile"] = aimProfile
        fields["nativeSpotBeamFitProfile"] = fitProfile
        let sessionData = try JSONSerialization.data(withJSONObject: fields), replyData = try JSONEncoder().encode(value)
        lock.lock(); defer { lock.unlock() }
        session = sessionData; reply = replyData; etag = responseETag; observed = []; postedBody = Data(); self.postStatus = postStatus
    }
    static func reset() { lock.lock(); defer { lock.unlock() }; session = Data(); reply = Data(); observed = []; postedBody = Data() }
    static func requests() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return observed }
    static func body() -> Data { lock.lock(); defer { lock.unlock() }; return postedBody }
    private static func response(_ request: URLRequest) -> (Data, String, Int) {
        lock.lock(); defer { lock.unlock() }; observed.append(request)
        if request.httpMethod == "GET" { return (session, etag, 200) }
        postedBody = request.httpBody ?? Data()
        if let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let count = stream.read(&buffer, maxLength: buffer.count)
                guard count > 0 else { break }; postedBody.append(contentsOf: buffer.prefix(count))
            }
        }
        return (reply, etag, postStatus)
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let (data, etag, statusCode) = Self.response(request)
        let response = HTTPURLResponse(url: request.url!, statusCode: statusCode, httpVersion: "HTTP/1.1",
            headerFields: ["Content-Type": "application/json", "ETag": etag])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
