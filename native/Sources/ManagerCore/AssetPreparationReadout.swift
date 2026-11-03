// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A bounded sample of the existing loader's last stages, not a load history or
/// a diagnosis. The caller must pin the sample to the current authenticated ACK.
public enum AssetPreparationReadout {
    public static let maximumEntries = 32
    public static let maximumBytes = 16_384

    /// Missing legacy fields make the sample unavailable. A null revision in a
    /// row means that its asset cannot be attributed to one admitted revision;
    /// in that case no claimed revision or error code is forwarded.
    public static func projected(_ json: String?, in draft: NativeExhibitionDraft) -> DocumentValue {
        guard let json, json.utf8.count <= maximumBytes,
              let value = try? JSONDecoder().decode(DocumentValue.self, from: Data(json.utf8)),
              case .object(let fields) = value, Set(fields.keys) == ["preparationStages"],
              let rows = fields["preparationStages"]?.items, rows.count <= maximumEntries else { return .null }
        var bindings: [UUID: Set<UUID>] = [:]
        for artwork in draft.artworks {
            guard let text = artwork["revisionId"].string, let revision = UUID(uuidString: text) else { continue }
            for asset in artwork["assets"].items ?? [] {
                guard let text = asset["id"].string, let id = UUID(uuidString: text) else { continue }
                bindings[id, default: []].insert(revision)
            }
        }
        var seen = Set<UUID>(), result: [DocumentValue] = []
        for row in rows {
            guard case .object(let fields) = row,
                  Set(fields.keys) == ["assetId", "artworkRevisionId", "stage", "errorCode"],
                  let text = fields["assetId"]?.string, let asset = UUID(uuidString: text),
                  seen.insert(asset).inserted, let revisions = bindings[asset],
                  let stage = token(fields["stage"], pattern: "^[a-z][a-z0-9-]{0,63}$") else { return .null }
            let revision: UUID?
            if fields["artworkRevisionId"] == .null { revision = nil }
            else if let text = fields["artworkRevisionId"]?.string, let id = UUID(uuidString: text) { revision = id }
            else { return .null }
            let code: DocumentValue
            if fields["errorCode"] == .null { code = .null }
            else if let text = token(fields["errorCode"], pattern: "^[A-Z][A-Z0-9_]{0,79}$"),
                    ["failed", "deferred-budget"].contains(stage) { code = .string(text) }
            else { return .null }
            let attributed = revision.map { revisions.count == 1 && revisions.contains($0) } ?? false
            result.append(.object([
                "assetId": .string(asset.uuidString.lowercased()),
                "artworkRevisionId": attributed ? .string(revision!.uuidString.lowercased()) : .null,
                "stage": .string(stage), "errorCode": attributed ? code : .null
            ]))
        }
        return .object(["preparationStages": .array(result)])
    }

    private static func token(_ value: DocumentValue?, pattern: String) -> String? {
        guard let text = value?.string, let range = text.range(of: pattern, options: .regularExpression),
              range == text.startIndex..<text.endIndex else { return nil }
        return text
    }
}
