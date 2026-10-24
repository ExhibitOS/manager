// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Native fields for the existing Viewer entrance extension. This is a bounded
/// editor adapter, not artwork approval, rights review or full OES validation.
/// The title remains NativeExhibitionDraft.title and is edited through rename.
public struct ViewerEntranceSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.viewer/entrance"
    public static let maximumImageBytes: Int64 = 8 * 1024 * 1024

    public enum MobilePosition: String, CaseIterable, Sendable {
        case center, top, bottom
    }
    public struct ImageChoice: Identifiable, Equatable, Sendable {
        public let id: String
        public let title: String
    }

    public var artist: String
    public var introduction: String
    public var imageAssetId: String?
    public var imageAlt: String
    public var mobilePosition: MobilePosition

    public init(artist: String = "", introduction: String = "", imageAssetId: String? = nil, imageAlt: String = "", mobilePosition: MobilePosition = .center) {
        self.artist = artist
        self.introduction = introduction
        self.imageAssetId = imageAssetId
        self.imageAlt = imageAlt
        self.mobilePosition = mobilePosition
    }

    public init(draft: NativeExhibitionDraft) throws {
        let extensions = try Self.extensions(in: draft.candidate)
        guard let value = extensions[Self.namespace] else { self.init(); return }
        guard case .object(let fields) = value else { throw Self.invalid() }
        var keys: Set<String> = ["version", "artist", "introduction", "imageAlt", "mobilePosition"]
        if fields["imageAssetId"] != nil { keys.insert("imageAssetId") }
        guard Set(fields.keys) == keys, fields["version"] == .integer(1),
              let artist = fields["artist"]?.string,
              let introduction = fields["introduction"]?.string,
              let imageAlt = fields["imageAlt"]?.string,
              let rawPosition = fields["mobilePosition"]?.string,
              let position = MobilePosition(rawValue: rawPosition),
              fields["imageAssetId"] == nil || fields["imageAssetId"]?.string != nil else { throw Self.invalid() }
        self.init(artist: artist, introduction: introduction, imageAssetId: fields["imageAssetId"]?.string, imageAlt: imageAlt, mobilePosition: position)
        try validate(in: draft.candidate)
    }

    private static func invalid() -> AuthoringFailure { AuthoringFailure("VIEWER_ENTRANCE_INVALID") }

    private static func extensions(in candidate: DocumentValue) throws -> [String: DocumentValue] {
        guard case .object(let fields) = candidate else { throw invalid() }
        guard let value = fields["extensions"] else { return [:] }
        guard case .object(let extensions) = value else { throw invalid() }
        return extensions
    }

    private static func validID(_ id: String) -> Bool {
        UUID(uuidString: id)?.uuidString.lowercased() == id.lowercased()
    }

    private static func matchingImage(_ id: String, in candidate: DocumentValue) -> DocumentValue? {
        guard validID(id) else { return nil }
        let matches = (candidate["artworks"].items ?? []).flatMap { artwork in
            (artwork["assets"].items ?? []).filter { $0["id"].string?.lowercased() == id.lowercased() }.map { (artwork, $0) }
        }
        guard matches.count == 1, let (artwork, asset) = matches.first,
              artwork["artworkType"].string == "image",
              artwork["primaryAssetId"].string?.lowercased() == id.lowercased(),
              asset["role"].string == "image", asset["mime"].string == "image/png",
              let bytes = asset["bytes"].double, bytes.isFinite, bytes.rounded(.towardZero) == bytes,
              bytes >= 1, bytes <= Double(maximumImageBytes) else { return nil }
        return artwork
    }

    private func validate(in candidate: DocumentValue) throws {
        // JavaScript's contract counts UTF-16 code units, not Swift graphemes.
        guard artist.utf16.count <= 512, introduction.utf16.count <= 4096, imageAlt.utf16.count <= 2048 else { throw Self.invalid() }
        if let imageAssetId {
            // Match ECMAScript trim, including BOM, without widening the field.
            let whitespace = CharacterSet(charactersIn: "\u{0009}\u{000A}\u{000B}\u{000C}\u{000D}\u{0020}\u{00A0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}")
            guard !imageAlt.trimmingCharacters(in: whitespace).isEmpty,
                  Self.matchingImage(imageAssetId, in: candidate) != nil else { throw Self.invalid() }
        }
    }

    private var value: DocumentValue {
        var fields: [String: DocumentValue] = ["version": .integer(1), "artist": .string(artist), "introduction": .string(introduction), "imageAlt": .string(imageAlt), "mobilePosition": .string(mobilePosition.rawValue)]
        if let imageAssetId { fields["imageAssetId"] = .string(imageAssetId) }
        return .object(fields)
    }

    fileprivate static func imageChoices(in draft: NativeExhibitionDraft) -> [ImageChoice] {
        draft.artworks.compactMap { artwork in
            guard let id = artwork["primaryAssetId"].string, matchingImage(id, in: draft.candidate) != nil else { return nil }
            return ImageChoice(id: id, title: artwork["metadata"]["title"].string ?? "작품")
        }
    }

    fileprivate func apply(to draft: inout NativeExhibitionDraft) throws {
        // Refuse malformed/future entrance fields rather than silently discard
        // them. Other namespaces and every opaque candidate field stay intact.
        _ = try ViewerEntranceSettings(draft: draft)
        try validate(in: draft.candidate)
        var candidate = draft.candidate
        var extensions = try Self.extensions(in: candidate)
        if extensions[Self.namespace] == value { return }
        extensions[Self.namespace] = value
        candidate["extensions"] = .object(extensions)
        try draft.replaceCandidate(candidate)
    }
}

extension NativeExhibitionDraft {
    public func viewerEntranceSettings() throws -> ViewerEntranceSettings { try ViewerEntranceSettings(draft: self) }
    /// These choices are existing exhibition snapshots only. Platform still
    /// verifies current approval, immutable revision and display rights on save.
    public func viewerEntranceImageChoices() -> [ViewerEntranceSettings.ImageChoice] { ViewerEntranceSettings.imageChoices(in: self) }
    public mutating func setViewerEntrance(_ settings: ViewerEntranceSettings) throws { try settings.apply(to: &self) }
}
