// SPDX-License-Identifier: Apache-2.0
import Foundation

private enum FurnishingThumbnailRecord {
    static var invalid: AuthoringFailure { AuthoringFailure("CATALOG_PREVIEW_INVALID") }
    static func closed(_ value: DocumentValue, _ keys: Set<String>) -> Bool {
        guard case .object(let fields) = value else { return false }; return Set(fields.keys) == keys
    }
    static func uuid(_ value: String?) -> Bool {
        guard let value, value.utf8.count == 36 else { return false }; return UUID(uuidString: value) != nil
    }
    static func scope(_ document: NativeDocumentState) throws {
        guard [document.workspaceId, document.tenantId, document.principalId].allSatisfy({ uuid($0) }) else { throw invalid }
    }
}

/// Issued source hashes are retained and bound, never recreated as authority.
public struct WorkspaceFurnishingThumbnailSource: Equatable, Sendable {
    public let value: DocumentValue
    public let furnishingId: String, modelSha256: String, descriptorSha256: String, rightsSha256: String
    public let furnishingVersion: Int, modelBytes: Int

    public init(value: DocumentValue, furnishing: WorkspaceFurnishing) throws {
        let model = furnishing.details.model
        guard FurnishingThumbnailRecord.closed(value, ["furnishingId", "furnishingVersion", "modelSha256", "modelBytes", "descriptorSha256", "rightsSha256"]),
              FurnishingThumbnailRecord.uuid(furnishing.id), furnishing.version == 1,
              value["furnishingId"] == .string(furnishing.id), value["furnishingVersion"] == .integer(1),
              model.mime == "model/gltf-binary", (28...32 * 1024 * 1024).contains(model.bytes), nativeImportDigest(model.sha256),
              value["modelSha256"] == .string(model.sha256), value["modelBytes"] == .integer(Int64(model.bytes)),
              let descriptor = value["descriptorSha256"].string, nativeImportDigest(descriptor),
              let rights = value["rightsSha256"].string, nativeImportDigest(rights) else { throw FurnishingThumbnailRecord.invalid }
        try NativeFurnishingThumbnailRules.requireDisplay(furnishing.details.rights)
        self.value = value; furnishingId = furnishing.id; furnishingVersion = 1
        modelSha256 = model.sha256; modelBytes = model.bytes; descriptorSha256 = descriptor; rightsSha256 = rights
    }
}

public struct WorkspaceFurnishingThumbnailPNG: Equatable, Sendable {
    public let value: DocumentValue
    public let sha256: String
    public let bytes: Int

    public init(value: DocumentValue) throws {
        guard FurnishingThumbnailRecord.closed(value, ["mime", "width", "height", "sha256", "bytes"]),
              value["mime"] == .string("image/png"), value["width"] == .integer(256), value["height"] == .integer(256),
              let hash = value["sha256"].string, nativeImportDigest(hash),
              let count = value["bytes"].integer, (33...512 * 1024).contains(count) else { throw FurnishingThumbnailRecord.invalid }
        self.value = value; sha256 = hash; bytes = Int(count)
    }
    /// Match fetched bytes to issued metadata, using the existing PNG validator.
    public func checkedData(_ data: Data) throws -> Data {
        guard data.count == bytes, nativeImportSHA256(data) == sha256,
              try GeneratedThumbnailPNG.canonical(data) == data else { throw FurnishingThumbnailRecord.invalid }
        return data
    }
}

/// Ready is capture availability only; selection is a separate issued record.
public struct WorkspaceFurnishingThumbnailCandidate: Equatable, Sendable {
    public let value: DocumentValue, receipt: DocumentValue
    public let candidateId: String, receiptSha256: String, workspaceId: String
    public let source: WorkspaceFurnishingThumbnailSource
    public let thumbnail: WorkspaceFurnishingThumbnailPNG
    public let request: ViewportFurnishingThumbnailRequest

