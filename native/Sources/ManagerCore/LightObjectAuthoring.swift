// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Matched shared light-object commands; existing saved-head authority and
/// explicit Save remain responsible for committing a prepared candidate.
public enum LightObjectAuthoring {
    public static let profile = "saved-head-light-object-v1"
    public static let capability = "light-object-v1"
    static let fixtureFirstProfile = "saved-head-fixture-first-v1"
    public static let addWithFixture = "add-light-with-fixture"
    public static let fixtureCatalogueIDs = [
        "museum-track-spot", "museum-track-spot-fixed-64p1", "recessed-downlight",
        "museum-wallwasher", "museum-wallwasher-rear-relief", "museum-track-spot-articulated"
    ]
    // The generic chooser has no explicit rail or special-host binding.
    public static let guidedFixtureCatalogueIDs = ["museum-track-spot", "recessed-downlight", "museum-wallwasher"]

    public static let commands: Set<String> = [
        "set-light-object", "set-room-base-light-placement", "set-room-ceiling-fill-brightness", "set-light-fixture",
        "duplicate-light-object", "remove-light-object", addWithFixture
    ]
}
