// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Metadata adapter only. The existing shared validator admits complete plane,
/// room/world transforms and Float32 geometry before native Preview/Apply.
public struct PlaqueSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/plaques"
    public enum Source: Equatable, Sendable {
        case exhibition, room, artwork(placementId: String)
        public var value: DocumentValue {
            switch self {
            case .exhibition: .object(["kind": .string("exhibition")])
            case .room: .object(["kind": .string("room")])
            case .artwork(let id): .object(["kind": .string("artwork"), "placementId": .string(id)])
            }
        }
        init(value: DocumentValue) throws {
            guard case .object(let fields) = value else { throw PlaqueSettings.invalid() }
            switch value["kind"].string {
            case "exhibition" where Set(fields.keys) == ["kind"]: self = .exhibition
            case "room" where Set(fields.keys) == ["kind"]: self = .room
            case "artwork" where Set(fields.keys) == ["kind", "placementId"]:
                guard let id = value["placementId"].string, BenchSettings.uuid(id) else { throw PlaqueSettings.invalid() }
                self = .artwork(placementId: id)
            default: throw PlaqueSettings.invalid()
            }
        }
    }
    public struct CaptionTranslation: Equatable, Sendable {
        public var locale: String, artist: String, date: String
        public var medium: String?
        /// Museum-recorded original dimensions, separate from the plaque plane.
        public var dimensions: String?

        public init(locale: String = "", artist: String = "", date: String = "", medium: String? = nil, dimensions: String? = nil) {
            self.locale = locale; self.artist = artist; self.date = date
            self.medium = medium
            self.dimensions = dimensions
        }

        fileprivate init(value: DocumentValue) throws {
            guard case .object(let fields) = value, Set(fields.keys).subtracting(["medium", "dimensions"]) == ["locale", "artist", "date"],
                  let locale = value["locale"].string, let artist = value["artist"].string, let date = value["date"].string,
                  fields["medium"] == nil || fields["medium"]?.string != nil,
                  fields["dimensions"] == nil || fields["dimensions"]?.string != nil else { throw PlaqueSettings.invalid() }
            self.init(locale: locale, artist: artist, date: date, medium: fields["medium"]?.string, dimensions: fields["dimensions"]?.string)
        }

        fileprivate var value: DocumentValue {
            var fields: [String: DocumentValue] = ["locale": .string(locale), "artist": .string(artist), "date": .string(date)]
            if let medium { fields["medium"] = .string(medium) }
            if let dimensions { fields["dimensions"] = .string(dimensions) }
            return .object(fields)
        }
    }
    public let id: String, roomId: String
    public let source: Source
    public let position: [Double], rotation: [Double] // Unit quaternion; plane front +Z.
    public let width: Double, height: Double
    public let visible: Bool
    public let captionTranslations: [CaptionTranslation]
    public init(id: String, roomId: String, source: Source, position: [Double], rotation: [Double], width: Double, height: Double, visible: Bool,
                captionTranslations: [CaptionTranslation] = []) throws {
        let artwork: Bool
        if case .artwork(let id) = source {
            guard BenchSettings.uuid(id) else { throw Self.invalid() }
            artwork = true
        } else {
            artwork = false
        }
        guard BenchSettings.uuid(id), BenchSettings.uuid(roomId), BenchSettings.point(position),
              rotation.count == 4, rotation.allSatisfy(\.isFinite),
              abs(sqrt(rotation.reduce(0) { $0 + $1 * $1 }) - 1) <= 1e-6,
              BenchSettings.bounded(width, (artwork ? 0.15 : 0.2)...2),
              BenchSettings.bounded(height, (artwork ? 0.10 : 0.15)...1.2) else { throw Self.invalid() }
        if !captionTranslations.isEmpty {
            guard artwork, captionTranslations.count <= 8 else { throw Self.invalid() }
            var tags = Set<String>()
            for row in captionTranslations {
                guard Self.validLocale(row.locale), tags.insert(row.locale.lowercased()).inserted,
                      Self.validCaptionLine(row.artist, maximum: 80), Self.validCaptionLine(row.date, maximum: 48),
                      [row.medium, row.dimensions].compactMap({ $0 }).allSatisfy({ Self.validCaptionLine($0, maximum: 96) })
                else { throw Self.invalid() }
            }
        }
        self.id = id; self.roomId = roomId; self.source = source; self.position = position; self.rotation = rotation
        self.captionTranslations = captionTranslations
        self.width = width; self.height = height; self.visible = visible
    }
    public init(value: DocumentValue) throws {
        guard case .object(let fields) = value, Set(fields.keys).subtracting(["artworkCaption"]) == ["id", "roomId", "source", "position", "rotation", "dimensions", "visible"],
              let id = value["id"].string, let room = value["roomId"].string,
              let position = BenchSettings.numbers(value["position"], count: 3), let rotation = BenchSettings.numbers(value["rotation"], count: 4),
              case .object(let size) = value["dimensions"], Set(size.keys) == ["width", "height"],
              let width = size["width"]?.double, let height = size["height"]?.double, case .bool(let visible) = value["visible"] else { throw Self.invalid() }
        let captions: [CaptionTranslation]
        if let raw = fields["artworkCaption"] {
            guard case .object(let data) = raw, Set(data.keys) == ["translations"],
                  let rows = raw["translations"].items, (1...8).contains(rows.count) else { throw Self.invalid() }
            captions = try rows.map(CaptionTranslation.init(value:))
        } else { captions = [] }
        try self.init(id: id, roomId: room, source: Source(value: value["source"]), position: position, rotation: rotation,
                      width: width, height: height, visible: visible, captionTranslations: captions)
    }
    public var value: DocumentValue {
        var fields: [String: DocumentValue] = ["id": .string(id), "roomId": .string(roomId), "source": source.value,
            "position": .vector(position), "rotation": .vector(rotation),
            "dimensions": .object(["width": .number(width), "height": .number(height)]), "visible": .bool(visible)]
        if !captionTranslations.isEmpty {
            fields["artworkCaption"] = .object(["translations": .array(captionTranslations.map(\.value))])
        }
        return .object(fields)
    }
    static func invalid() -> AuthoringFailure { AuthoringFailure("PLAQUE_FIELDS_INVALID") }
    public static func list(in draft: NativeExhibitionDraft) throws -> [Self] {
        guard let raw = try BenchSettings.extensionValue(namespace, in: draft) else { return [] }
        guard case .object(let fields) = raw,
              let rows = raw["plaques"].items, rows.count <= 32, try JSONEncoder().encode(raw).count <= 16_384 else { throw invalid() }
        switch raw["version"] {
        case .integer(1) where Set(fields.keys) == ["version", "plaques"]: break
        case .integer(2) where Set(fields.keys) == ["version", "plaques", "roomIntroductions"]:
            guard try !roomIntroductionRows(fields["roomIntroductions"], in: draft).isEmpty else { throw invalid() }
        case .integer(3) where Set(fields.keys).subtracting(["roomIntroductions"]) == ["version", "plaques"]:
            if let introductions = fields["roomIntroductions"] {
                guard try !roomIntroductionRows(introductions, in: draft).isEmpty else { throw invalid() }
            }
        default: throw invalid()
        }
        var seen = Set<String>()
        let plaques = try rows.map { row in
            let plaque = try Self(value: row)
            guard seen.insert(plaque.id.lowercased()).inserted,
                  draft.rooms.contains(where: { $0["id"].string?.lowercased() == plaque.roomId.lowercased() }) else { throw invalid() }
            if case .artwork(let id) = plaque.source {
                guard draft.placements.contains(where: { $0["id"].string?.lowercased() == id.lowercased() && $0["roomId"].string?.lowercased() == plaque.roomId.lowercased() }) else { throw invalid() }
            }
            return plaque
        }
        guard plaques.contains(where: { !$0.captionTranslations.isEmpty }) == (raw["version"] == .integer(3)) else { throw invalid() }
        return plaques
    }
    private static func validLocale(_ locale: String) -> Bool {
        locale.range(of: #"^[A-Za-z]{2,8}(?:-[A-Za-z0-9]{1,8}){0,4}$"#, options: .regularExpression) == locale.startIndex..<locale.endIndex
    }
    private static func validCaptionLine(_ text: String, maximum: Int) -> Bool {
        text.utf16.count <= maximum && validIntroductionText(text) && !text.unicodeScalars.contains {
            (0...31).contains($0.value) || (127...159).contains($0.value) || $0.value == 0x2028 || $0.value == 0x2029
        }
    }
    static func validIntroductionText(_ text: String) -> Bool {
        let whitespace = CharacterSet(charactersIn: "\u{0009}\u{000A}\u{000B}\u{000C}\u{000D}\u{0020}\u{00A0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}")
        return text.utf16.count <= 1024 && !text.trimmingCharacters(in: whitespace).isEmpty
    }
    static func roomIntroductionRows(_ value: DocumentValue?, in draft: NativeExhibitionDraft) throws -> [String: DocumentValue] {
        guard let value else { return [:] }
        guard case .object(let rows) = value, rows.count <= 32 else { throw invalid() }
        var seen = Set<String>()
        for (roomId, row) in rows {
            guard BenchSettings.uuid(roomId), seen.insert(roomId.lowercased()).inserted,
                  draft.rooms.contains(where: { $0["id"].string?.lowercased() == roomId.lowercased() }),
                  case .object(let fields) = row, Set(fields.keys).subtracting(["translations"]) == ["text"],
                  let text = row["text"].string, validIntroductionText(text) else { throw invalid() }
            if let translations = fields["translations"] {
                guard let variants = translations.items, variants.count <= 8 else { throw invalid() }
                var tags = Set<String>()
                for variant in variants {
                    guard case .object(let data) = variant, Set(data.keys) == ["locale", "text"],
                          let locale = variant["locale"].string,
                          validLocale(locale),
                          tags.insert(locale.lowercased()).inserted,
                          let text = variant["text"].string, validIntroductionText(text) else { throw invalid() }
                }
            }
        }
        return rows
    }
    public static func roomIntroductionText(in draft: NativeExhibitionDraft, roomId: String) throws -> String {
        let raw = try BenchSettings.extensionValue(namespace, in: draft)
        guard let raw else { return "" }
        guard case .object(let fields) = raw else { throw invalid() }
        let rows = try roomIntroductionRows(fields["roomIntroductions"], in: draft)
        return rows.first(where: { $0.key.lowercased() == roomId.lowercased() })?.value["text"].string ?? ""
    }
}

