// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeCompactModelVariantTests: XCTestCase {
    private let artwork = "10000000-0000-4000-8000-000000000001"
    private let source = "10000000-0000-4000-8000-000000000002"
    private let coarse = "10000000-0000-4000-8000-000000000003"
    private let job = "10000000-0000-4000-8000-000000000004"
    private let operation = "10000000-0000-4000-8000-000000000005"
    private let fullHash = String(repeating: "a", count: 64)
    private let coarseHash = String(repeating: "b", count: 64)
    private let proof = String(repeating: "c", count: 64)

    private var input: NativeCompactModelVariantRequest {
        .init(revision: 2, assetId: source, sha256: fullHash, bytes: 16_036_864,
              idempotencyKey: operation, generation: "generation-1")
    }

    private func body() throws -> DocumentValue {
        let recipe = NativeModelPreparationIntent.profile
        let recipeHash = NativeModelPreparationIntent.recipeSHA256
        return try JSONDecoder().decode(DocumentValue.self, from: Data("""
        {
          "id":"\(job)","state":"approved","progress":100,"attempts":1,"assetId":"\(coarse)",
          "modelPreparation":{
            "version":1,"purpose":"approved-full-compact","profileId":"\(recipe)",
            "operationID":"\(operation)","generation":"generation-1","artworkRevision":2,
            "required":true,"status":"ready","workerDrained":true,
            "sourceFull":{"assetId":"\(source)","mime":"model/gltf-binary","sha256":"\(fullHash)","bytes":16036864},
            "coarse":{"assetId":"\(coarse)","mime":"model/gltf-binary","sha256":"\(coarseHash)","bytes":1874704},
            "serverReceipt":{
              "version":1,"purpose":"approved-full-compact","profileId":"\(recipe)",
              "publicationProfile":"exhibitos-budget-geometry-pair-v1","recipeSHA256":"\(recipeHash)",
              "jobId":"\(job)","artworkId":"\(artwork)","artworkRevision":2,"operationID":"\(operation)",
              "generation":"generation-1","rightsSHA256":"\(proof)","approvalSHA256":"\(proof)",
              "sourceFull":{"assetId":"\(source)","mime":"model/gltf-binary","sha256":"\(fullHash)","bytes":16036864},
              "coarse":{"assetId":"\(coarse)","mime":"model/gltf-binary","sha256":"\(coarseHash)","bytes":1874704},
              "producer":{
                "version":1,"recipeId":"\(recipe)","recipeSHA256":"\(recipeHash)","decoder":"fixture",
                "source":{"sha256":"\(fullHash)","bytes":16036864,"triangles":100000,"vertices":86202,"bounds":{"min":[0,0,0],"max":[1,1,1]}},
                "output":{"sha256":"\(coarseHash)","bytes":1874704,"triangles":20000,"vertices":14734,
                          "textureEdge":512,"reusedOriginal":false,"bounds":{"min":[0,0,0],"max":[1,1,1]}},
                "images":[],"simplifier":{"version":"1.1.1","relativeAppearanceError":0.006,"lockedVertices":8}
              },
              "sha256":"\(proof)"
            }
          }
        }
        """.utf8))
    }

    private func decode(_ value: DocumentValue) throws -> NativeCompactModelVariantJob {
        try JSONDecoder().decode(NativeCompactModelVariantJob.self, from: JSONEncoder().encode(value))
    }

    func testReadyRequiresSeparateSourceAndCoarseBoundToProducerAndWorkerDrain() throws {
        let ready = try body()
        let decoded = try decode(ready)
        try decoded.check(request: input, artworkId: artwork)
        XCTAssertEqual(decoded.automationValue["modelPreparation"]["sourceFull"]["assetId"].string, source)
        XCTAssertEqual(decoded.automationValue["modelPreparation"]["coarse"]["assetId"].string, coarse)

        var changed = ready
        changed["modelPreparation"]["workerDrained"] = .bool(false)
        XCTAssertThrowsError(try decode(changed).check(request: input, artworkId: artwork))
        changed = ready
        changed["modelPreparation"]["serverReceipt"]["producer"]["output"]["bounds"]["max"] = .vector([1, 1, 2])
        XCTAssertThrowsError(try decode(changed).check(request: input, artworkId: artwork))
        changed = ready
        changed["modelPreparation"]["serverReceipt"]["sourceFull"]["sha256"] = .string(coarseHash)
        XCTAssertThrowsError(try decode(changed).check(request: input, artworkId: artwork))
        XCTAssertThrowsError(try decode(ready).check(request: input, artworkId: source))
    }

    func testCompactResponseDoesNotDecodeAsOrdinaryRequiredOriginal() throws {
        let ready = try body()
        XCTAssertThrowsError(try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(ready)))
        var pending = ready
        pending["state"] = .string("queued")
        pending["progress"] = .integer(0)
        pending["assetId"] = .null
        pending["modelPreparation"]["status"] = .string("pending")
        pending["modelPreparation"]["workerDrained"] = .bool(false)
        pending["modelPreparation"]["coarse"] = .null
        pending["modelPreparation"]["serverReceipt"] = .null
        try decode(pending).check(request: input, artworkId: artwork)
        pending["modelPreparation"]["purpose"] = .string("ordinary-original")
        XCTAssertThrowsError(try decode(pending).check(request: input, artworkId: artwork))
    }
}
