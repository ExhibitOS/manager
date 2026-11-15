// SPDX-License-Identifier: Apache-2.0
import XCTest
import ManagerCore
@testable import ManagerApp

final class NativeCameraPoseReadoutTests: XCTestCase {
    private let epoch = "11111111-1111-4111-8111-111111111111"
    private func sample(position: DocumentValue = .array([.number(1), .number(2), .number(3)]),
                        target: DocumentValue = .array([.number(1), .number(4), .number(3)])) -> DocumentValue {
        .object(["version": .integer(1), "frameToken": .string("\(epoch):7"),
                 "position": position, "target": target])
    }
    private func project(_ value: DocumentValue) throws -> DocumentValue {
        NativeCameraPoseReadout.projected(String(decoding: try JSONEncoder().encode(value), as: UTF8.self), epoch: epoch, sequence: 7)
    }
    func testCurrentWorldPositionAndOrbitTargetAreProjectedWithoutRequestMetadata() throws {
        let result = try project(sample())
        XCTAssertEqual(result["status"], .string("current"))
        XCTAssertEqual(result["position"].items?.compactMap(\.double), [1, 2, 3])
        XCTAssertEqual(result["target"].items?.compactMap(\.double), [1, 4, 3])
        guard case .object(let fields) = result else { return XCTFail("object required") }
        XCTAssertEqual(Set(fields.keys), ["status", "position", "target"])
    }
    func testStaleFrameUnknownFieldsAndMalformedDataRemainUnavailable() throws {
        for (key, value) in [("frameToken", DocumentValue.string("\(epoch):6")),
                             ("frameToken", .string("22222222-2222-4222-8222-222222222222:7")),
                             ("version", .integer(2)), ("requestedPose", .bool(true))] {
            var input = sample(); input[key] = value
            XCTAssertEqual(try project(input), NativeCameraPoseReadout.unavailable)
        }
        for raw in [nil, "{}", "not-json", String(repeating: " ", count: 4097)] as [String?] {
            XCTAssertEqual(NativeCameraPoseReadout.projected(raw, epoch: epoch, sequence: 7), NativeCameraPoseReadout.unavailable)
        }
    }
    func testFiniteBoundsExactVectorShapeAndNondegenerateThreshold() throws {
        XCTAssertEqual(try project(sample(position: .array([.number(-10_000), .number(0), .number(10_000)])))["status"], .string("current"))
        for position in [DocumentValue.array([.number(10_000.1), .number(0), .number(0)]),
                         .array([.number(0), .number(0)]), .array([.bool(true), .number(0), .number(0)])] {
            XCTAssertEqual(try project(sample(position: position)), NativeCameraPoseReadout.unavailable)
        }
        let zero: DocumentValue = .array([.number(0), .number(0), .number(0)])
        XCTAssertEqual(try project(sample(position: zero, target: zero)), NativeCameraPoseReadout.unavailable)
        XCTAssertEqual(try project(sample(position: zero, target: .array([.number(1e-6), .number(0), .number(0)]))), NativeCameraPoseReadout.unavailable)
        XCTAssertEqual(try project(sample(position: zero, target: .array([.number(2e-6), .number(0), .number(0)])))["status"], .string("current"))
        XCTAssertEqual(NativeCameraPoseReadout.projected("{\"version\":1,\"frameToken\":\"\(epoch):7\",\"position\":[1e309,0,0],\"target\":[0,1,0]}", epoch: epoch, sequence: 7), NativeCameraPoseReadout.unavailable)
    }
    func testInvalidExpectedFrameCannotAdmitASample() throws {
        let raw = String(decoding: try JSONEncoder().encode(sample()), as: UTF8.self)
        for sequence in [0, -1, 9_007_199_254_740_992] {
            XCTAssertEqual(NativeCameraPoseReadout.projected(raw, epoch: epoch, sequence: sequence), NativeCameraPoseReadout.unavailable)
        }
        XCTAssertEqual(NativeCameraPoseReadout.projected(raw, epoch: "invalid", sequence: 7), NativeCameraPoseReadout.unavailable)
    }
}
