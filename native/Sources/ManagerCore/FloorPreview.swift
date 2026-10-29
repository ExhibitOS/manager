// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Only exact qualified floor-containing runtimes are admitted here. This
/// is metadata/byte availability, not a claim of visual or audio qualification.
public enum NativeFloorRuntime {
    public static func supports(manifestSHA256: String?, ready: Bool) -> Bool {
        ready && (manifestSHA256 == "b2ce49e9703cadf7d22b73ec5dfd93622489a07c34721f9bbe2b9f6df17ee28e"
            || manifestSHA256 == "7e823d9372aba3613fb8794f981c0f01cc97a420e3e82c388a783439bc933556")
    }
}

extension ViewportFrame {
    /// The host retains this exact immutable frame after accept() succeeds.
    /// The shared ACK does not echo fields and does not certify map readiness.
    public func confirmsFloorPreview(ownerEpoch: String?, acceptedSequence: Int, expectedDraft: NativeExhibitionDraft,
                                     tenantId: String, principalId: String) -> Bool {
        epoch == ownerEpoch && UUID(uuidString: epoch) != nil && sequence > 0 && sequence == acceptedSequence
            && self.tenantId == tenantId && self.principalId == principalId && draft == expectedDraft && disabled
    }
}

/// Four immutable color-map projections from the existing public floor registry.
/// They are native list previews; the shared renderer owns all thirteen PBR maps.
public struct NativeFloorPreview: Sendable {
    public let path: String
    public let bytes: Int
    public let sha256: String
    public static func forPreset(_ preset: FloorSettings.Preset) -> Self {
        switch preset {
        case .wood: .init(path: "/floor-textures/WoodFloor051/1/color-68bad517cc4a.png", bytes: 1_235_894, sha256: "68bad517cc4aafa4c5ab5812f6a2ee57068da49fcb9446941d44371d716b3ce3")
        case .marble: .init(path: "/floor-textures/Marble006/1/color-d44e0a2b0402.png", bytes: 1_798_345, sha256: "d44e0a2b040249c629a67aace4bf0f82a79c33199f1817d55cb1d199bbcefea2")
        case .tile: .init(path: "/floor-textures/Tiles074/1/color-99d35940d5ae.png", bytes: 1_714_324, sha256: "99d35940d5ae7e645ea99f0d3c0d20eb73932d3d68303f534ac481afd0976f16")
        case .carpet: .init(path: "/floor-textures/Carpet004/1/color-df90d6a3be6b.png", bytes: 1_932_055, sha256: "df90d6a3be6b2be92e89b830d41ec329a170266009a3cffa2e05bf043085b25f")
        }
    }
    public func check(_ data: Data) throws -> Data {
        guard data.count == bytes, nativeImportSHA256(data) == sha256 else { throw AuthoringFailure("FLOOR_PREVIEW_INVALID") }
        return data
    }
}
