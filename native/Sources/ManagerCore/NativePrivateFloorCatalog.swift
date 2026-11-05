// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct NativePrivateFloorCatalog: Equatable, Sendable {
    public static let id = "quiet-oak-herringbone"
    public static let version = 1
    public static let name = "차분한 오크 헤링본"
    static let reference: DocumentValue = .object(["id": .string(id), "version": .integer(Int64(version))])

    // The versioned catalogue is available only in the exact reviewed local runtime.
    static let manifestSHA256 = "9b9d6b1cba4b5722a8710471283e9f693f14883f5da950c4504ca7bbf00efeb7"

    public let previewPath: String
    static let previewBytes = 27_373
    static let previewSHA256 = "4423a1fc11dc52075ae1882d6971c656819630098696b77608b7309d0e53c9a9"

    public static let defaults: [FloorSettings.Field: DocumentValue] = [
        .texturePack: reference,
        .repeatMeters: .vector([0.9, 0.9]),
        .rotation: .number(0),
        .normalScale: .number(1)
    ]

    public static func supports(manifestSHA256: String?, ready: Bool) -> Bool {
        ready && [Self.manifestSHA256,
                  "7ff6e64700cbe7324b634d2cb11e0acc87288ce27ab90ae18e675ae5fa899eb2",
                  "4f4b584c98edfb54ce82da76bec53bad4e6b72e9c8389b0385e524d81db15a23",
                  "83d711e787ba01938739f2af95f83f578784dc6105db887799ac8355068f8483",
                  "a2a83a968a2fab269cd43ba0c0f054dfe86ce1033135bd304964726c3676ea24",
                  "b751e43ed1968cd53aea2a9495a395d193b4d02ec48cd9e16ebae6806b4599b9",
                  "c9907e0733363fac0fa05848976913b5704d28ea8a7cb6bfcef343eb42af7031"].contains(manifestSHA256 ?? "")
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
