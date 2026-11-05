// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativePrivateFloorCatalog: Equatable, Sendable {
    public static let id = "quiet-oak-herringbone"
    public static let version = 1
    public static let name = "차분한 오크 헤링본"

    // The versioned catalogue is available only in the exact reviewed local runtime.
    static let manifestSHA256 = "9b9d6b1cba4b5722a8710471283e9f693f14883f5da950c4504ca7bbf00efeb7"

    public let previewPath: String
    static let previewBytes = 27_373
    static let previewSHA256 = "4423a1fc11dc52075ae1882d6971c656819630098696b77608b7309d0e53c9a9"

    public static let defaults: [FloorSettings.Field: DocumentValue] = [
        .texturePack: .object(["id": .string(id), "version": .integer(1)]),
        .repeatMeters: .vector([0.9, 0.9]),
        .rotation: .number(0),
        .normalScale: .number(1)
    ]

    public static func supports(manifestSHA256: String?, ready: Bool) -> Bool {
        ready && manifestSHA256 == Self.manifestSHA256
    }

    public init(value: DocumentValue, tenantId: String) throws {
        let invalid = AuthoringFailure("MATERIAL_CATALOG_INVALID")
        guard let tenant = UUID(uuidString: tenantId)?.uuidString.lowercased(),
              case .object(let envelope) = value, Set(envelope.keys) == ["version", "floor"],
              envelope["version"] == .integer(1), let floor = envelope["floor"],
              case .object(let fields) = floor, Set(fields.keys) == ["id", "version", "name", "defaults", "preview"],
              fields["id"] == .string(Self.id), fields["version"] == .integer(1),
              fields["name"] == .string(Self.name) else { throw invalid }
        var expectedDefaults = Dictionary(uniqueKeysWithValues: Self.defaults.map { ($0.key.rawValue, $0.value) })
        expectedDefaults["preset"] = .string("wood")
        let path = "/api/v1/tenants/\(tenant)/studio/materials/\(Self.id)/\(Self.version)/preview"
        let preview: DocumentValue = .object([
            "url": .string(path), "mime": .string("image/png"),
            "bytes": .integer(Int64(Self.previewBytes)), "sha256": .string(Self.previewSHA256),
            "width": .integer(256), "height": .integer(256)
        ])
        guard fields["defaults"] == .object(expectedDefaults), fields["preview"] == preview else { throw invalid }
        previewPath = path
    }
}
