// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeModelPreparationTests: XCTestCase {
    func testRequiredReadyWireAndTamperedReceipt() throws {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_REQUIRED_MODEL_READY_FIXTURE"] else {
            throw XCTSkip("Set the synthetic Platform 045 fixture path for this paired contract check")
        }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
        let ready = try JSONDecoder().decode(NativeImportJob.self, from: bytes)
        let status = try XCTUnwrap(ready.modelPreparation)
        let receipt = try XCTUnwrap(status.serverReceipt)
        let request = NativeImportIntent(artworkId: receipt.artworkId, idempotencyKey: status.intent.operationID,
            mime: status.original.mime, sha256: status.original.sha256, bytes: status.original.bytes,
            scaleMeters: 1, rights: .init(), modelPreparation: status.intent)
        XCTAssertTrue(status.matches(request, job: ready))
        XCTAssertTrue(ready.confirmsIntakeDrain(for: request))
        let restored = try JSONDecoder().decode(NativeImportIntent.self, from: JSONEncoder().encode(request))
        XCTAssertEqual(restored, request)

        var response = try JSONDecoder().decode(DocumentValue.self, from: bytes)
        response["modelPreparation"]["serverReceipt"]["generation"] = .string("another-generation")
        let tampered = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
        XCTAssertFalse(tampered.confirmsIntakeDrain(for: request))
        response = try JSONDecoder().decode(DocumentValue.self, from: bytes)
        response["modelPreparation"]["workerDrained"] = .bool(false)
        let draining = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
        XCTAssertFalse(draining.confirmsIntakeDrain(for: request))
    }
}
