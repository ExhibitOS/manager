// SPDX-License-Identifier: Apache-2.0
import CryptoKit
import Foundation
import XCTest
@testable import ManagerCore

final class NativeWorkspaceOEXTerminalProofTests: XCTestCase {
    private typealias Terminal = NativeWorkspaceOEXTerminalProof

    func testExactPlatformFixture() throws {
        struct PlatformFixture: Decodable {
            let scope: Terminal.Scope
            let proofUtf8: String
            let resultUtf8: String
            let result: NativeStudioResponse
            let saved: NativeStudioResponse
        }
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_DRAFT12_TERMINAL_TEST_FIXTURE"] else {
            throw XCTSkip("Supply the pinned Platform fixture for the producer/consumer check.")
        }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
        XCTAssertEqual(sha256(bytes), "cfb5db6d04f4cb7e0be25cb4d67fce9fac156f3eb6aa6bd39b28a104f9a05d9e")
        let fixture = try JSONDecoder().decode(PlatformFixture.self, from: bytes)
        let offer = NativeOEXSaveOffer(attemptId: fixture.scope.attemptId, exhibitionId: fixture.result.draft.id,
                                      etag: fixture.result.etag, requestId: uuid())
        let verified = try Terminal.verify(proofBytes: Data(fixture.proofUtf8.utf8),
            resultBytes: Data(fixture.resultUtf8.utf8), scope: fixture.scope, offer: offer,
            held: fixture.result, reopened: fixture.saved)
        XCTAssertEqual(verified.scope, fixture.scope)
        XCTAssertEqual(verified.studioETag, fixture.result.etag)
        XCTAssertEqual(verified.resultBytes, 31_503)
        XCTAssertEqual(verified.resultSha256, "99f2f5adc98e30e170813b96fee742db67e767ac60fb21185abdc5a20641c497")
        XCTAssertEqual(verified.proofSha256, "cb9fadf4f98414781521c7363168c1591a71a5132fa6eb1ce65089f0311da6f3")
    }

    func testValidProofReturnsExactByteDigests() throws {
        let input = try Fixture()
        let verified = try input.verify()
        XCTAssertEqual(verified.scope, input.scope)
        XCTAssertEqual(verified.exhibitionId, input.held.draft.id)
        XCTAssertEqual(verified.immutableRevisionId, input.held.draft.candidate["revisionId"].string)
        XCTAssertEqual(verified.studioETag, input.held.etag)
        XCTAssertEqual(verified.resultBytes, input.resultBytes.count)
        XCTAssertEqual(verified.resultSha256, sha256(input.resultBytes))
        XCTAssertEqual(verified.proofSha256, sha256(try input.proof()))
    }

    func testRejectsCorruptionBoundsAndMalformedProof() throws {
        let input = try Fixture()
        var corrupted = input.resultBytes
        corrupted[corrupted.startIndex + 1] ^= 1
        rejects(try input.verify(result: corrupted))
        rejects(try input.verify(result: input.resultBytes + Data([0x20])))
        for count in [0, Terminal.resultByteLimit + 1] {
            rejects(try input.verify(result: Data(repeating: 0x20, count: count)))
        }
        for count in [0, Terminal.proofByteLimit + 1] {
            rejects(try input.verify(proof: Data(repeating: 0x20, count: count)))
        }
        let wire = try input.proof()
        for suffix in [#" ,"extra":0}"#, #" ,"version":1}"#] {
            rejects(try input.verify(proof: wire.dropLast() + Data(suffix.utf8)))
        }
        rejects(try input.verify(proof: input.proof(replacing: ["version": true])))
        rejects(try input.verify(proof: input.proof(replacing: ["archiveFormat": "1.0.0-draft.10"])))

        let invalidUTF8 = Data([0xff])
        rejects(try input.verify(proof: input.proof(for: invalidUTF8), result: invalidUTF8))
        let bom = Data([0xef, 0xbb, 0xbf]) + input.resultBytes
        rejects(try input.verify(proof: input.proof(for: bom), result: bom))
    }

    func testRejectsEveryWrongScopeFieldAndSaveOfferAttempt() throws {
        let input = try Fixture()
        for key in ["workspaceId", "tenantId", "principalId", "jobId", "attemptId", "importRequestId",
                    "exhibitionId", "immutableRevisionId"] {
            rejects(try input.verify(proof: input.proof(replacing: [key: uuid()])))
        }
        rejects(try input.verify(proof: input.proof(replacing: ["sourceArchiveSha256": String(repeating: "b", count: 64)])))
        let wrongOffer = NativeOEXSaveOffer(attemptId: uuid(), exhibitionId: input.offer.exhibitionId,
                                           etag: input.offer.etag, requestId: input.offer.requestId)
        rejects(try input.verify(offer: wrongOffer))
    }

    func testRejectsR1AndR2SubstitutionOrDifferentFullDraft() throws {
        let input = try Fixture()
        rejects(try input.verify(held: input.reopened, reopened: input.held))
        rejects(try input.verify(reopened: input.held))
        rejects(try input.verify(proof: input.proof(replacing: ["studioETag": input.reopened.etag])))
        var wrongResult = input.result
        wrongResult["etag"] = .string(input.reopened.etag)
        wrongResult["revision"] = .integer(2)
        let wrongResultBytes = try JSONEncoder().encode(wrongResult)
        rejects(try input.verify(proof: input.proof(for: wrongResultBytes), result: wrongResultBytes))

        var candidate = input.held.draft.candidate
        candidate["extensions"]["test.example/opaque"] = .object(["note": .string("changed")])
        var draft = input.held.draft
        try draft.replaceCandidate(candidate)
        let wrongReopen = NativeStudioResponse(revision: 2, etag: input.reopened.etag, draft: draft)
        rejects(try input.verify(reopened: wrongReopen))

        wrongResult = input.result
        wrongResult["draft"] = draft.document
        let changedDraftBytes = try JSONEncoder().encode(wrongResult)
        rejects(try input.verify(proof: input.proof(for: changedDraftBytes), result: changedDraftBytes))
    }

    func testRejectsInvalidResultEnvelopeEvenWithMatchingDigest() throws {
        let input = try Fixture()
        let duplicate = input.resultBytes.dropLast() + Data(#","revision":1}"#.utf8)
        rejects(try input.verify(proof: input.proof(for: duplicate), result: duplicate))
        for (key, value) in [("extra", .bool(true)), ("assetAliases", .array([.object([:])])),
                             ("idMap", .object(["invalid": .string(uuid())]))] as [(String, DocumentValue)] {
            var result = input.result
            result[key] = value
            let bytes = try JSONEncoder().encode(result)
            rejects(try input.verify(proof: input.proof(for: bytes), result: bytes))
        }
    }

    func testRejectsNestedDuplicateKeysEvenWithMatchingDigest() throws {
        let input = try Fixture()
        let result = String(decoding: input.resultBytes, as: UTF8.self)
        let candidate = try XCTUnwrap(result.range(of: #""candidate":{"#))
        for key in ["id", #"\u0069d"#] {
            var changed = result
            changed.insert(contentsOf: #""\#(key)":"\#(input.held.draft.id)","#, at: candidate.upperBound)
            let bytes = Data(changed.utf8)
            rejects(try input.verify(proof: input.proof(for: bytes), result: bytes))
        }
    }

    func testRejectsUnmappedOrInvalidMaterialBindingEvenWhenBothDraftsMatch() throws {
        let input = try Fixture()
        var unmapped = input.result
        unmapped["idMap"] = .object([:])
        let unmappedBytes = try JSONEncoder().encode(unmapped)
        rejects(try input.verify(proof: input.proof(for: unmappedBytes), result: unmappedBytes))

        let namespace = WorkspaceFloorMaterialBinding.namespace
        let extensionValue = input.held.draft.candidate["extensions"][namespace]
        let binding = try XCTUnwrap(extensionValue["bindings"].items?.first)
        for (key, value) in [("surfaceId", .string(uuid())), ("materialVersion", .integer(2)),
                             ("repeatMeters", .vector([0, 2])), ("normalScale", .null)] as [(String, DocumentValue)] {
            var changed = binding
            changed[key] = value
            var candidate = input.held.draft.candidate
            candidate["extensions"][namespace]["bindings"] = .array([changed])
            var draft = input.held.draft
            try draft.replaceCandidate(candidate)
            var result = input.result
            result["draft"] = draft.document
            let resultBytes = try JSONEncoder().encode(result)
            rejects(try input.verify(proof: input.proof(for: resultBytes), result: resultBytes,
                held: NativeStudioResponse(revision: 1, etag: input.held.etag, draft: draft),
                reopened: NativeStudioResponse(revision: 2, etag: input.reopened.etag, draft: draft)))
        }
    }

    private func rejects(_ expression: @autoclosure () throws -> Terminal.Verified, line: UInt = #line) {
        XCTAssertThrowsError(try expression(), line: line) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "OEX_TERMINAL_PROOF_INVALID", line: line)
        }
    }

    private struct Fixture {
        let scope = Terminal.Scope(workspaceId: uuid(), tenantId: uuid(), principalId: uuid(), jobId: uuid(),
                                   attemptId: uuid(), importRequestId: uuid(), sourceArchiveSha256: String(repeating: "a", count: 64))
        let offer: NativeOEXSaveOffer
        let held: NativeStudioResponse
        let reopened: NativeStudioResponse
        let result: DocumentValue
        let resultBytes: Data

        init() throws {
            let floor = uuid(), materialId = uuid()
            let material = try JSONDecoder().decode(WorkspaceMaterial.self, from: Data(
                """
                {"id":"\(materialId)","version":1,"kind":"floor","name":"Synthetic","provenance":"Synthetic",
                 "rights":{"holder":"Synthetic","ownership":"owner","creditLine":"Synthetic",
                   "permissions":{"display":true,"download":false,"export":true,"commercial":false}},
                 "maps":{"color":{"mime":"image/png","bytes":68,"sha256":"\(scope.sourceArchiveSha256)",
                   "width":2,"height":2,"colorSpace":"srgb"}}}
                """.utf8))
            var draft = try NativeExhibitionDraft.new()
            var candidate = draft.candidate
            candidate["surfaces"] = .array([.object(["id": .string(floor), "type": .string("floor")])])
            try draft.replaceCandidate(candidate)
            let binding = try WorkspaceFloorMaterialBinding(surfaceId: floor, materialId: materialId, materialVersion: 1,
                                                            repeatMeters: [2, 2], rotation: 0)
            draft = try XCTUnwrap(binding.applying(material, to: draft))
            let etag = "\"studio-r1-\(scope.sourceArchiveSha256)\""
            held = NativeStudioResponse(revision: 1, etag: etag, draft: draft)
            reopened = NativeStudioResponse(revision: 2, etag: "\"studio-r2-\(scope.sourceArchiveSha256)\"", draft: draft)
            offer = NativeOEXSaveOffer(attemptId: scope.attemptId, exhibitionId: draft.id, etag: etag, requestId: uuid())
            result = .object(["exhibitionId": .string(draft.id), "etag": .string(etag), "revision": .integer(1),
                              "draft": draft.document, "assetAliases": .array([]),
                              "idMap": .object([uuid(): .string(floor), uuid(): .string(materialId)])])
            resultBytes = try JSONEncoder().encode(result)
        }

        func proof(for result: Data? = nil, replacing: [String: Any] = [:]) throws -> Data {
            let raw = result ?? resultBytes
            let fields: [(String, Any)] = [
                ("version", 1), ("kind", "workspace-oex-import-terminal"), ("workspaceId", scope.workspaceId),
                ("tenantId", scope.tenantId), ("principalId", scope.principalId), ("jobId", scope.jobId),
                ("attemptId", scope.attemptId), ("importRequestId", scope.importRequestId),
                ("sourceArchiveSha256", scope.sourceArchiveSha256), ("archiveFormat", "1.0.0-draft.12"),
                ("exhibitionId", held.draft.id), ("immutableRevisionId", try XCTUnwrap(held.draft.candidate["revisionId"].string)),
                ("studioETag", offer.etag), ("resultBytes", raw.count), ("resultSha256", sha256(raw))
            ]
            let members = try fields.map { key, value in
                let encoded = try JSONSerialization.data(withJSONObject: replacing[key] ?? value, options: .fragmentsAllowed)
                return "\"\(key)\":" + String(decoding: encoded, as: UTF8.self)
            }
            return Data(("{" + members.joined(separator: ",") + "}").utf8)
        }

        func verify(proof: Data? = nil, result: Data? = nil, offer: NativeOEXSaveOffer? = nil,
                    held: NativeStudioResponse? = nil, reopened: NativeStudioResponse? = nil) throws -> Terminal.Verified {
            try Terminal.verify(proofBytes: proof ?? self.proof(), resultBytes: result ?? resultBytes,
                                scope: scope, offer: offer ?? self.offer, held: held ?? self.held,
                                reopened: reopened ?? self.reopened)
        }
    }
}

private func uuid() -> String { UUID().uuidString.lowercased() }
private func sha256(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
