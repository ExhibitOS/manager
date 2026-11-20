// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct DefaultCatalogueUpdateCheck: Decodable, Sendable {
    public struct Snapshot: Decodable, Equatable, Sendable {
        public let bundleVersion: String
        public let manifestSha256: String

        var valid: Bool {
            !bundleVersion.isEmpty && bundleVersion.utf8.count <= 64 && nativeImportDigest(manifestSha256)
        }
    }

    public struct Change: Decodable, Identifiable, Sendable {
        public let id: String
        public let version: Int

        var valid: Bool {
            id.range(of: "^[a-z0-9-]{1,128}$", options: .regularExpression) != nil && version > 0
        }
    }

    public struct Plan: Decodable, Sendable {
        public let bundleVersion: String
        public let manifestSha256: String
        public let planSha256: String
        public let added: [Change]
        public let changed: [Change]
        public let downloadBytes: Int64

        var snapshot: Snapshot { .init(bundleVersion: bundleVersion, manifestSha256: manifestSha256) }

        var valid: Bool {
            let items = added + changed
            return snapshot.valid && nativeImportDigest(planSha256) && downloadBytes >= 0
                && items.allSatisfy(\.valid) && Set(items.map(\.id)).count == items.count
        }
    }

    public static let capability = "default-catalogue-update-v1"
    public let profile: String
    public let active: Snapshot
    public let update: Plan?

    private enum CodingKeys: String, CodingKey { case profile, active, update }

    public init(from decoder: Decoder) throws {
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        profile = try fields.decode(String.self, forKey: .profile)
        active = try fields.decode(Snapshot.self, forKey: .active)
        guard fields.contains(.update) else { throw AuthoringFailure("DEFAULT_CATALOGUE_INTEGRITY") }
        update = try fields.decodeIfPresent(Plan.self, forKey: .update)
    }

    func checked() throws -> Self {
        guard profile == Self.capability, active.valid,
              update.map({ $0.valid && $0.manifestSha256 != active.manifestSha256 }) ?? true else {
            throw AuthoringFailure("DEFAULT_CATALOGUE_INTEGRITY")
        }
        return self
    }
}

public struct DefaultCatalogueUpdateReceipt: Decodable, Sendable {
    public let profile: String
    public let active: DefaultCatalogueUpdateCheck.Snapshot
    public let previous: DefaultCatalogueUpdateCheck.Snapshot
    public let downloadedBytes: Int64

    func checked(previous expected: DefaultCatalogueUpdateCheck.Snapshot,
                 plan: DefaultCatalogueUpdateCheck.Plan) throws -> Self {
        guard profile == DefaultCatalogueUpdateCheck.capability, previous == expected, active == plan.snapshot,
              (0...plan.downloadBytes).contains(downloadedBytes) else {
            throw AuthoringFailure("DEFAULT_CATALOGUE_INTEGRITY")
        }
        return self
    }
}
