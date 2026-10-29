// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class FloorPreviewTests: XCTestCase {
    func testOnlyQualifiedRuntimeCanEditWhileExistingMetadataCanStillBeRead() throws {
        let hash = "b2ce49e9703cadf7d22b73ec5dfd93622489a07c34721f9bbe2b9f6df17ee28e"
        XCTAssertTrue(NativeFloorRuntime.supports(manifestSHA256: hash, ready: true))
        XCTAssertFalse(NativeFloorRuntime.supports(manifestSHA256: hash, ready: false))
        let interior = "859525370a4599a465a502fff15d1cb26db3961ee11ac5bf0a6f64af070c355d"
        XCTAssertTrue(NativeFloorRuntime.supports(manifestSHA256: interior, ready: true))
        XCTAssertFalse(NativeFloorRuntime.supports(manifestSHA256: interior, ready: false))
        XCTAssertFalse(NativeFloorRuntime.supports(manifestSHA256: "7e823d9372aba3613fb8794f981c0f01cc97a420e3e82c388a783439bc933556", ready: true), "Do not retain the incomplete proposal")
        XCTAssertFalse(NativeFloorRuntime.supports(manifestSHA256: String(repeating: "0", count: 64), ready: true))
        XCTAssertFalse(NativeFloorRuntime.supports(manifestSHA256: nil, ready: true))
        XCTAssertEqual(try FloorSettings.all(in: NativeExhibitionDraft.new().candidate), [])
    }
    func testRetainedHostFrameMustMatchCurrentPreviewAndScopeWithoutInventingEchoFields() throws {
        let base = try NativeExhibitionDraft.new(), epoch = UUID().uuidString, tenant = UUID().uuidString, principal = UUID().uuidString
        func frame(_ draft: NativeExhibitionDraft, disabled: Bool = true) -> ViewportFrame {
            .init(epoch: epoch, sequence: 7, tenantId: tenant, principalId: principal, draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: disabled)
        }
        XCTAssertTrue(frame(base).confirmsFloorPreview(ownerEpoch: epoch, acceptedSequence: 7, expectedDraft: base, tenantId: tenant, principalId: principal))
        XCTAssertFalse(frame(base).confirmsFloorPreview(ownerEpoch: UUID().uuidString, acceptedSequence: 7, expectedDraft: base, tenantId: tenant, principalId: principal))
        XCTAssertFalse(frame(base).confirmsFloorPreview(ownerEpoch: epoch, acceptedSequence: 8, expectedDraft: base, tenantId: tenant, principalId: principal))
        XCTAssertFalse(frame(base).confirmsFloorPreview(ownerEpoch: epoch, acceptedSequence: 7, expectedDraft: base, tenantId: UUID().uuidString, principalId: principal))
        XCTAssertFalse(frame(base).confirmsFloorPreview(ownerEpoch: epoch, acceptedSequence: 7, expectedDraft: base, tenantId: tenant, principalId: UUID().uuidString))
        XCTAssertFalse(frame(base, disabled: false).confirmsFloorPreview(ownerEpoch: epoch, acceptedSequence: 7, expectedDraft: base, tenantId: tenant, principalId: principal))
        var changed = base; try changed.rename("Changed")
        XCTAssertFalse(frame(base).confirmsFloorPreview(ownerEpoch: epoch, acceptedSequence: 7, expectedDraft: changed, tenantId: tenant, principalId: principal))
    }
    func testCatalogPreviewsHaveFixedRegisteredIdentityAndRefuseWrongBytes() throws {
        let previews = FloorSettings.Preset.allCases.map(NativeFloorPreview.forPreset)
        XCTAssertEqual(Set(previews.map(\.path)).count, 4)
        for file in previews {
            XCTAssertTrue(file.path.hasPrefix("/floor-textures/")); XCTAssertTrue(file.path.hasSuffix(".png"))
            XCTAssertTrue((1...2_000_000).contains(file.bytes))
            XCTAssertThrowsError(try file.check(Data()))
            XCTAssertThrowsError(try file.check(Data(repeating: 0, count: file.bytes)))
        }
    }
}
