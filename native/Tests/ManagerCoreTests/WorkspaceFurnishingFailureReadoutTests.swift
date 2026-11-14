// SPDX-License-Identifier: Apache-2.0
import Foundation
import JavaScriptCore
import XCTest
@testable import ManagerCore

final class WorkspaceFurnishingFailureReadoutTests: XCTestCase {
    private let furnishing = "22222222-2222-4222-8222-222222222222"
    private let digest = String(repeating: "a", count: 64)
    private func host(_ index: Int) -> String { String(format: "11111111-1111-4111-8111-%012d", index) }
    private func draft(hosts: Int = 1, attachments: Bool = false) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let rows: [DocumentValue] = (0..<hosts).map { .object([
            "id": .string(host($0)), "furnishingId": .string(furnishing), "furnishingVersion": .integer(1)
        ]) }
        var stored: DocumentValue = .object([
            "version": .integer(11), "furnishings": .array([.object([
                "id": .string(furnishing), "version": .integer(1), "model": .object(["sha256": .string(digest)])
            ])]), "placements": .array(rows), "frameAttachments": .array([]), "openingAttachments": .array([]),
            "ceilingAttachments": .array([.object(["placementId": .string(host(0))])])
        ])
        if attachments {
            stored["frameAttachments"] = .array([.object(["placementId": .string(host(100)), "furnishingId": .string(furnishing), "furnishingVersion": .integer(1)])])
            stored["openingAttachments"] = .array([.object(["openingId": .string(host(101)), "furnishingId": .string(furnishing), "furnishingVersion": .integer(1)])])
        }
        candidate["extensions"] = .object([WorkspaceFurnishingPlacement.namespace: stored])
        try draft.replaceCandidate(candidate)
        return draft
    }
    private func row(_ index: Int = 0) -> DocumentValue {
        .object([
            "placementId": .string(host(index)), "furnishingId": .string(furnishing), "furnishingVersion": .integer(1),
            "modelSha256": .string(digest), "stage": .string("scene-reserve"), "code": .string("WORKSPACE_SCENE_BUDGET"),
            "httpKind": .string("model"), "httpStatus": .integer(200), "remainingDraws": .integer(3),
            "remainingTriangles": .integer(1000), "requestedDraws": .integer(10), "requestedTriangles": .integer(1880),
            "byteLimit": .integer(10_000_000), "bytesUsed": .integer(8_000_000), "bytesReserved": .integer(1_000_000),
            "requestedBytes": .null
        ])
    }
    private func sample(_ rows: [DocumentValue]? = nil, loaded: Int64 = 0, failed: Int64 = 1, settled: Bool = true) -> DocumentValue {
        .object(["version": .integer(1), "settled": .bool(settled), "loaded": .integer(loaded), "failed": .integer(failed),
                 "truncated": .bool(failed > 32), "failures": .array(rows ?? [row()])])
    }
    private func json(_ value: DocumentValue) throws -> String {
        String(decoding: try JSONEncoder().encode(value), as: UTF8.self)
    }
    private func project(_ value: DocumentValue, in draft: NativeExhibitionDraft) throws -> DocumentValue {
        WorkspaceFurnishingFailureReadout.projected(try json(value), in: draft)
    }
    private func changed(_ source: DocumentValue, _ key: String, _ value: DocumentValue) -> DocumentValue {
        var result = source, rows = source["failures"].items!; rows[0][key] = value; result["failures"] = .array(rows); return result
    }

    func testCurrentHostVersionAndHashRetainExactFailureWithoutMutation() throws {
        let draft = try draft(), before = draft, value = sample()
        XCTAssertEqual(try project(value, in: draft), value)
        XCTAssertEqual(draft, before)
        for (key, bad) in [("placementId", DocumentValue.string(host(9))), ("furnishingId", .string(host(0))),
                           ("furnishingVersion", .integer(2)), ("modelSha256", .string(String(repeating: "b", count: 64)))] {
            XCTAssertEqual(try project(changed(value, key, bad), in: draft), .null)
        }
    }
    func testFrameAndOpeningHostsAreBoundWithoutDuplicateCeilingHost() throws {
        let draft = try draft(attachments: true), value = sample([row(), row(100), row(101)], failed: 3)
        XCTAssertEqual(try project(value, in: draft), value)
        var candidate = draft.candidate, malformed = draft
        var descriptors = candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"].items!
        descriptors[0]["model"]["sha256"] = .string("invalid")
        candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"] = .array(descriptors)
        try malformed.replaceCandidate(candidate)
        XCTAssertEqual(try project(value, in: malformed), .null)
    }
    func testMissingLegacyAndMalformedSamplesStayUnavailable() throws {
        let draft = try draft()
        for text in [nil, "null", "{", "{}"] as [String?] {
            XCTAssertEqual(WorkspaceFurnishingFailureReadout.projected(text, in: draft), .null)
        }
        var extra = sample(); extra["rawError"] = .string("private path"); XCTAssertEqual(try project(extra, in: draft), .null)
        XCTAssertEqual(try project(changed(sample(), "rawError", .string("private path")), in: draft), .null)
        let padded = String(repeating: " ", count: WorkspaceFurnishingFailureReadout.maximumBytes) + (try json(sample()))
        XCTAssertEqual(WorkspaceFurnishingFailureReadout.projected(padded, in: draft), .null)
    }
    func testCountsTruncationAndUniqueFailuresCannotClaimCompleteness() throws {
        let draft = try draft(hosts: 33), rows = (0..<32).map { row($0) }, value = sample(rows, failed: 33)
        XCTAssertEqual(try project(value, in: draft), value)
        var bad = value; bad["truncated"] = .bool(false); XCTAssertEqual(try project(bad, in: draft), .null)
        bad = value; bad["failures"] = .array(rows + [row(32)]); XCTAssertEqual(try project(bad, in: draft), .null)
        bad = value; bad["failures"] = .array([row()] + rows.dropLast()); XCTAssertEqual(try project(bad, in: draft), .null)
        bad = sample(loaded: 31); XCTAssertEqual(try project(bad, in: draft), .null)
        bad["settled"] = .bool(false); XCTAssertEqual(try project(bad, in: draft), .null)
        bad = sample(loaded: 33); XCTAssertEqual(try project(bad, in: draft), .null)
        let empty = sample([], failed: 0)
        XCTAssertEqual(try project(empty, in: NativeExhibitionDraft.new()), empty)
    }
    func testFiniteEnumsHTTPAndCountersDoNotForwardRawErrors() throws {
        let draft = try draft(), value = sample()
        for (key, bad) in [("stage", DocumentValue.string("failed/private")), ("code", .string("https://private.invalid")),
                           ("code", .string("UNAGREED_CODE")), ("httpKind", .string("raw-url")), ("httpStatus", .integer(600)),
                           ("httpStatus", .integer(99)), ("remainingDraws", .integer(-1)), ("bytesUsed", .bool(true)),
                           ("bytesReserved", .integer(9_007_199_254_740_992)), ("requestedBytes", .number(1.5))] {
            XCTAssertEqual(try project(changed(value, key, bad), in: draft), .null)
        }
        var unknown = changed(value, "httpKind", .null)
        XCTAssertEqual(try project(unknown, in: draft), .null)
        unknown = changed(unknown, "httpStatus", .null)
        unknown = changed(unknown, "requestedDraws", .null); unknown = changed(unknown, "requestedTriangles", .null)
        XCTAssertEqual(try project(unknown, in: draft), unknown)
        let peak = changed(value, "code", .string("ASSET_TRANSITION_PEAK"))
        XCTAssertEqual(try project(peak, in: draft), peak)
    }
    func testExistingSynchronousBrowserSliceFiltersMalformedAndRawFields() throws {
        let context = try XCTUnwrap(JSContext()), value = sample(), raw = try json(value)
        func sampled(_ raw: String?) throws -> String? {
            let input: Any = raw.map { $0 as Any } ?? NSNull()
            context.setObject(input, forKeyedSubscript: "input" as NSString)
            let reply = context.evaluateScript("(() => { const c = {dataset:{workspaceFurnishingFailures:input}}, result = {}; " + WorkspaceFurnishingFailureReadout.samplingScript + "; return result.workspaceFurnishingFailures; })()")
            XCTAssertNil(context.exception)
            return reply?.isNull == true || reply?.isUndefined == true ? nil : reply?.toString()
        }
        XCTAssertEqual(WorkspaceFurnishingFailureReadout.projected(try sampled(raw), in: try draft()), value)
        XCTAssertNil(try sampled(nil))
        XCTAssertNil(try sampled("{"))
        var unsettled = value; unsettled["settled"] = .bool(false)
        XCTAssertNil(try sampled(json(unsettled)))
        XCTAssertNil(try sampled(json(changed(value, "code", .string("private raw message")))))
        XCTAssertNil(try sampled(json(changed(value, "stack", .string("/private/path")))))
        XCTAssertNil(try sampled(String(repeating: " ", count: 32_768) + raw))
    }
}
