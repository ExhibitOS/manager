// SPDX-License-Identifier: Apache-2.0
import Foundation

public enum WorkspaceMaterialKind: String, Codable, CaseIterable, Sendable {
    case floor, wall
}

public enum WorkspaceMaterialRole: String, Codable, CaseIterable, Sendable {
    case color, normal, roughness, ao
}

public struct WorkspaceMaterialMap: Codable, Equatable, Sendable {
    public static let byteLimit = 8 * 1024 * 1024
    public let mime: String
    public let bytes: Int
    public let sha256: String
    public let width: Int
    public let height: Int
    public let colorSpace: String
    public let normalY: String?

    func valid(for role: WorkspaceMaterialRole) -> Bool {
        mime == "image/png" && (1...Self.byteLimit).contains(bytes)
            && nativeImportDigest(sha256) && (1...8192).contains(width) && (1...8192).contains(height)
            && width * height <= 4_194_304 && colorSpace == (role == .color ? "srgb" : "linear")
            && normalY == (role == .normal ? "up" : nil)
    }

    /// The server remains responsible for decoding and validating the image.
    public init(snapshot: ImportFileSnapshot, role: WorkspaceMaterialRole) throws {
        let header = Array(snapshot.data.prefix(29))
        guard snapshot.mime == "image/png", header.count == 29,
              header.prefix(16).elementsEqual([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82]),
              header[24] == 8, [2, 6].contains(header[25]), header[28] == 0 else {
            throw AuthoringFailure("MATERIAL_MAP_DESCRIPTOR", role: role.rawValue)
        }
        func integer(at index: Int) -> Int {
            header[index..<(index + 4)].reduce(0) { ($0 << 8) | Int($1) }
        }
        mime = snapshot.mime; bytes = snapshot.data.count; sha256 = snapshot.sha256
        width = integer(at: 16); height = integer(at: 20)
        colorSpace = role == .color ? "srgb" : "linear"
        normalY = role == .normal ? "up" : nil
        guard valid(for: role) else { throw AuthoringFailure("MATERIAL_MAP_DESCRIPTOR", role: role.rawValue) }
    }
}

public struct WorkspaceMaterialDeclaration: Codable, Equatable, Sendable {
    public let requestId: String
    public let kind: WorkspaceMaterialKind
    public let name: String
    public let rights: ArtworkMetadata.Rights
    public let provenance: String
    public let maps: [String: WorkspaceMaterialMap]

    public init(requestId: String, kind: WorkspaceMaterialKind, name: String, rights: ArtworkMetadata.Rights,
                provenance: String, maps: [String: WorkspaceMaterialMap]) {
        self.requestId = requestId; self.kind = kind; self.name = name
        self.rights = rights; self.provenance = provenance; self.maps = maps
    }

    func check() throws {
        guard UUID(uuidString: requestId)?.uuidString.lowercased() == requestId,
              workspaceMaterialFieldsValid(name: name, provenance: provenance, maps: maps) else {
            throw AuthoringFailure("MATERIAL_DECLARATION")
        }
    }

    public func pendingAfterCreateFailure(_ failure: AuthoringFailure) -> Self? {
        // Preserve the exact declaration when a retry could find an existing intake.
        switch (failure.status, failure.code) {
        case (0, "MATERIAL_DECLARATION"), (400, "MATERIAL_DECLARATION"), (409, "MATERIAL_INTAKE_LIMIT"):
            return nil
        default:
            return self
        }
    }
}

private func workspaceMaterialFieldsValid(name: String, provenance: String, maps: [String: WorkspaceMaterialMap]) -> Bool {
    (1...256).contains(name.unicodeScalars.count) && (1...4096).contains(provenance.unicodeScalars.count)
        && maps["color"] != nil && maps.allSatisfy { key, map in
            WorkspaceMaterialRole(rawValue: key).map { map.valid(for: $0) } ?? false
        }
}

public struct WorkspaceMaterial: Codable, Equatable, Sendable {
    public let id: String
    public let version: Int
    public let kind: WorkspaceMaterialKind
    public let name: String
    public let rights: ArtworkMetadata.Rights
    public let provenance: String
    public let maps: [String: WorkspaceMaterialMap]

    func check() throws {
        guard UUID(uuidString: id)?.uuidString.lowercased() == id, (1...2_147_483_647).contains(version),
              workspaceMaterialFieldsValid(name: name, provenance: provenance, maps: maps) else {
            throw AuthoringFailure("MATERIAL_RESPONSE_INVALID")
        }
    }
}

public struct WorkspaceMaterialSummary: Codable, Sendable {
    public struct Availability: Codable, Sendable { public let display: Bool, download: Bool, export: Bool }
    public let id: String
    public let version: Int
    public let kind: WorkspaceMaterialKind
    public let name: String
    public let rightsAvailability: Availability
}

public struct WorkspaceMaterialIntake: Codable, Sendable {
    public enum Phase: String, Codable, Sendable {
        case uploading, validating, cancelling, registered, failed, cancelled
    }
    public struct Receipt: Codable, Sendable { public let bytes: Int, sha256: String }
    public struct Failure: Codable, Sendable { public let code: String, role: WorkspaceMaterialRole?, field: String? }
    public let intakeId: String
    public let requestId: String
    public let kind: WorkspaceMaterialKind
    public let phase: Phase
    public let received: [String: Receipt]
    public let retryable: Bool
    public let error: Failure?
    public let material: WorkspaceMaterial?

    public func check(id: String? = nil, declaration: WorkspaceMaterialDeclaration? = nil) throws {
        guard UUID(uuidString: intakeId)?.uuidString.lowercased() == intakeId,
              UUID(uuidString: requestId)?.uuidString.lowercased() == requestId, id == nil || id == intakeId,
              (phase == .registered) == (material != nil), !retryable || phase == .failed,
              received.allSatisfy({ WorkspaceMaterialRole(rawValue: $0.key) != nil
                  && (1...WorkspaceMaterialMap.byteLimit).contains($0.value.bytes) && nativeImportDigest($0.value.sha256) }) else {
            throw AuthoringFailure("MATERIAL_RESPONSE_INVALID")
        }
        if let error, !AuthoringFailure.safeCode(error.code) || !AuthoringFailure.safeField(error.field) {
            throw AuthoringFailure("MATERIAL_RESPONSE_INVALID")
        }
        if let material {
            try material.check()
            guard material.kind == kind, material.maps.count == received.count,
                  received.allSatisfy({ material.maps[$0.key]?.bytes == $0.value.bytes
                      && material.maps[$0.key]?.sha256 == $0.value.sha256 }) else {
                throw AuthoringFailure("MATERIAL_RESPONSE_INVALID")
            }
        }
        if let declaration {
            guard declaration.requestId == requestId, declaration.kind == kind,
                  received.allSatisfy({ declaration.maps[$0.key]?.bytes == $0.value.bytes
                      && declaration.maps[$0.key]?.sha256 == $0.value.sha256 }),
                  material.map({ $0.name == declaration.name && $0.rights == declaration.rights
                      && $0.provenance == declaration.provenance && $0.maps == declaration.maps }) ?? true else {
                throw AuthoringFailure("MATERIAL_RESPONSE_INVALID")
            }
        }
    }
}

public enum WorkspaceMaterialIntakeAction: String, Sendable {
    case complete, cancel, retry
}
