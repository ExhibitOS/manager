// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Editing transport only. Recommendations, estimates and admission decisions
/// come from the shared Server analyzer; this type performs no budget math.
public struct ViewerBudgetSettings: Equatable, Sendable {
    public static let namespace = "org.exhibitos.studio/viewer-budget"
    public static let profiles = ["standard", "compact", "native"]
    public static let metrics = ["colorDraws", "displayTriangles", "shadowCasterTriangles", "shadowDepthTriangles", "residentBytes", "transitionBytes"]
    public var targetProfile: String
    public private(set) var selectedProfile: String
    public var warning: [String: String]
    public var admission: [String: String]
    private let original: DocumentValue

    public init(candidate: DocumentValue, selectedProfile: String = "standard") throws {
        guard Self.profiles.contains(selectedProfile) else { throw Self.invalid }
        let value: DocumentValue?
        if case .object(let extensions) = candidate["extensions"] { value = extensions[Self.namespace] } else { value = nil }
        let policy = value ?? DocumentValue.object(["version": .integer(1), "policyVersion": .integer(1), "profiles": .object([:])])
        try Self.validate(policy)
        original = policy
        targetProfile = policy["targetProfile"].string ?? "auto"
        self.selectedProfile = selectedProfile
        let overrides = policy["profiles"][selectedProfile]
        warning = Self.textFields(overrides["warning"])
        admission = Self.textFields(overrides["admission"])
    }

    private static var invalid: AuthoringFailure { AuthoringFailure("VIEWER_BUDGET_POLICY_INVALID") }
    private static func textFields(_ value: DocumentValue) -> [String: String] {
        Dictionary(uniqueKeysWithValues: metrics.compactMap { key in
            value[key].integer.map { (key, String($0)) }
        })
    }
    private static func numbers(_ fields: [String: String], minimum: Int64) throws -> DocumentValue {
        guard Set(fields.keys).isSubset(of: Set(metrics)) else { throw invalid }
        var result: [String: DocumentValue] = [:]
        for (key, text) in fields {
            let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if text.isEmpty { continue }
            guard text.allSatisfy({ $0.isASCII && $0.isNumber }), let number = Int64(text),
                  number >= minimum, number <= 9_007_199_254_740_991 else { throw invalid }
            result[key] = .integer(number)
        }
        return .object(result)
    }
    private static func validate(_ policy: DocumentValue) throws {
        guard case .object(let fields) = policy,
              Set(fields.keys).isSubset(of: ["version", "policyVersion", "profiles", "targetProfile"]),
              policy["version"] == .integer(1), policy["policyVersion"] == .integer(1),
              case .object(let profiles) = policy["profiles"], Set(profiles.keys).isSubset(of: Set(Self.profiles)),
              !fields.keys.contains("targetProfile") || ["auto", "standard", "compact"].contains(policy["targetProfile"].string ?? "") else { throw invalid }
        for overrides in profiles.values {
            guard case .object(let groups) = overrides, Set(groups.keys).isSubset(of: ["warning", "admission"]) else { throw invalid }
            for (group, values) in groups {
                guard case .object(let limits) = values, Set(limits.keys).isSubset(of: Set(metrics)) else { throw invalid }
                for value in limits.values {
                    guard let number = value.integer, number >= (group == "warning" ? 0 : 1), number <= 9_007_199_254_740_991 else { throw invalid }
                }
            }
        }
    }

    /// Returns the exact protected command payload. Blank fields retain the
    /// shared recommendation rather than storing a guessed Native default.
    public func command(resetSelectedProfile: Bool = false) throws -> DocumentValue {
        guard ["auto", "standard", "compact"].contains(targetProfile), Self.profiles.contains(selectedProfile),
              case .object(var policy) = original, case .object(var profiles) = original["profiles"] else { throw Self.invalid }
        if resetSelectedProfile { profiles.removeValue(forKey: selectedProfile) }
        else {
            let warning = try Self.numbers(warning, minimum: 0), admission = try Self.numbers(admission, minimum: 1)
            var groups: [String: DocumentValue] = [:]
            if warning != .object([:]) { groups["warning"] = warning }
            if admission != .object([:]) { groups["admission"] = admission }
            if groups.isEmpty { profiles.removeValue(forKey: selectedProfile) }
            else { profiles[selectedProfile] = .object(groups) }
        }
        policy["profiles"] = .object(profiles)
        let persistedTarget = resetSelectedProfile ? original["targetProfile"].string ?? "auto" : targetProfile
        if persistedTarget == "auto" { policy.removeValue(forKey: "targetProfile") }
        else { policy["targetProfile"] = .string(persistedTarget) }
        let value: DocumentValue = profiles.isEmpty && persistedTarget == "auto" ? .null : .object(policy)
        return .object(["type": .string("set-viewer-budget-policy"), "policy": value])
    }
    public static var resetCommand: DocumentValue {
        .object(["type": .string("set-viewer-budget-policy"), "policy": .null])
    }
}
