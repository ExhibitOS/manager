// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class SurfaceMaterialTextureRequestTests: XCTestCase {
    private let surface = "aaaaaaaa-0000-4000-8000-000000000001"
    private let other = "bbbbbbbb-0000-4000-8000-000000000002"
    private let tenant = "cccccccc-0000-4000-8000-000000000003"
    private let principal = "dddddddd-0000-4000-8000-000000000004"
    private let workspace = "eeeeeeee-0000-4000-8000-000000000005"
    private let etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
    private func fixture() throws -> (SurfaceMaterialTextureRequest, DocumentValue, NativeDocumentState) {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["surfaces"] = .array([surface, other].map { .object(["id": .string($0), "roomId": draft.rooms[0]["id"], "type": .string("wall"), "dimensions": .object(["width": .integer(10), "height": .integer(4)]), "transform": NativeExhibitionDraft.identity]) })
        let scalar = try SurfaceMaterial(color: "#aabbcc", roughness: 0.7, metalness: 0).value
        candidate["extensions"] = .object(["opaque": .string(surface), SurfaceMaterialCommand.namespace: .object([
            "version": .integer(1), "surfaces": .object([surface: scalar, other: scalar])])])
        try draft.replaceCandidate(candidate)
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: tenant, principalId: principal,
            draft: draft, selection: ViewportSelection(kind: "surface", id: surface), mode: "translate", view: "isometric", disabled: false, workspaceId: workspace, baseETag: etag)
        let texture = try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.concrete, repeatMeters: [2, 2], rotation: 0, normalScale: 1)
        let request = try SurfaceMaterialTextureRequest(frame: frame, input: .object(["type": .string("set-material-texture"), "surfaceId": .string(surface), "texture": texture.value]))
        candidate["extensions"][SurfaceMaterialCommand.namespace]["surfaceTextures"] = .object([surface: texture.value])
        var reply: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { reply[key] = request.value[key] }
        reply["surfaceId"] = .string(surface); reply["candidate"] = candidate
        let state = NativeDocumentState(format: 1, revision: 5, workspaceId: workspace, tenantId: tenant, principalId: principal,
            draft: draft, etag: etag, savedEditVersion: draft.document["editVersion"].integer)
        return (request, .object(reply), state)
    }
    func testOneWallDomainPreservesExistingScalarsAndOpaqueBody() throws {
        let (request, reply, state) = try fixture()
        XCTAssertEqual(request.value["material"], state.draft!.candidate["extensions"][SurfaceMaterialCommand.namespace]["surfaces"][surface])
        let applied = try request.applyReply(reply, to: state)
        XCTAssertEqual(applied.candidate["extensions"][SurfaceMaterialCommand.namespace]["surfaces"], state.draft!.candidate["extensions"][SurfaceMaterialCommand.namespace]["surfaces"])
        XCTAssertEqual(applied.candidate["extensions"]["opaque"], .string(surface))
        XCTAssertEqual(applied.document["editVersion"].integer, state.savedEditVersion! + 1)
        XCTAssertEqual(request.command.texture?.catalogId, SurfaceMaterialTextureBinding.concrete)
    }
    func testWrongCorrelationOtherWallAndExtraCandidateChangeRefuse() throws {
        let (request, reply, state) = try fixture()
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "surfaceId"] {
            var changed = reply; changed[key] = .string("stale")
            XCTAssertThrowsError(try request.applyReply(changed, to: state))
        }
        var changed = reply; changed["extra"] = .bool(true)
        XCTAssertThrowsError(try request.applyReply(changed, to: state))
        changed = reply; changed["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try request.applyReply(changed, to: state))
        changed = reply; changed["candidate"]["extensions"][SurfaceMaterialCommand.namespace]["surfaces"][other]["color"] = .string("#000000")
        XCTAssertThrowsError(try request.applyReply(changed, to: state))
        var stale = state; stale.etag = "\"studio-r3-" + String(repeating: "b", count: 64) + "\""
        XCTAssertThrowsError(try request.applyReply(reply, to: stale))
        stale = state; stale.pending = .init(etag: state.etag, draft: state.draft!)
        XCTAssertThrowsError(try request.applyReply(reply, to: stale))
    }
    func testCallerCannotChangeScalarOrChooseFloorThroughTextureInput() throws {
        let (request, _, _) = try fixture()
        var input: DocumentValue = .object(["type": .string("set-material-texture"), "surfaceId": .string(surface), "texture": request.command.texture!.value, "material": .null])
        XCTAssertThrowsError(try SurfaceMaterialTextureRequest(frame: request.frame, input: input))
        input = .object(["type": .string("set-material-texture"), "surfaceId": .string(other), "texture": request.command.texture!.value])
        XCTAssertThrowsError(try SurfaceMaterialTextureRequest(frame: request.frame, input: input))
    }
    func testMissingBoundedPolicyOrStaleSavedETagNeverAdmitsMutation() throws {
        let (_, _, state) = try fixture()
        let base: DocumentValue = .object(["exhibitionId": .string(state.draft!.id), "revision": .integer(2), "etag": .string(etag),
            "boundedReadOnly": .bool(false), "boundedProfile": .bool(true), "boundedEditRequired": .bool(false)])
        func policy(_ value: DocumentValue) throws -> NativeViewportPolicy { try JSONDecoder().decode(NativeViewportPolicy.self, from: JSONEncoder().encode(value)) }
        try SurfaceMaterialTextureRequest.requireEditingPolicy(policy(base), document: state)
        for key in ["boundedProfile", "boundedEditRequired"] {
            var changed = base; changed[key] = .null
            XCTAssertThrowsError(try SurfaceMaterialTextureRequest.requireEditingPolicy(policy(changed), document: state))
        }
        for key in ["boundedReadOnly", "boundedEditRequired"] {
            var changed = base; changed[key] = .bool(true)
            XCTAssertThrowsError(try SurfaceMaterialTextureRequest.requireEditingPolicy(policy(changed), document: state))
        }
        var changed = base; changed["etag"] = .string("other")
        XCTAssertThrowsError(try SurfaceMaterialTextureRequest.requireEditingPolicy(policy(changed), document: state))
    }
}