    public init(value: DocumentValue, document: NativeDocumentState, furnishing: WorkspaceFurnishing,
                submission: WorkspaceFurnishingThumbnailSubmission? = nil) throws {
        try FurnishingThumbnailRecord.scope(document)
        guard FurnishingThumbnailRecord.closed(value, ["candidateId", "state", "source", "thumbnail", "receipt", "receiptSha256"]),
              let id = value["candidateId"].string, FurnishingThumbnailRecord.uuid(id), value["state"] == .string("ready"),
              let hash = value["receiptSha256"].string, nativeImportDigest(hash) else { throw FurnishingThumbnailRecord.invalid }
        let source = try WorkspaceFurnishingThumbnailSource(value: value["source"], furnishing: furnishing)
        let thumbnail = try WorkspaceFurnishingThumbnailPNG(value: value["thumbnail"])
        let receipt = value["receipt"]
        guard FurnishingThumbnailRecord.closed(receipt, ["version", "purpose", "profile", "workspaceId", "candidateId", "requestId", "source", "renderRequest", "png"]),
              receipt["version"] == .integer(1), receipt["purpose"] == .string("registered-furnishing-thumbnail"),
              receipt["profile"] == .string(WorkspaceFurnishingThumbnailSubmission.kind),
              receipt["workspaceId"] == .string(document.workspaceId), receipt["candidateId"] == .string(id),
              receipt["source"] == source.value, receipt["png"] == thumbnail.value else { throw FurnishingThumbnailRecord.invalid }
        let request = try WorkspaceFurnishingThumbnailSubmission.checkedRenderRequest(receipt["renderRequest"], document: document, furnishing: furnishing)
        guard receipt["requestId"] == .string(request.requestId) else { throw FurnishingThumbnailRecord.invalid }
        if let submission {
            guard submission.workspaceId == document.workspaceId, submission.request == request,
                  submission.pngSha256 == thumbnail.sha256, submission.pngBytes == thumbnail.bytes else { throw FurnishingThumbnailRecord.invalid }
        }
        self.value = value; self.receipt = receipt; candidateId = id; receiptSha256 = hash
        workspaceId = document.workspaceId; self.source = source; self.thumbnail = thumbnail; self.request = request
    }
    public func checkedPNG(_ data: Data) throws -> Data { try thumbnail.checkedData(data) }
}

/// GET is authoritative. A historical POST replay need not contain the latest
/// selectionVersion; this value alone never claims freshness or mutation.
public struct WorkspaceFurnishingThumbnailSelection: Equatable, Sendable {
    public struct Selected: Equatable, Sendable {
        public let value: DocumentValue
        public let candidateId: String, receiptSha256: String
        public let png: WorkspaceFurnishingThumbnailPNG
        fileprivate init(_ value: DocumentValue) throws {
            guard FurnishingThumbnailRecord.closed(value, ["candidateId", "receiptSha256", "png"]),
                  let id = value["candidateId"].string, FurnishingThumbnailRecord.uuid(id),
                  let hash = value["receiptSha256"].string, nativeImportDigest(hash) else { throw FurnishingThumbnailRecord.invalid }
            self.value = value; candidateId = id; receiptSha256 = hash
            png = try WorkspaceFurnishingThumbnailPNG(value: value["png"])
        }
    }
    public let value: DocumentValue
    public let source: WorkspaceFurnishingThumbnailSource
    public let selectionVersion: Int64
    public let selected: Selected?
    public let workspaceId: String, tenantId: String, principalId: String

    public init(value: DocumentValue, document: NativeDocumentState, furnishing: WorkspaceFurnishing) throws {
        try FurnishingThumbnailRecord.scope(document)
        guard FurnishingThumbnailRecord.closed(value, ["source", "selectionVersion", "selected"]),
              let version = value["selectionVersion"].integer, version >= 0 else { throw FurnishingThumbnailRecord.invalid }
        let selected = value["selected"] == .null ? nil : try Selected(value["selected"])
        guard selected == nil || version > 0 else { throw FurnishingThumbnailRecord.invalid }
        source = try WorkspaceFurnishingThumbnailSource(value: value["source"], furnishing: furnishing)
        self.value = value; selectionVersion = version; self.selected = selected
        workspaceId = document.workspaceId; tenantId = document.tenantId; principalId = document.principalId
    }
    public func matches(_ candidate: WorkspaceFurnishingThumbnailCandidate) -> Bool {
        guard let selected else { return false }
        return workspaceId == candidate.workspaceId && tenantId == candidate.request.tenantId && principalId == candidate.request.principalId
            && source == candidate.source && selected.candidateId == candidate.candidateId
            && selected.receiptSha256 == candidate.receiptSha256 && selected.png == candidate.thumbnail
    }
}
