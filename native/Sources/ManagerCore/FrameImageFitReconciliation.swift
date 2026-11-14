// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A bounded viewport proposal. Platform validation and saved-head authority
/// remain with the caller; this value never grants permission or saves a draft.
public struct FrameImageFitReconciliationRequest: Sendable {
    public static let profile = "saved-head-frame-image-fit-v1"
    /// A local availability hint only; saved-head and API admission remain authoritative.
    public static func supports(placementId: String, in draft: NativeExhibitionDraft) -> Bool {
        (try? FrameImageFitCheck.validate(draft.candidate, target: placementId)) != nil
    }
    public let value: DocumentValue
    private let frame: ViewportFrame
    private let placementId: String

    public init(placementId: String, frame: ViewportFrame) throws {
        guard workspaceFurnishingID(placementId), !frame.disabled, frame.sequence > 0, frame.sequence <= 9_007_199_254_740_991,
              [frame.epoch, frame.tenantId, frame.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
              frame.selection == ViewportSelection(kind: "placement", id: placementId),
              frame.selection?.belongs(to: frame.draft) == true else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        _ = try FrameImageFitCheck.validate(frame.draft.candidate, target: placementId)
        self.frame = frame; self.placementId = placementId
        value = .object(["version": .integer(1), "epoch": .string(frame.epoch),
            "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId),
            "principalId": .string(frame.principalId), "requestId": .string(UUID().uuidString.lowercased()),
            "type": .string("reconcile-frame-image-fit"), "placementId": .string(placementId)])
    }

    public func checkedReply(_ reply: DocumentValue) throws -> FrameImageFitReconciliationPreview {
        let bindings: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
        guard case .object(let fields) = reply, Set(fields.keys) == bindings.union(["placementId", "candidate"]),
              try JSONEncoder().encode(reply).count <= 1_100_000,
              bindings.allSatisfy({ FrameImageFitCheck.exact(reply[$0], value[$0]) }),
              reply["placementId"] == .string(placementId) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        let base = frame.draft.candidate, candidate = reply["candidate"]
        let previous = try FrameImageFitCheck.validate(base, target: placementId)
        let next = try FrameImageFitCheck.validate(candidate, target: placementId)
        guard let newFit = next.fit else { throw AuthoringFailure("FRAME_IMAGE_FIT_INVALID") }
        if let existing = previous.fit, !FrameImageFitCheck.exact(existing, newFit) {
            throw AuthoringFailure("FRAME_IMAGE_FIT_CONFLICT")
        }
        var expected = base
        if previous.fit == nil {
            var rows = previous.rows
            rows[previous.index]["imageFit"] = newFit
            if base["extensions"][WorkspaceFurnishingPlacement.namespace]["version"] != .integer(11) {
                expected["extensions"][WorkspaceFurnishingPlacement.namespace]["version"] = .integer(10)
            }
            expected["extensions"][WorkspaceFurnishingPlacement.namespace]["frameAttachments"] = .array(rows)
        }
        guard FrameImageFitCheck.exact(candidate, expected) else { throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED") }
        return FrameImageFitReconciliationPreview(base: frame.draft, candidate: candidate,
                                                   placementId: placementId, changed: previous.fit == nil)
    }
}

public struct FrameImageFitReconciliationPreview: Sendable {
    public let base: NativeExhibitionDraft
    public let candidate: DocumentValue
    public let placementId: String
    public let changed: Bool

    public func apply(to current: NativeExhibitionDraft) throws -> NativeExhibitionDraft? {
        guard FrameImageFitCheck.exact(current.document, base.document) else { throw AuthoringFailure("VIEWPORT_FRAME_STALE") }
        guard changed else { return nil }
        var result = current
        try result.replaceCandidate(candidate)
        return result
    }
}

private enum FrameImageFitCheck {
    struct Target { let rows: [DocumentValue], index: Int, fit: DocumentValue? }
    private static let namespace = WorkspaceFurnishingPlacement.namespace
    private static func fail(_ code: String) -> AuthoringFailure { AuthoringFailure(code) }
    private static func fields(_ value: DocumentValue, _ keys: Set<String>) -> Bool {
        guard case .object(let object) = value else { return false }; return Set(object.keys) == keys
    }
    private static func present(_ object: DocumentValue, _ key: String) -> DocumentValue? {
        guard case .object(let values) = object else { return nil }; return values[key]
    }
    static func validate(_ candidate: DocumentValue, target: String) throws -> Target {
        let ext = candidate["extensions"][namespace]
        guard let version = ext["version"].integer, [7, 9, 10, 11].contains(version),
              fields(ext, version == 11 ? ["version", "furnishings", "placements", "frameAttachments", "openingAttachments", "ceilingAttachments"] : ["version", "furnishings", "placements", "frameAttachments", "openingAttachments"]),
              ext["placements"].items != nil, ext["openingAttachments"].items != nil,
              ext["furnishings"].items != nil, let rows = ext["frameAttachments"].items,
              rows.count <= 128 else { throw fail("FRAME_IMAGE_FIT_INVALID") }
        if version == 11 { try WorkspaceCeilingRail.validate(candidate: candidate) }
        let indices = rows.indices.filter { rows[$0]["placementId"] == .string(target) }
        guard indices.count == 1 else { throw fail("FRAME_IMAGE_FIT_STALE") }
        var seen: [String: DocumentValue] = [:]
        for row in rows {
            if let fit = present(row, "imageFit") {
                guard version == 10 || version == 11 else { throw fail("FRAME_IMAGE_FIT_INVALID") }
                let identity = try validateRow(row, candidate: candidate, fit: fit)
                if let old = seen[identity], !exact(old, fit) { throw fail("FRAME_IMAGE_FIT_CONFLICT") }
                seen[identity] = fit
            }
        }
        let row = rows[indices[0]]
        _ = try validateRow(row, candidate: candidate, fit: present(row, "imageFit"))
        return Target(rows: rows, index: indices[0], fit: present(row, "imageFit"))
    }
    private static func validateRow(_ row: DocumentValue, candidate: DocumentValue, fit: DocumentValue?) throws -> String {
        let keys: Set<String> = ["placementId", "artworkRevisionId", "assetId", "furnishingId", "furnishingVersion", "modelSha256"]
        guard fields(row, fit == nil ? keys : keys.union(["imageFit"])),
              let placementId = row["placementId"].string, let revision = row["artworkRevisionId"].string,
              let assetId = row["assetId"].string, let furnishingId = row["furnishingId"].string,
              [placementId, revision, assetId, furnishingId].allSatisfy(workspaceFurnishingID),
              row["furnishingVersion"] == .integer(1), let modelHash = row["modelSha256"].string,
              nativeImportDigest(modelHash) else { throw fail("FRAME_IMAGE_FIT_INVALID") }
        let ext = candidate["extensions"][namespace]
        let placements = candidate["placements"].items?.filter { $0["id"] == .string(placementId) } ?? []
        let arts = candidate["artworks"].items?.filter { $0["revisionId"] == .string(revision) } ?? []
        guard ext["frameAttachments"].items?.filter({ $0["placementId"] == .string(placementId) }).count == 1,
              placements.count == 1, arts.count == 1,
              placements[0]["artworkRevisionId"] == .string(revision), placements[0]["assetId"] == .string(assetId),
              arts[0]["artworkType"] == .string("image"), arts[0]["primaryAssetId"] == .string(assetId) else { throw fail("FRAME_IMAGE_FIT_STALE") }
        let assets = arts[0]["assets"].items?.filter { $0["id"] == .string(assetId) } ?? []
        guard assets.count == 1, assets[0]["mime"] == .string("image/png"),
              let source = assets[0]["sha256"].string, nativeImportDigest(source) else { throw fail("FRAME_IMAGE_FIT_STALE") }
        let descriptors = ext["furnishings"].items?.filter { $0["id"] == .string(furnishingId) && $0["version"] == row["furnishingVersion"] } ?? []
        guard descriptors.count == 1 else { throw fail("FRAME_IMAGE_FIT_UNSUPPORTED") }
        let descriptor = descriptors[0], model = descriptor["model"], issued = descriptor["fit"], geometry = issued["geometry"]
        guard descriptor["kind"] == .string("artwork-frame"), model["sha256"] == .string(modelHash),
              let bytes = model["bytes"].integer, bytes > 0,
              fields(issued, ["version", "sourceBytes", "sourceSha256", "geometry"]), issued["version"] == .integer(1),
              issued["sourceBytes"] == model["bytes"], issued["sourceSha256"] == model["sha256"],
              fields(geometry, ["profile", "openingWidth", "openingHeight", "imagePlaneZ"]),
              let profile = geometry["profile"].string, ["straight-ring-v1", "straight-miter-ring-v1"].contains(profile),
              let w = geometry["openingWidth"].double, w.isFinite, (0.4...2).contains(w),
              let h = geometry["openingHeight"].double, h.isFinite, (0.4...2).contains(h),
              geometry["imagePlaneZ"].double == -0.012,
              let width = arts[0]["dimensions"]["width"].double, width.isFinite, (0.4...2).contains(width),
              let height = arts[0]["dimensions"]["height"].double, height.isFinite, (0.4...2).contains(height) else { throw fail("FRAME_IMAGE_FIT_UNSUPPORTED") }
        if let fit {
            guard fields(fit, ["version", "profile", "sourceSha256", "pixelWidth", "pixelHeight"]),
                  fit["version"] == .integer(1), fit["profile"] == .string("contain-contact-v1"),
                  let sourceHash = fit["sourceSha256"].string, nativeImportDigest(sourceHash),
                  let x = fit["pixelWidth"].integer, let y = fit["pixelHeight"].integer,
                  (1...8192).contains(x), (1...8192).contains(y), x * y <= 25_165_824 else { throw fail("FRAME_IMAGE_FIT_INVALID") }
            guard sourceHash == source else { throw fail("FRAME_IMAGE_FIT_STALE") }
            let ratio = Double(x) / Double(y)
            let footprintWidth = ratio >= width / height ? width : height * ratio
            let footprintHeight = ratio >= width / height ? width / ratio : height
            guard (0.4...2).contains(footprintWidth), (0.4...2).contains(footprintHeight) else { throw fail("FRAME_IMAGE_FIT_UNSUPPORTED") }
        }
        return revision + "/" + assetId
    }
    static func exact(_ lhs: DocumentValue, _ rhs: DocumentValue) -> Bool {
        switch (lhs, rhs) {
        case (.object(let a), .object(let b)): return a.count == b.count && a.allSatisfy { key, value in b[key].map { exact(value, $0) } ?? false }
        case (.array(let a), .array(let b)): return a.count == b.count && zip(a, b).allSatisfy { exact($0.0, $0.1) }
        case (.string(let a), .string(let b)): return a == b
        case (.integer(let a), .integer(let b)): return a == b
        case (.number(let a), .number(let b)): return a.bitPattern == b.bitPattern
        case (.bool(let a), .bool(let b)): return a == b
        case (.null, .null): return true
        default: return false
        }
    }
}
