// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeJPEGImportRuntimeTests: XCTestCase {
    func testCurrentPacketRequiresReadinessAndUnknownPacketsStayClosed() {
        let current = NativeJPEGImportRuntime.currentManifestSHA256
        XCTAssertTrue(NativeJPEGImportRuntime.supports(manifestSHA256: current, ready: true))
        XCTAssertFalse(NativeJPEGImportRuntime.supports(manifestSHA256: current, ready: false))
        for manifest in [nil, "", String(repeating: "0", count: 64), current.uppercased()] {
            XCTAssertFalse(NativeJPEGImportRuntime.supports(manifestSHA256: manifest, ready: true))
        }
    }

    func testExistingJPEGPacketsRetainedWithoutEnablingOtherFeatures() throws {
        let watermarkPacket = try XCTUnwrap(PublicationWatermarkRuntime.qualifiedManifestSHA256)
        for manifest in ["74b08c7899d74ad989bbb61e47fc86fe9fa25c8853ab27ce57e9135c4932e96a",
                         watermarkPacket,
                         "b2ce49e9703cadf7d22b73ec5dfd93622489a07c34721f9bbe2b9f6df17ee28e",
                         "859525370a4599a465a502fff15d1cb26db3961ee11ac5bf0a6f64af070c355d"] {
            XCTAssertTrue(NativeJPEGImportRuntime.supports(manifestSHA256: manifest, ready: true))
            XCTAssertFalse(NativeJPEGImportRuntime.supports(manifestSHA256: manifest, ready: false))
        }
        let current = NativeJPEGImportRuntime.currentManifestSHA256
        XCTAssertFalse(PublicationWatermarkRuntime.supports(manifestSHA256: current, ready: true))
        XCTAssertFalse(NativeModelDerivationProfile.supports(manifestSHA256: current, ready: true))
    }
}
