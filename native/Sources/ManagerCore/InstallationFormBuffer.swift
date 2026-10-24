// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Keeps incomplete numeric text separate from committed installation metadata.
/// Native form ownership decides Apply/Undo and selection changes; the Viewer
/// alone resolves affine geometry and alignment.
public struct InstallationFormBuffer: Equatable, Sendable {
    public enum Field: String, CaseIterable, Sendable {
        case x, y, z, yawDegrees, width, depth, height, diameter
        case pedestalRoughness, pedestalMetalness
        case borderWidth, thickness, frameDepth, gap, matWidth
        case frameRoughness, frameMetalness
    }
    public var pedestalKind: InstallationSettings.Pedestal.Kind
    public var frameKind: InstallationSettings.Frame.Kind
    public var pedestalPreset: InstallationSettings.Preset
    public var framePreset: InstallationSettings.Preset
    public var pedestalColor: String
    public var frameColor: String
    public var matColor: String
    public var fields: [Field: String]
    private let initial: InstallationSettings?

    public init(settings: InstallationSettings?) {
        initial = settings
        let p = settings?.pedestal ?? .init(), f = settings?.frame ?? .init()
        pedestalKind = p.kind; frameKind = f.kind
        pedestalPreset = p.material.preset; framePreset = f.material.preset
        pedestalColor = p.material.color; frameColor = f.material.color; matColor = f.matColor
        func text(_ value: Double) -> String { String(value) }
        fields = [.x: p.position.count == 3 ? text(p.position[0]) : "",
                  .y: p.position.count == 3 ? text(p.position[1]) : "",
                  .z: p.position.count == 3 ? text(p.position[2]) : "",
                  .yawDegrees: text(p.yaw * 180 / .pi), .width: text(p.width), .depth: text(p.depth),
                  .height: text(p.height), .diameter: text(p.diameter),
                  .pedestalRoughness: text(p.material.roughness), .pedestalMetalness: text(p.material.metalness),
                  .borderWidth: text(f.borderWidth), .thickness: text(f.thickness), .frameDepth: text(f.depth),
                  .gap: text(f.gap), .matWidth: text(f.matWidth), .frameRoughness: text(f.material.roughness),
                  .frameMetalness: text(f.material.metalness)]
    }
    public mutating func applyMaterialPreset(_ preset: InstallationSettings.Preset, frame: Bool) {
        guard !frame || preset != .stone else { return }
        let material = InstallationSettings.Material.defaults(preset)
        if frame {
            framePreset = preset; frameColor = material.color
            fields[.frameRoughness] = String(material.roughness); fields[.frameMetalness] = String(material.metalness)
        } else {
            pedestalPreset = preset; pedestalColor = material.color
            fields[.pedestalRoughness] = String(material.roughness); fields[.pedestalMetalness] = String(material.metalness)
        }
    }
    private func number(_ field: Field, locale: Locale) throws -> Double {
        var text = (fields[field] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if let separator = locale.decimalSeparator, separator != "." { text = text.replacingOccurrences(of: separator, with: ".") }
        guard !text.isEmpty, text.utf8.allSatisfy({ (48...57).contains($0) || [43, 45, 46, 69, 101].contains($0) }), let value = Double(text), value.isFinite else { throw AuthoringFailure("INSTALLATION_NUMBER_REQUIRED") }
        return value
    }
    public func value(locale: Locale = .current) throws -> InstallationSettings? {
        if self == Self(settings: initial) { return initial }
        let pedestal: InstallationSettings.Pedestal?
        if pedestalKind == .none { pedestal = initial?.pedestal.map { _ in .init() } }
        else {
            pedestal = .init(kind: pedestalKind, position: try [.x, .y, .z].map { try number($0, locale: locale) },
                yaw: fields[.yawDegrees] == Self(settings: initial).fields[.yawDegrees] ? (initial?.pedestal?.yaw ?? 0) : try number(.yawDegrees, locale: locale) * .pi / 180,
                width: pedestalKind == .box ? try number(.width, locale: locale) : 1,
                depth: pedestalKind == .box ? try number(.depth, locale: locale) : 1,
                height: try number(.height, locale: locale),
                diameter: pedestalKind == .cylinder ? try number(.diameter, locale: locale) : 1,
                material: .init(preset: pedestalPreset, color: pedestalColor, roughness: try number(.pedestalRoughness, locale: locale), metalness: try number(.pedestalMetalness, locale: locale)))
        }
        let frame: InstallationSettings.Frame?
        if frameKind == .none { frame = initial?.frame.map { _ in .init() } }
        else {
            frame = .init(kind: frameKind, borderWidth: try number(.borderWidth, locale: locale), thickness: try number(.thickness, locale: locale), depth: try number(.frameDepth, locale: locale), gap: frameKind == .floating ? try number(.gap, locale: locale) : 0, matWidth: try number(.matWidth, locale: locale), matColor: matColor,
                material: .init(preset: framePreset, color: frameColor, roughness: try number(.frameRoughness, locale: locale), metalness: try number(.frameMetalness, locale: locale)))
        }
        if pedestal == nil && frame == nil && initial == nil { return nil }
        return .init(pedestal: pedestal, frame: frame)
    }
}
