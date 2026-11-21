// SPDX-License-Identifier: Apache-2.0
import Foundation

struct NativeExhibitionCreditsEdit: Sendable {
    static let operation = "workspace-exhibition-credits-apply"
    let credits: String

    init(_ value: DocumentValue) throws {
        guard let credits = value.string, credits.utf16.count <= 4096 else {
            throw AuthoringFailure("EXHIBITION_CREDITS_INVALID")
        }
        self.credits = credits
    }

    func applying(to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft? {
        let namespace = BenchSettings.presentationNamespace
        _ = try BenchViewpoint.list(in: draft)
        var candidate = draft.candidate
        guard case .object(var presentation) = candidate["extensions"][namespace],
              let before = presentation["credits"]?.string else {
            throw AuthoringFailure("EXHIBITION_CREDITS_INVALID")
        }
        guard before != credits else { return nil }
        presentation["credits"] = .string(credits)
        candidate["extensions"][namespace] = .object(presentation)
        var next = draft
        try next.replaceCandidate(candidate)
        _ = try BenchViewpoint.list(in: next)
        return next
    }
}
