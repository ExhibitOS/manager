// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct FurnishingViewportRequest: Sendable {
    public let frame: ViewportFrame, command: FurnishingCommand, value: DocumentValue
    public init(frame: ViewportFrame, command: FurnishingCommand) throws {
        guard frame.selection == ViewportSelection(kind: "placement", id: command.hostId), frame.selection?.belongs(to: frame.draft) == true else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        self.frame = frame; self.command = command
        var fields = try furnishingBinding(frame)
        guard case .object(let payload) = command.value else { throw AuthoringFailure("FURNISHING_INVALID") }
        fields.merge(payload) { _, new in new }; value = .object(fields)
    }
    public func checkedReply(_ reply: DocumentValue) throws -> FurnishingPreview {
        try checkFurnishingBinding(reply, request: value, extra: ["candidate"])
        return try FurnishingPreview(candidate: reply["candidate"], base: frame.draft, command: command)
    }
}
private let furnishingBindingKeys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
private func furnishingBinding(_ frame: ViewportFrame) throws -> [String: DocumentValue] {
    guard !frame.disabled, frame.sequence > 0, [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
    return ["version": .integer(1), "epoch": .string(frame.epoch), "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId), "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased())]
}
private func checkFurnishingBinding(_ reply: DocumentValue, request: DocumentValue, extra: Set<String>) throws {
    guard case .object(let fields) = reply, Set(fields.keys) == furnishingBindingKeys.union(extra), try JSONEncoder().encode(reply).count <= 1_100_000 else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
    for key in furnishingBindingKeys { guard reply[key] == request[key] else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") } }
}
/// A separate catalog-source identity; never an artwork-thumbnail request.
public struct FurnishingThumbnailRequest: Sendable {
    public let value: DocumentValue
    public init(frame: ViewportFrame) throws {
        var fields = try furnishingBinding(frame)
        fields["catalogId"] = .string(FurnishingCatalog.id); fields["catalogVersion"] = .integer(1); fields["sha256"] = .string(FurnishingCatalog.sha256)
        value = .object(fields)
    }
    public func pngData(_ reply: DocumentValue) throws -> Data {
        try checkFurnishingBinding(reply, request: value, extra: ["catalogId", "catalogVersion", "sha256", "pngBase64", "width", "height"])
        for key in ["catalogId", "catalogVersion", "sha256"] { guard reply[key] == value[key] else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") } }
        guard reply["width"] == .integer(256), reply["height"] == .integer(256), let encoded = reply["pngBase64"].string, encoded.utf8.count <= 699_052,
              let bytes = Data(base64Encoded: encoded), bytes.base64EncodedString() == encoded, (33...524_288).contains(bytes.count),
              bytes.prefix(16) == Data([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82]) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        func dimension(_ offset: Int) -> Int { bytes[offset..<offset+4].reduce(0) { ($0 << 8) | Int($1) } }
        guard dimension(16) == 256, dimension(20) == 256 else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        return bytes
    }
}
