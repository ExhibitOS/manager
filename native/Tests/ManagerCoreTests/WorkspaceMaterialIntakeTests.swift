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
}
