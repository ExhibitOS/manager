// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Matched shared light-object commands; existing saved-head authority and
/// explicit Save remain responsible for committing a prepared candidate.
public enum LightObjectAuthoring {
    public static let profile = "saved-head-light-object-v1"
    public static let capability = "light-object-v1"
    public static let commands: Set<String> = [
        "set-light-object", "set-room-base-light-placement", "set-light-fixture", "duplicate-light-object", "remove-light-object"
    ]
}
