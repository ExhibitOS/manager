// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class FloorSettingsTests: XCTestCase {
    private func fixture() throws -> (NativeExhibitionDraft, FloorSettings.Target) {
        var draft = try NativeExhibitionDraft.new()
        let id = UUID().uuidString, room = draft.rooms[0]["id"].string!
        var candidate = draft.candidate
        candidate["surfaces"] = .array([.object(["id": .string(id), "type": .string("floor"), "roomId": .string(room)])])
        candidate["extensions"] = .object(["opaque.example/data": .object(["untouched": .integer(9)])])
        try draft.replaceCandidate(candidate)
        return (draft, FloorSettings.Target(kind: .surface, id: id.lowercased()))
    }
    private func preset(_ preset: FloorSettings.Preset, draft: NativeExhibitionDraft, target: FloorSettings.Target, fields: Set<FloorSettings.Field> = Set(FloorSettings.Field.allCases)) throws -> FloorSettings {
        try FloorSettings.applying(preset: preset, defaults: FloorSettings.defaults(preset), fields: fields, target: target, prior: nil, candidate: draft.candidate)
    }
    func testWholePresetWritesAppearanceAndSoundWithCanonicalReferenceAndNoop() throws {
        var (draft, target) = try fixture()
        let noBinding = draft
        try draft.setFloor(nil, target: target)
        XCTAssertEqual(draft, noBinding)
        let settings = try preset(.wood, draft: draft, target: target)
        try draft.setFloor(settings, target: target)
        let read = try XCTUnwrap(FloorSettings.read(from: draft, target: target))
        XCTAssertEqual(read.target.id, draft.candidate["surfaces"].items![0]["id"].string)
        XCTAssertEqual(read.value["sound"], .object(["mode": .string("material-default"), "gain": .number(1)]))
        XCTAssertEqual(read.value["repeatMeters"], .vector([1.8, 1.8]))
        XCTAssertEqual(draft.candidate["extensions"]["opaque.example/data"]["untouched"], .integer(9))
        let unchanged = draft
        try draft.setFloor(read, target: target)
        XCTAssertEqual(draft, unchanged)
    }
    func testAppearanceOnlyPreservesAbsentLegacySoundAndAuthoredPresenceCounts() throws {
        var (draft, target) = try fixture()
        var candidate = draft.candidate
        candidate["extensions"]["org.exhibitos.studio/materials"] = .object(["version": .integer(1), "surfaces": .object([target.id: .object(["color": .string("#ffffff"), "roughness": .number(1), "metalness": .number(0)])])])
        candidate["extensions"]["org.exhibitos.viewer/experience"] = .object(["footsteps": .array([.object(["surfaceId": .string(target.id), "material": .string("stone")])]), "rooms": .array([.object(["roomId": draft.rooms[0]["id"], "reverb": .number(0.2)])])])
        try draft.replaceCandidate(candidate)
        let customized = try FloorSettings.customizedFields(target: target, candidate: candidate)
        XCTAssertTrue(Set([FloorSettings.Field.color, .roughness, .metalness, .sound]).isSubset(of: customized))
        let settings = try preset(.carpet, draft: draft, target: target, fields: [.texturePack])
        XCTAssertEqual(settings.value["sound"], .null)
        try draft.setFloor(settings, target: target)
        XCTAssertEqual(draft.candidate["extensions"]["org.exhibitos.viewer/experience"], candidate["extensions"]["org.exhibitos.viewer/experience"])
        let reset = try FloorSettings.applying(preset: settings.preset, defaults: FloorSettings.defaults(settings.preset), fields: [.roughness, .sound], target: target, prior: settings, candidate: draft.candidate)
        XCTAssertEqual(reset.value["materialOverride"]["roughness"], .number(1))
        XCTAssertEqual(reset.value["sound"]["mode"], .string("material-default"))
    }
    func testScopedPresetFreezesUnselectedInheritedValuesAndDefaultSound() throws {
        let (draft, target) = try fixture()
        let wood = try preset(.wood, draft: draft, target: target)
        let appearance = try FloorSettings.applying(preset: .carpet, defaults: FloorSettings.defaults(.carpet), fields: [.texturePack], target: target, prior: wood, candidate: draft.candidate)
        XCTAssertEqual(appearance.preset, .wood)
        XCTAssertEqual(appearance.value["texturePack"]["id"], .string("Carpet004"))
        XCTAssertEqual(appearance.value["sound"], wood.value["sound"])
        let audio = try FloorSettings.applying(preset: .tile, defaults: FloorSettings.defaults(.tile), fields: [.sound], target: target, prior: wood, candidate: draft.candidate)
        XCTAssertEqual(audio.preset, .tile)
        XCTAssertEqual(audio.value["texturePack"]["id"], .string("WoodFloor051"))
        XCTAssertEqual(audio.value["repeatMeters"], .vector([1.8, 1.8]))
    }
    func testTileAndMarbleAppearanceChangesPreserveUnselectedDefaultSound() throws {
        let (draft, target) = try fixture()
        for (old, next) in [(FloorSettings.Preset.tile, FloorSettings.Preset.marble), (.marble, .tile)] {
            let prior = try preset(old, draft: draft, target: target)
            let appearance = try FloorSettings.applying(preset: next, defaults: FloorSettings.defaults(next), fields: [.texturePack], target: target, prior: prior, candidate: draft.candidate)
            XCTAssertEqual(appearance.preset, old)
            XCTAssertEqual(appearance.value["sound"], prior.value["sound"])
            XCTAssertEqual(appearance.value["texturePack"], FloorSettings.defaults(next)[.texturePack])
            let audio = try FloorSettings.applying(preset: next, defaults: FloorSettings.defaults(next), fields: [.sound], target: target, prior: prior, candidate: draft.candidate)
            XCTAssertEqual(audio.preset, next)
            XCTAssertEqual(audio.value["sound"], FloorSettings.defaults(next)[.sound])
        }
    }
    func testExplicitDefaultSoundDeclaresOnlySelectedFamilyAndPreservesOtherData() throws {
        var (draft, target) = try fixture()
        let tile = try preset(.tile, draft: draft, target: target)
        try draft.setFloor(tile, target: target)
        let legacy = draft.candidate
        XCTAssertEqual(legacy["extensions"]["org.exhibitos.viewer/footstep-resources"], .null)
        try draft.setFloor(tile, target: target, declareDefaultSound: true)
        let key = "org.exhibitos.viewer/footstep-resources"
        XCTAssertEqual(draft.candidate["extensions"][key], .object(["version": .integer(1), "families": .array([.string("tile")])]))
        XCTAssertEqual(draft.candidate["extensions"]["opaque.example/data"], legacy["extensions"]["opaque.example/data"])
        let tileAgain = draft
        try draft.setFloor(tile, target: target, declareDefaultSound: true)
        XCTAssertEqual(draft, tileAgain)
        let wood = try preset(.wood, draft: draft, target: target)
        try draft.setFloor(wood, target: target, declareDefaultSound: true)
        XCTAssertEqual(draft.candidate["extensions"][key]["families"], .array([.string("tile"), .string("wood")]))
    }
    func testFootstepDeclarationRejectsUnknownOrNonDefaultWithoutChangingDraft() throws {
        var (draft, target) = try fixture()
        let tile = try preset(.tile, draft: draft, target: target)
        var bad = draft.candidate
        bad["extensions"]["org.exhibitos.viewer/footstep-resources"] = .object(["version": .integer(1), "families": .array([.string("wood"), .string("tile")])])
        try draft.replaceCandidate(bad); let original = draft
        XCTAssertThrowsError(try draft.setFloor(tile, target: target, declareDefaultSound: true))
        XCTAssertEqual(draft, original)
        var none = tile.value; none["sound"] = .object(["mode": .string("none"), "gain": .number(1)])
        let noneSettings = try FloorSettings(value: none, candidate: draft.candidate)
        XCTAssertThrowsError(try draft.setFloor(noneSettings, target: target, declareDefaultSound: true))
        XCTAssertEqual(draft, original)
    }
    func testFootstepOptInIsTransientAndExplicit() throws {
        let (draft, target) = try fixture(); let tile = try preset(.tile, draft: draft, target: target)
        var buffer = FloorFormBuffer(settings: tile)
        XCTAssertFalse(buffer.materialSoundChosen)
        buffer.selectSoundMode("material-default"); XCTAssertTrue(buffer.materialSoundChosen)
        buffer.selectSoundMode("none"); XCTAssertFalse(buffer.materialSoundChosen)
        let revision = buffer.soundChoiceRevision
        try buffer.stage(tile, replacing: [.texturePack], candidate: draft.candidate); XCTAssertFalse(buffer.materialSoundChosen)
        XCTAssertEqual(buffer.soundChoiceRevision, revision)
        try buffer.stage(tile, replacing: [.sound], candidate: draft.candidate); XCTAssertTrue(buffer.materialSoundChosen); XCTAssertNotEqual(buffer.soundChoiceRevision, revision)
        XCTAssertTrue(try buffer.declareSoundResources(available: true, requiresQualifiedRuntime: true))
        XCTAssertThrowsError(try buffer.declareSoundResources(available: false, requiresQualifiedRuntime: true))
        XCTAssertFalse(try buffer.declareSoundResources(available: false, requiresQualifiedRuntime: false))
        XCTAssertFalse(try buffer.declareSoundResources(available: true, requiresQualifiedRuntime: false))
        XCTAssertFalse(NativeFloorAudioRuntime.supports(manifestSHA256: String(repeating: "a", count: 64), ready: true))
    }
    func testFootstepCapabilityRequiresExactReadyRuntime() {
        let qualified = "46c3d7ef193e21eb8185e707cf886832ccd4d7942667407453e939b2f72ce111"
        XCTAssertTrue(NativeFloorAudioRuntime.supports(manifestSHA256: qualified, ready: true))
        XCTAssertTrue(NativeFloorRuntime.supports(manifestSHA256: qualified, ready: true))
        XCTAssertFalse(NativeFloorAudioRuntime.supports(manifestSHA256: qualified, ready: false))
        XCTAssertFalse(NativeFloorAudioRuntime.supports(manifestSHA256: nil, ready: true))
        XCTAssertFalse(NativeFloorAudioRuntime.supports(manifestSHA256: "b2ce49e9703cadf7d22b73ec5dfd93622489a07c34721f9bbe2b9f6df17ee28e", ready: true))
    }
    func testRejectUnknownBoundsDuplicateTargetAndNonWavReference() throws {
        var (draft, target) = try fixture()
        let settings = try preset(.tile, draft: draft, target: target)
        var bad = settings.value; bad["unknown"] = .bool(true)
        XCTAssertThrowsError(try FloorSettings(value: bad, candidate: draft.candidate))
        XCTAssertThrowsError(try settings.replacing(.repeatMeters, with: .vector([0, 1]), candidate: draft.candidate))
        XCTAssertThrowsError(try settings.replacing(.color, with: .string("#FFFFFF"), candidate: draft.candidate))
        let asset = UUID().uuidString.lowercased()
        var candidate = draft.candidate
        candidate["mediaAssets"] = .array([.object(["id": .string(asset), "mime": .string("audio/mpeg")])])
        let explicit: DocumentValue = .object(["mode": .string("explicit"), "gain": .number(0), "assetIds": .array([.string(asset)])])
        XCTAssertThrowsError(try settings.replacing(.sound, with: explicit, candidate: candidate))
        candidate["mediaAssets"] = .array([.object(["id": .string(asset), "mime": .string("audio/wav")])])
        XCTAssertEqual(try settings.replacing(.sound, with: explicit, candidate: candidate).value["sound"]["gain"], .number(0))
        candidate["extensions"][FloorSettings.namespace] = .object(["version": .integer(1), "bindings": .array([settings.value, settings.value])])
        XCTAssertThrowsError(try FloorSettings.all(in: candidate))
        try draft.setFloor(settings, target: target)
        var buffer = FloorFormBuffer(settings: settings); buffer.numbers[.repeatU] = "-"
        XCTAssertThrowsError(try buffer.value(candidate: draft.candidate))
        XCTAssertEqual(buffer.numbers[.repeatU], "-")
    }
    func testInactiveRoomBaseAndIdentityCopyRetainTypedReferences() throws {
        var (draft, _) = try fixture()
        let target = FloorSettings.Target(kind: .roomBase, id: draft.rooms[0]["id"].string!)
        let settings = try preset(.marble, draft: draft, target: target)
        try draft.setFloor(settings, target: target)
        XCTAssertFalse(FloorSettings.isActive(target, in: draft.candidate))
        let copy = try draft.copyForPublication()
        XCTAssertNotEqual(copy.id, draft.id)
        XCTAssertEqual(try FloorSettings.read(from: copy, target: target), settings)
        let opaque = draft.candidate["extensions"]["opaque.example/data"]
        try draft.setFloor(nil, target: target)
        XCTAssertEqual(draft.candidate["extensions"][FloorSettings.namespace], .null)
        XCTAssertEqual(draft.candidate["extensions"]["opaque.example/data"], opaque)
    }
    func testScopedResetRepairsOnlyChosenRawFieldAndRetypingOriginalStillAuthorsIt() throws {
        let (draft, target) = try fixture()
        let saved = try preset(.wood, draft: draft, target: target).replacing(.roughness, with: .number(0.25), candidate: draft.candidate)
        var buffer = FloorFormBuffer(settings: saved)
        buffer.numbers[.roughness] = "-"
        buffer.numbers[.repeatU] = "-"
        try buffer.reset(.roughness, to: .wood, candidate: draft.candidate)
        XCTAssertEqual(buffer.numbers[.roughness], "1.0")
        XCTAssertEqual(buffer.numbers[.repeatU], "-")
        XCTAssertThrowsError(try buffer.value(candidate: draft.candidate))
        buffer.numbers[.repeatU] = "1.8000"
        buffer.numbers[.roughness] = "0.25"
        XCTAssertEqual(try buffer.value(candidate: draft.candidate)?.value["materialOverride"]["roughness"], .number(0.25))
        XCTAssertEqual(buffer.numbers[.repeatU], "1.8000")
    }
    func testScopedPresetPreservesUnselectedRawFormattingAndSound() throws {
        let (draft, target) = try fixture()
        let saved = try preset(.wood, draft: draft, target: target)
        var buffer = FloorFormBuffer(settings: saved)
        buffer.numbers[.repeatU] = "1.8000"
        buffer.numbers[.gain] = "0.7500"
        let prior = try XCTUnwrap(buffer.value(candidate: draft.candidate))
        let next = try FloorSettings.applying(preset: .carpet, defaults: FloorSettings.defaults(.carpet), fields: [.texturePack], target: target, prior: prior, candidate: draft.candidate)
        try buffer.stage(next, replacing: [.texturePack], candidate: draft.candidate)
        XCTAssertEqual(buffer.numbers[.repeatU], "1.8000")
        XCTAssertEqual(buffer.numbers[.gain], "0.7500")
        XCTAssertEqual(try buffer.value(candidate: draft.candidate)?.value["sound"]["gain"], .number(0.75))
        XCTAssertEqual(try buffer.value(candidate: draft.candidate)?.preset, .wood)
    }
    func testSoundScopeFreezesInheritedAppearanceWithoutParsingEmptyRawFields() throws {
        let (draft, target) = try fixture()
        let saved = try preset(.wood, draft: draft, target: target, fields: [.texturePack])
        var buffer = FloorFormBuffer(settings: saved)
        let next = try FloorSettings.applying(preset: .tile, defaults: FloorSettings.defaults(.tile), fields: [.sound], target: target, prior: saved, candidate: draft.candidate)
        try buffer.stage(next, replacing: [.sound], candidate: draft.candidate)
        XCTAssertEqual(buffer.color, "")
        XCTAssertEqual(buffer.numbers[.roughness], "")
        let result = try XCTUnwrap(buffer.value(candidate: draft.candidate))
        XCTAssertEqual(result.preset, .tile)
        XCTAssertEqual(result.value["materialOverride"]["color"], .string("#ffffff"))
        XCTAssertEqual(result.value["texturePack"]["id"], .string("WoodFloor051"))
    }
    func testRetypingPersistedValuesAfterScopedStageReplacesStagedEdits() throws {
        let (draft, target) = try fixture()
        let saved = try preset(.wood, draft: draft, target: target)
        var buffer = FloorFormBuffer(settings: saved)
        buffer.numbers[.gain] = "0.7500"; buffer.numbers[.roughness] = "0.25"; buffer.color = "#123456"
        let edited = try XCTUnwrap(buffer.value(candidate: draft.candidate))
        let next = try FloorSettings.applying(preset: .carpet, defaults: FloorSettings.defaults(.carpet), fields: [.texturePack], target: target, prior: edited, candidate: draft.candidate)
        try buffer.stage(next, replacing: [.texturePack], candidate: draft.candidate)
        XCTAssertEqual(buffer.numbers[.gain], "0.7500")
        buffer.numbers[.gain] = "1.0"; buffer.numbers[.roughness] = "1.0"; buffer.color = "#ffffff"
        let restored = try XCTUnwrap(buffer.value(candidate: draft.candidate))
        XCTAssertEqual(restored.value["sound"]["gain"], .number(1))
        XCTAssertEqual(restored.value["materialOverride"]["roughness"], .number(1))
        XCTAssertEqual(restored.value["materialOverride"]["color"], .string("#ffffff"))
        buffer.numbers[.repeatU] = "-"
        let invalid = buffer
        XCTAssertThrowsError(try buffer.stage(next, replacing: [.texturePack], candidate: draft.candidate))
        XCTAssertEqual(buffer, invalid)
    }
    func testReturningToAbsentSoundDiscardsHiddenGainAndPreservesAppearanceInput() throws {
        let (draft, target) = try fixture()
        let saved = try preset(.wood, draft: draft, target: target, fields: [.texturePack])
        var buffer = FloorFormBuffer(settings: saved)
        buffer.color = "#123456"
        buffer.selectSoundMode("none")
        XCTAssertEqual(buffer.numbers[.gain], "1")
        buffer.selectSoundMode(nil)
        XCTAssertNil(buffer.soundMode); XCTAssertEqual(buffer.numbers[.gain], "")
        let restored = try XCTUnwrap(buffer.value(candidate: draft.candidate))
        XCTAssertEqual(restored.value["sound"], .null)
        XCTAssertEqual(restored.value["materialOverride"]["color"], .string("#123456"))
        buffer.numbers[.roughness] = "-"
        buffer.selectSoundMode("explicit"); buffer.assetIds = [UUID().uuidString]; buffer.numbers[.gain] = "-"
        buffer.selectSoundMode(nil)
        XCTAssertTrue(buffer.assetIds.isEmpty); XCTAssertEqual(buffer.numbers[.roughness], "-")
        XCTAssertThrowsError(try buffer.value(candidate: draft.candidate))
    }
}
