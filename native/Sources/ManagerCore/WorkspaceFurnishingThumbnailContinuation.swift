// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A private client intent, never a server receipt or permission grant.
/// Resumption must fetch the current registered descriptor and API authority.
public struct NativeFurnishingThumbnailPending: Codable, Sendable, Equatable {
    public enum Kind: String, Sendable { case capture, selection }
    public let value: DocumentValue
    public let kind: Kind
    public let furnishingId: String
    public let requestId: String
    private static var invalid: AuthoringFailure { AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_INVALID") }
    private static func closed(_ value: DocumentValue, _ keys: Set<String>) -> Bool {
        guard case .object(let fields) = value else { return false }; return Set(fields.keys) == keys
    }
    private static func uuid(_ value: DocumentValue) -> Bool { value.string.flatMap(UUID.init(uuidString:)) != nil }

    public init(capture: WorkspaceFurnishingThumbnailSubmission) throws {
        try self.init(value: .object(["kind": .string("capture"), "furnishingId": .string(capture.request.furnishingId),
            "furnishingVersion": .integer(1), "requestId": .string(capture.request.requestId),
            "capture": capture.captureMetadata, "pngBase64": .string(capture.pngData.base64EncodedString())]))
    }
    public init(candidate: WorkspaceFurnishingThumbnailCandidate, requestId: String, expectedSelectionVersion: Int64) throws {
        try self.init(value: .object(["kind": .string("selection"), "furnishingId": .string(candidate.source.furnishingId),
            "furnishingVersion": .integer(1), "requestId": .string(requestId), "candidate": candidate.value,
            "expectedSelectionVersion": .integer(expectedSelectionVersion)]))
    }
    public init(value: DocumentValue) throws {
        guard let name = value["kind"].string, let kind = Kind(rawValue: name),
              Self.uuid(value["furnishingId"]), Self.uuid(value["requestId"]), value["furnishingVersion"] == .integer(1),
              let furnishingId = value["furnishingId"].string, let requestId = value["requestId"].string else { throw Self.invalid }
        let common: Set<String> = ["kind", "furnishingId", "furnishingVersion", "requestId"]
        switch kind {
        case .capture:
            guard Self.closed(value, common.union(["capture", "pngBase64"])),
                  Self.closed(value["capture"], ["workspaceId", "nativeRevision", "editVersion", "baseETag", "request", "png"]),
                  Self.closed(value["capture"]["png"], ["mime", "width", "height", "bytes", "sha256"]),
                  value["capture"]["request"]["furnishingId"] == .string(furnishingId),
                  value["capture"]["request"]["furnishingVersion"] == .integer(1),
                  value["capture"]["request"]["requestId"] == .string(requestId),
                  value["capture"]["png"]["mime"] == .string("image/png"),
                  value["capture"]["png"]["width"] == .integer(256), value["capture"]["png"]["height"] == .integer(256),
                  let base64 = value["pngBase64"].string, base64.utf8.count <= 699_052,
                  let data = Data(base64Encoded: base64), data.base64EncodedString() == base64,
                  data.count <= 512 * 1024, value["capture"]["png"]["bytes"] == .integer(Int64(data.count)),
                  value["capture"]["png"]["sha256"] == .string(nativeImportSHA256(data)),
                  try GeneratedThumbnailPNG.canonical(data) == data else { throw Self.invalid }
        case .selection:
            guard Self.closed(value, common.union(["candidate", "expectedSelectionVersion"])),
                  let version = value["expectedSelectionVersion"].integer, (0...2_147_483_646).contains(version),
                  Self.closed(value["candidate"], ["candidateId", "state", "source", "thumbnail", "receipt", "receiptSha256"]),
                  value["candidate"]["state"] == .string("ready"),
                  value["candidate"]["source"]["furnishingId"] == .string(furnishingId),
                  value["candidate"]["source"]["furnishingVersion"] == .integer(1) else { throw Self.invalid }
        }
        self.value = value; self.kind = kind; self.furnishingId = furnishingId; self.requestId = requestId
    }
    public func requireScope(_ document: NativeDocumentState) throws {
        let request = kind == .capture ? value["capture"]["request"] : value["candidate"]["receipt"]["renderRequest"]
        let workspace = kind == .capture ? value["capture"]["workspaceId"] : value["candidate"]["receipt"]["workspaceId"]
        guard workspace == .string(document.workspaceId), request["tenantId"] == .string(document.tenantId),
              request["principalId"] == .string(document.principalId) else {
            throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_SCOPE_CHANGED")
        }
    }
    public func submission(in document: NativeDocumentState, furnishing: WorkspaceFurnishing) throws -> WorkspaceFurnishingThumbnailSubmission {
        try requireScope(document)
        guard kind == .capture, furnishing.id == furnishingId, let raw = value["pngBase64"].string,
              let data = Data(base64Encoded: raw) else { throw Self.invalid }
        return try WorkspaceFurnishingThumbnailSubmission(capture: value["capture"], pngData: data, document: document, furnishing: furnishing)
    }
    public func candidate(in document: NativeDocumentState, furnishing: WorkspaceFurnishing) throws -> WorkspaceFurnishingThumbnailCandidate {
        try requireScope(document)
        guard kind == .selection, furnishing.id == furnishingId else { throw Self.invalid }
        return try WorkspaceFurnishingThumbnailCandidate(value: value["candidate"], document: document, furnishing: furnishing)
    }
    public var expectedSelectionVersion: Int64? { value["expectedSelectionVersion"].integer }
    public init(from decoder: Decoder) throws { try self.init(value: DocumentValue(from: decoder)) }
    public func encode(to encoder: Encoder) throws { try value.encode(to: encoder) }
}

/// One bounded intent per selected workspace, held by the existing native
/// owner store. The journal revision is separate from exhibition revisions.
public struct NativeFurnishingThumbnailState: Codable, Sendable {
    public let format: Int
    public let revision: UInt64
    public let workspaceId: String, tenantId: String, principalId: String
    public var pending: NativeFurnishingThumbnailPending?

