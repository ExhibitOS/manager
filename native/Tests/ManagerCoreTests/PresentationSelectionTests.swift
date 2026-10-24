// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class PresentationSelectionTests: XCTestCase {
    func testVisibleSelectionChangesBeforeSaveAndLateReplyCannotReturnToPreviousScreen() throws {
        var state = PresentationSelection(); state.refresh(Presentation())
        state.select(step: .create)
        XCTAssertEqual(state.value?.step, .create)
        let first = try XCTUnwrap(state.nextSave())
        state.select(step: .select); state.select(step: .publish)
        XCTAssertEqual(state.value?.step, .publish)
        // Simulate both a stale refresh and the first asynchronous save reply.
        state.refresh(Presentation())
        XCTAssertEqual(state.value?.step, .publish)
        var firstReply = first.value; firstReply.revision += 1
        try state.acknowledge(first, stored: firstReply)
        XCTAssertEqual(state.value?.step, .publish)
        let last = try XCTUnwrap(state.nextSave())
        XCTAssertEqual(last.value.step, .publish); XCTAssertEqual(last.value.revision, 1)
        var lastReply = last.value; lastReply.revision += 1
        try state.acknowledge(last, stored: lastReply)
        // A duplicate or delayed response from the old screen is harmless.
        try state.acknowledge(first, stored: firstReply)
        state.refresh(firstReply)
        XCTAssertEqual(state.value?.step, .publish); XCTAssertEqual(state.value?.revision, 2)
        XCTAssertNil(state.nextSave())
    }
    func testModeAndScaleAreRetainedAcrossRapidNavigationAndFailedPersistence() throws {
        var state = PresentationSelection(); state.refresh(Presentation(revision: 4, step: .select))
        state.select(step: .preview); let request = try XCTUnwrap(state.nextSave())
        state.select(mode: .expert); state.select(scale: 150); state.select(step: .create)
        state.failed(request)
        XCTAssertEqual(state.value?.step, .create); XCTAssertEqual(state.value?.mode, .expert); XCTAssertEqual(state.value?.textScale, 150)
        state.refresh(Presentation(revision: 5, step: .preview))
        XCTAssertEqual(state.value?.step, .create)
        let retry = try XCTUnwrap(state.nextSave()); XCTAssertEqual(retry.value.revision, 5)
        var stored = retry.value; stored.revision += 1
        try state.acknowledge(retry, stored: stored)
        var reopened = PresentationSelection(); reopened.refresh(stored)
        XCTAssertEqual(reopened.value, state.value)
        XCTAssertNil(reopened.nextSave())
    }
}
