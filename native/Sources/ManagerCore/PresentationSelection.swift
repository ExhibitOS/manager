// SPDX-License-Identifier: Apache-2.0
import Foundation

/// The visible selection is current user intent. Persistence acknowledges a
/// revision; it never supplies an older screen after the user has moved on.
public struct PresentationSelection: Sendable {
    public struct Save: Equatable, Sendable {
        public let generation: UInt64
        public let value: Presentation
    }
    public private(set) var value: Presentation?
    private var generation: UInt64 = 0
    private var savedGeneration: UInt64 = 0
    private var active: Save?
    public var hasPendingSave: Bool { generation != savedGeneration }
    public init() {}
    public mutating func refresh(_ stored: Presentation) {
        guard var current = value else { value = stored; return }
        // A status refresh in flight must not move the selected sidebar row or
        // reset mode/scale. A save owns its revision until it is acknowledged.
        guard active == nil, stored.revision >= current.revision else { return }
        current.revision = stored.revision; value = current
        if current == stored { savedGeneration = generation }
    }
    @discardableResult
    public mutating func select(mode: Mode? = nil, step: Step? = nil, scale: Int? = nil) -> Bool {
        guard var next = value else { return false }
        let old = next
        if let mode { next.mode = mode }; if let step { next.step = step }; if let scale { next.textScale = scale }
        guard next != old else { return false }
        generation += 1; value = next; return true
    }
    public mutating func nextSave() -> Save? {
        guard active == nil, generation != savedGeneration, let value else { return nil }
        let request = Save(generation: generation, value: value); active = request; return request
    }
    public mutating func acknowledge(_ request: Save, stored: Presentation) throws {
        guard active == request else { return }
        var expected = request.value
        guard expected.revision < UInt64.max else { throw BridgeFailure("NATIVE_PROTOCOL_INVALID") }
        expected.revision += 1
        guard stored == expected, var current = value else { throw BridgeFailure("NATIVE_PROTOCOL_INVALID") }
        current.revision = stored.revision; value = current
        savedGeneration = request.generation; active = nil
    }
    public mutating func failed(_ request: Save) { if active == request { active = nil } }
}
