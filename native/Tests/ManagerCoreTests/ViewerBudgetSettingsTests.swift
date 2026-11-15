// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewerBudgetSettingsTests: XCTestCase {
    private func candidate(_ policy: DocumentValue) -> DocumentValue {
        .object(["extensions": .object([ViewerBudgetSettings.namespace: policy])])
    }
    func testAbsenceAndBlankFieldsDoNotInventRecommendations() throws {
        var form = try ViewerBudgetSettings(candidate: .object([:]))
        XCTAssertEqual(form.targetProfile, "auto")
        XCTAssertEqual(try form.command(), ViewerBudgetSettings.resetCommand)
        form.warning["colorDraws"] = " "
        form.admission["residentBytes"] = ""
        XCTAssertEqual(try form.command(), ViewerBudgetSettings.resetCommand)
    }
    func testProfileResetPreservesOtherOverridesAndPersistedTarget() throws {
        let policy: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1), "targetProfile": .string("compact"), "profiles": .object([
            "standard": .object(["warning": .object(["colorDraws": .integer(0)])]),
            "native": .object(["admission": .object(["residentBytes": .integer(123456789)])])])])
        var form = try ViewerBudgetSettings(candidate: candidate(policy))
        form.targetProfile = "standard"
        let value = try form.command(resetSelectedProfile: true)["policy"]
        XCTAssertEqual(value["targetProfile"], .string("compact"))
        XCTAssertEqual(value["profiles"]["standard"], .null)
        XCTAssertEqual(value["profiles"]["native"], policy["profiles"]["native"])
    }
    func testOnlyClosedSafeIntegerPolicyValuesAreTransported() throws {
        var form = try ViewerBudgetSettings(candidate: .object([:]))
        for text in ["-1", "0", "1.5", "1e6", "9007199254740992", "１２"] {
            form.admission["colorDraws"] = text
            XCTAssertThrowsError(try form.command(), text)
        }
        form.admission["colorDraws"] = " 384 "
        form.warning["colorDraws"] = "0"
        XCTAssertEqual(try form.command()["policy"]["profiles"]["standard"]["warning"]["colorDraws"], .integer(0))
        form.admission["unknown"] = "1"
        XCTAssertThrowsError(try form.command())
    }
    func testProfileResetIgnoresUnappliedTargetAndLimitFields() throws {
        let policy: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1), "profiles": .object([
            "standard": .object(["warning": .object(["colorDraws": .integer(12)])])])])
        var form = try ViewerBudgetSettings(candidate: candidate(policy))
        form.targetProfile = "compact"
        form.admission["residentBytes"] = "invalid unsaved input"
        XCTAssertEqual(try form.command(resetSelectedProfile: true), ViewerBudgetSettings.resetCommand)
        XCTAssertThrowsError(try form.command())
    }
    func testStoredNativeTargetAndUnknownVersionAreRejected() {
        for (key, value) in [("targetProfile", DocumentValue.string("native")), ("policyVersion", .integer(2))] {
            var policy: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1), "profiles": .object([:])])
            policy[key] = value
            XCTAssertThrowsError(try ViewerBudgetSettings(candidate: candidate(policy)))
        }
    }
    func testExplicitNullDoesNotMasqueradeAsAbsentSharedPolicy() {
        XCTAssertThrowsError(try ViewerBudgetSettings(candidate: candidate(.null)))
        let policy: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1), "profiles": .object([:]), "targetProfile": .null])
        XCTAssertThrowsError(try ViewerBudgetSettings(candidate: candidate(policy)))
    }

}