    public init(document: NativeDocumentState) {
        format = 1; revision = 0; workspaceId = document.workspaceId
        tenantId = document.tenantId; principalId = document.principalId; pending = nil
    }
    public init(from decoder: Decoder) throws {
        let raw = try DocumentValue(from: decoder)
        guard case .object(let fields) = raw, Set(fields.keys) == ["format", "revision", "workspaceId", "tenantId", "principalId", "pending"],
              raw["format"] == .integer(1), let revision = raw["revision"].integer, revision >= 0,
              let workspaceId = raw["workspaceId"].string, let tenantId = raw["tenantId"].string,
              let principalId = raw["principalId"].string,
              [workspaceId, tenantId, principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else {
            throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_INVALID")
        }
        format = 1; self.revision = UInt64(revision); self.workspaceId = workspaceId
        self.tenantId = tenantId; self.principalId = principalId
        pending = raw["pending"] == .null ? nil : try NativeFurnishingThumbnailPending(value: raw["pending"])
        if let pending {
            let request = pending.kind == .capture ? pending.value["capture"]["request"] : pending.value["candidate"]["receipt"]["renderRequest"]
            let workspace = pending.kind == .capture ? pending.value["capture"]["workspaceId"] : pending.value["candidate"]["receipt"]["workspaceId"]
            guard workspace == .string(workspaceId), request["tenantId"] == .string(tenantId), request["principalId"] == .string(principalId) else {
                throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_SCOPE_CHANGED")
            }
        }
    }
    public func requireScope(_ document: NativeDocumentState) throws {
        guard format == 1, [workspaceId, tenantId, principalId] == [document.workspaceId, document.tenantId, document.principalId] else {
            throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_SCOPE_CHANGED")
        }
        try pending?.requireScope(document)
    }
    public func encode(to encoder: Encoder) throws {
        guard revision <= UInt64(Int64.max) else { throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_INVALID") }
        try DocumentValue.object(["format": .integer(1), "revision": .integer(Int64(revision)),
            "workspaceId": .string(workspaceId), "tenantId": .string(tenantId), "principalId": .string(principalId),
            "pending": pending?.value ?? .null]).encode(to: encoder)
    }
}