/// Plaques refer to existing exhibition, room and artwork sources. Room text
/// uses the shared source for every plaque associated with that room.
/// Every raw pose/size input remains in the model buffer.
public struct PlaqueFormBuffer: Equatable, Sendable {
    public enum Facing: String, CaseIterable, Sendable { case forward, back, left, right }
    public enum SourceKind: String, CaseIterable, Sendable { case exhibition, room, artwork }
    public let id: String
    public var roomId: String
    public var sourceKind: SourceKind = .exhibition
    public var placementId = ""
    public var captionTranslations: [PlaqueSettings.CaptionTranslation] = []
    public var roomIntroductionText = ""
    private var originalRoomIntroductionText = ""
    public var roomIntroductionChanged: Bool { roomIntroductionText != originalRoomIntroductionText }
    public var x = "0.0", y = "1.5", z = "0.0"
    public var rotationX = "0.0", rotationY = "0.0", rotationZ = "0.0", rotationW = "1.0"
    public var width = "0.8", height = "0.4"
    public var visible = true
    public init(roomId: String) throws {
        guard BenchSettings.uuid(roomId) else { throw PlaqueSettings.invalid() }
        id = UUID().uuidString.lowercased(); self.roomId = roomId
    }
    public init(draft: NativeExhibitionDraft, plaqueId: String) throws {
        guard let plaque = try PlaqueSettings.list(in: draft).first(where: { $0.id == plaqueId }) else { throw AuthoringFailure("PLAQUE_SELECTION_CHANGED") }
        id = plaque.id; roomId = plaque.roomId
        try selectRoom(plaque.roomId, in: draft)
        switch plaque.source {
        case .exhibition: sourceKind = .exhibition
        case .room: sourceKind = .room
        case .artwork(let id): sourceKind = .artwork; placementId = id
        }
        x = String(plaque.position[0]); y = String(plaque.position[1]); z = String(plaque.position[2])
        rotationX = String(plaque.rotation[0]); rotationY = String(plaque.rotation[1]); rotationZ = String(plaque.rotation[2]); rotationW = String(plaque.rotation[3])
        width = String(plaque.width); height = String(plaque.height); visible = plaque.visible
        captionTranslations = plaque.captionTranslations
    }
    public mutating func selectRoom(_ roomId: String, in draft: NativeExhibitionDraft) throws {
        let original = try PlaqueSettings.roomIntroductionText(in: draft, roomId: roomId)
        self.roomId = roomId
        originalRoomIntroductionText = original
        roomIntroductionText = original
    }
    public func settings() throws -> PlaqueSettings {
        func number(_ text: String) throws -> Double {
            // These are decimal measurements, not grouped numbers. Accept the
            // two common decimal separators without modifying the raw field.
            let decimal = text.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: ",", with: ".")
            guard let value = Double(decimal), value.isFinite else { throw PlaqueSettings.invalid() }; return value
        }
        let source: PlaqueSettings.Source
        switch sourceKind { case .exhibition: source = .exhibition; case .room: source = .room; case .artwork: source = .artwork(placementId: placementId) }
        return try PlaqueSettings(id: id, roomId: roomId, source: source, position: [number(x), number(y), number(z)],
                                 rotation: [number(rotationX), number(rotationY), number(rotationZ), number(rotationW)],
                                 width: number(width), height: number(height), visible: visible, captionTranslations: captionTranslations)
    }
    public mutating func face(_ direction: Facing) {
        rotationX = "0.0"; rotationZ = "0.0"
        switch direction {
        case .forward: rotationY = "0.0"; rotationW = "1.0"
        case .back: rotationY = "1.0"; rotationW = "0.0"
        case .left: rotationY = String(-sqrt(0.5)); rotationW = String(sqrt(0.5))
        case .right: rotationY = String(sqrt(0.5)); rotationW = String(sqrt(0.5))
        }
    }
}
