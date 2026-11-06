// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceMaterialIntakeTests: XCTestCase {
    func testIntakeResponseCannotChangeRoleBytesOrRequestIdentity() throws {
        let requestId = UUID().uuidString.lowercased()
        let intakeId = UUID().uuidString.lowercased()
        let sha = String(repeating: "a", count: 64)
        let map = try JSONDecoder().decode(WorkspaceMaterialMap.self, from: Data("""
            {"mime":"image/png","bytes":128,"sha256":"\(sha)","width":4,"height":4,"colorSpace":"srgb"}
            """.utf8))
        let declaration = WorkspaceMaterialDeclaration(requestId: requestId, kind: .floor,
            name: "Fixture", rights: ArtworkMetadata().rights, provenance: "Synthetic input",
            maps: ["color": map])
        func status(request: String, role: String, bytes: Int) throws -> WorkspaceMaterialIntake {
            try JSONDecoder().decode(WorkspaceMaterialIntake.self, from: Data("""
                {"intakeId":"\(intakeId)","requestId":"\(request)","kind":"floor","phase":"uploading",
                "received":{"\(role)":{"bytes":\(bytes),"sha256":"\(sha)"}},"retryable":false,"error":null}
                """.utf8))
        }
        XCTAssertNoThrow(try status(request: requestId, role: "color", bytes: 128).check(declaration: declaration))
        XCTAssertThrowsError(try status(request: requestId, role: "color", bytes: 129).check(declaration: declaration))
        XCTAssertThrowsError(try status(request: requestId, role: "normal", bytes: 128).check(declaration: declaration))
        XCTAssertThrowsError(try status(request: UUID().uuidString.lowercased(), role: "color", bytes: 128).check(declaration: declaration))
    }

    func testUnknownRuntimeRejectsIntakeBeforeRequest() async throws {
        let api = try AuthoringAPI(port: 65535, tenantId: UUID().uuidString,
            nativeRuntimeManifestSHA256: String(repeating: "0", count: 64))
        do {
            _ = try await api.workspaceMaterialIntake(UUID().uuidString)
            XCTFail("Unknown runtime admitted the workspace route")
        } catch let error as AuthoringFailure {
            XCTAssertEqual(error.code, "WORKSPACE_MATERIAL_UNAVAILABLE")
        }
    }

    func testExactValidationManifestStillRequiresAuthenticatedNativeCapability() async throws {
        let tenantId = UUID().uuidString
        let manifest = "40ca49e65e769c4c034eea69300ff65ce8af46ef9b081a9a2b4b1ade12ed9415"
        let api = try AuthoringAPI(port: 65535, tenantId: tenantId, nativeRuntimeManifestSHA256: manifest)
        #if DEBUG
        XCTAssertTrue(api.workspaceMaterialRuntimeQualified)
        #else
        XCTAssertFalse(api.workspaceMaterialRuntimeQualified)
        #endif
        let changed = try AuthoringAPI(port: 65535, tenantId: tenantId,
            nativeRuntimeManifestSHA256: String(manifest.dropLast()) + "0")
        XCTAssertFalse(changed.workspaceMaterialRuntimeQualified)
        let admitted = await api.admitsWorkspaceMaterials
        XCTAssertFalse(admitted)
    }

    func testOnlyDefinitiveCreateRejectionsReleasePendingDeclaration() {
        let declaration = WorkspaceMaterialDeclaration(requestId: UUID().uuidString.lowercased(), kind: .floor,
            name: "Fixture", rights: ArtworkMetadata().rights, provenance: "Synthetic input", maps: [:])

        XCTAssertNil(declaration.pendingAfterCreateFailure(AuthoringFailure("MATERIAL_DECLARATION")))
        XCTAssertNil(declaration.pendingAfterCreateFailure(AuthoringFailure("MATERIAL_DECLARATION", status: 400)))
        XCTAssertNil(declaration.pendingAfterCreateFailure(AuthoringFailure("MATERIAL_INTAKE_LIMIT", status: 409)))
        XCTAssertEqual(declaration.pendingAfterCreateFailure(AuthoringFailure("MATERIAL_INTAKE_LIMIT", status: 500)), declaration)
        XCTAssertEqual(declaration.pendingAfterCreateFailure(AuthoringFailure("JOB_STATE", status: 409)), declaration)
        XCTAssertEqual(declaration.pendingAfterCreateFailure(AuthoringFailure("CONNECTION")), declaration)
    }

    func testWorkspaceFloorBindingKeepsOtherFloorsAndExtensionsAndIsIdempotent() throws {
        let target = UUID().uuidString.lowercased(), other = UUID().uuidString.lowercased(), wall = UUID().uuidString.lowercased()
        let first = try floorMaterialFixture(), replacement = try floorMaterialFixture()
        let namespace = "org.exhibitos.studio/workspace-materials"
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        candidate["surfaces"] = .array([target, other, wall].map { id in
            .object(["id": .string(id), "type": .string(id == wall ? "wall" : "floor")])
        })
        let sound: DocumentValue = .object(["entrance": .string("concrete")])
        candidate["extensions"] = .object(["org.exhibitos.viewer/experience": sound])
        try draft.replaceCandidate(candidate)
        func binding(_ surface: String, _ material: WorkspaceMaterial) throws -> WorkspaceFloorMaterialBinding {
            try WorkspaceFloorMaterialBinding(surfaceId: surface, materialId: material.id, materialVersion: 1,
                                               repeatMeters: [2, 2], rotation: 0)
        }
        let one = try XCTUnwrap(binding(target, first).applying(first, to: draft))
        let both = try XCTUnwrap(binding(other, first).applying(first, to: one))
        let selected = try binding(target, replacement)
        let changed = try XCTUnwrap(selected.applying(replacement, to: both))
        let saved = changed.candidate["extensions"][namespace]
        XCTAssertEqual(saved["bindings"].items?.map { $0["materialId"].string }, [replacement.id, first.id])
        XCTAssertEqual(saved["materials"].items?.count, 2)
        XCTAssertEqual(changed.candidate["extensions"]["org.exhibitos.viewer/experience"], sound)
        XCTAssertEqual(changed.candidate["surfaces"], draft.candidate["surfaces"])
        XCTAssertNil(try selected.applying(replacement, to: changed))
        XCTAssertThrowsError(try binding(wall, replacement).applying(replacement, to: both))
        XCTAssertThrowsError(try selected.applying(first, to: both))
    }

    func testWorkspaceFloorBindingRejectsNonFiniteOrOutOfRangeValues() throws {
        let target = UUID().uuidString.lowercased(), material = try floorMaterialFixture()
        XCTAssertThrowsError(try WorkspaceFloorMaterialBinding(surfaceId: target, materialId: material.id, materialVersion: 1,
                                                               repeatMeters: [.nan, 2], rotation: 0))
        XCTAssertThrowsError(try WorkspaceFloorMaterialBinding(surfaceId: target, materialId: material.id, materialVersion: 1,
                                                               repeatMeters: [2, 2], rotation: 4))
    }

    private func floorMaterialFixture() throws -> WorkspaceMaterial {
        let id = UUID().uuidString.lowercased(), sha = String(repeating: "a", count: 64)
        return try JSONDecoder().decode(WorkspaceMaterial.self, from: Data("""
            {"id":"\(id)","version":1,"kind":"floor","name":"Fixture","provenance":"Synthetic",
             "rights":{"holder":"Fixture","ownership":"owner","licenseId":"CC0-1.0","creditLine":"",
             "permissions":{"display":true,"download":true,"export":true,"commercial":true}},
             "maps":{"color":{"mime":"image/png","bytes":128,"sha256":"\(sha)","width":4,"height":4,"colorSpace":"srgb"}}}
            """.utf8))
    }
}
