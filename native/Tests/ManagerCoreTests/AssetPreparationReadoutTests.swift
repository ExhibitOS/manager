// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class AssetPreparationReadoutTests: XCTestCase {
    private let revision = "aaaaaaaa-0000-4000-8000-000000000001"
    private let asset = "bbbbbbbb-0000-4000-8000-000000000002"
    private let otherRevision = "cccccccc-0000-4000-8000-000000000003"

    private func draft(assets: [String]? = nil, ambiguous: Bool = false) throws -> NativeExhibitionDraft {
        var value = try NativeExhibitionDraft.new(), candidate = value.candidate
        var artworks: [DocumentValue] = [.object([
            "revisionId": .string(revision),
            "assets": .array((assets ?? [asset]).map { .object(["id": .string($0)]) })
        ])]
        if ambiguous { artworks.append(.object(["revisionId": .string(otherRevision), "assets": .array([.object(["id": .string(asset)])])])) }
        candidate["artworks"] = .array(artworks)
        try value.replaceCandidate(candidate)
        return value
    }
    private func row(asset: String? = nil, stage: String = "failed", code: DocumentValue = .string("DERIVATIVE_UNAVAILABLE")) -> DocumentValue {
        .object(["assetId": .string(asset ?? self.asset), "artworkRevisionId": .string(revision),
                 "stage": .string(stage), "errorCode": code])
    }
    private func json(_ rows: [DocumentValue]) throws -> String {
        String(decoding: try JSONEncoder().encode(DocumentValue.object(["preparationStages": .array(rows)])), as: UTF8.self)
    }

    func testExactDocumentBindingAndSafeFailureCodeWithoutMutation() throws {
        let source = try draft(), before = source
        var entry = row(); entry["assetId"] = .string(asset.uppercased())
        let result = AssetPreparationReadout.projected(try json([entry]), in: source)
        XCTAssertEqual(result["preparationStages"].items, [row()])
        XCTAssertEqual(source, before)
        let budget = row(stage: "deferred-budget", code: .string("ASSET_TRANSITION_PEAK"))
        XCTAssertEqual(AssetPreparationReadout.projected(try json([budget]), in: source)["preparationStages"].items, [budget])
    }

    func testLegacyMissingAndMalformedSamplesAreUnavailable() throws {
        let source = try draft()
        for text in [nil, "null", "{", "{}", "{\"preparationStages\":null}"] as [String?] {
            XCTAssertEqual(AssetPreparationReadout.projected(text, in: source), .null)
        }
        var old = row()
        if case .object(var fields) = old { fields.removeValue(forKey: "artworkRevisionId"); old = .object(fields) }
        XCTAssertEqual(AssetPreparationReadout.projected(try json([old]), in: source), .null)
        var extra = row(); extra["stack"] = .string("private file or URL")
        XCTAssertEqual(AssetPreparationReadout.projected(try json([extra]), in: source), .null)
    }

    func testUnknownAssetAndAmbiguousOrWrongRevisionCannotClaimAnError() throws {
        let source = try draft()
        XCTAssertEqual(AssetPreparationReadout.projected(try json([row(asset: UUID().uuidString)]), in: source), .null)
        XCTAssertEqual(AssetPreparationReadout.projected(try json([row()]), in: try NativeExhibitionDraft.new()), .null)
        for declared in [DocumentValue.null, .string(otherRevision)] {
            var entry = row(); entry["artworkRevisionId"] = declared
            let result = AssetPreparationReadout.projected(try json([entry]), in: source)["preparationStages"].items![0]
            XCTAssertEqual(result["assetId"], .string(asset))
            XCTAssertEqual(result["artworkRevisionId"], .null)
            XCTAssertEqual(result["errorCode"], .null)
        }
        let result = AssetPreparationReadout.projected(try json([row()]), in: try draft(ambiguous: true))["preparationStages"].items![0]
        XCTAssertEqual(result["artworkRevisionId"], .null)
        XCTAssertEqual(result["errorCode"], .null)
    }

    func testLatest32UniqueAssetsAndByteLimitAreEnforced() throws {
        let assets = (0..<33).map { String(format: "dddddddd-0000-4000-8000-%012d", $0) }
        let source = try draft(assets: assets)
        let rows = assets.map { row(asset: $0) }
        XCTAssertEqual(AssetPreparationReadout.projected(try json(Array(rows.prefix(32))), in: source)["preparationStages"].items?.count, 32)
        XCTAssertEqual(AssetPreparationReadout.projected(try json(rows), in: source), .null)
        XCTAssertEqual(AssetPreparationReadout.projected(try json([row(), row()]), in: try draft()), .null)
        let padded = String(repeating: " ", count: AssetPreparationReadout.maximumBytes) + (try json([]))
        XCTAssertEqual(AssetPreparationReadout.projected(padded, in: source), .null)
    }

    func testRawMessagesPathsAndInvalidTokensAreNeverForwarded() throws {
        let source = try draft()
        for code in ["https://example.invalid/private", "/private/profile", "TypeError: failed", "FAILED\n", String(repeating: "A", count: 81)] {
            XCTAssertEqual(AssetPreparationReadout.projected(try json([row(code: .string(code))]), in: source), .null)
        }
        for stage in ["failed\n", "FAILED", "bad stage", String(repeating: "a", count: 65)] {
            XCTAssertEqual(AssetPreparationReadout.projected(try json([row(stage: stage)]), in: source), .null)
        }
        var entry = row(); entry["artworkRevisionId"] = .string("not-a-uuid")
        XCTAssertEqual(AssetPreparationReadout.projected(try json([entry]), in: source), .null)
    }

    func testProgressSuccessAndCancellationCannotCarryAnOldFailureCode() throws {
        let source = try draft()
        for stage in ["fetch-verify", "verified-download", "display-attached-render-requested", "cancelled-settled"] {
            let entry = row(stage: stage, code: .null)
            XCTAssertEqual(AssetPreparationReadout.projected(try json([entry]), in: source)["preparationStages"].items, [entry])
            XCTAssertEqual(AssetPreparationReadout.projected(try json([row(stage: stage)]), in: source), .null)
        }
        let unknown = row(code: .null)
        XCTAssertEqual(AssetPreparationReadout.projected(try json([unknown]), in: source)["preparationStages"].items, [unknown])
    }
}
