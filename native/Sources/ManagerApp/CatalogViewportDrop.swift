// SPDX-License-Identifier: Apache-2.0
import AppKit
import ManagerCore
import SwiftUI
import UniformTypeIdentifiers

enum CatalogDropKind: String {
    case artwork, floor, frame, light
}

enum CatalogDropPhase: String {
    case hover, drop, clear
}

enum CatalogDropHit: Equatable {
    case artwork
    case floor(roomID: String, position: [Double])
    case frame(String)
    case light(String)
}

struct CatalogDropQuery {
    let kind: CatalogDropKind
    let phase: CatalogDropPhase
    let draft: NativeExhibitionDraft
    var point: CGPoint?
    var placementRequestID: String?

    func value(frame: ViewportFrame, requestID: String) -> DocumentValue {
        var fields: [String: DocumentValue] = [
            "version": .integer(1), "requestId": .string(requestID), "epoch": .string(frame.epoch),
            "sequence": .integer(Int64(frame.sequence)), "tenantId": .string(frame.tenantId),
            "principalId": .string(frame.principalId), "targetKind": .string(kind.rawValue),
            "phase": .string(phase.rawValue)
        ]
        if let point {
            fields["point"] = .object(["x": .number(Double(point.x)), "y": .number(Double(point.y))])
        }
        if let placementRequestID { fields["placementRequestId"] = .string(placementRequestID) }
        return .object(fields)
    }

    func checkedReply(_ reply: DocumentValue, frame: ViewportFrame, requestID: String) throws -> CatalogDropHit? {
        let invalid = AuthoringFailure("CATALOG_DROP_INVALID")
        guard case .object(let envelope) = reply, envelope.count == 5, envelope["hit"] != nil, reply["version"] == .integer(1), reply["requestId"] == .string(requestID),
              reply["epoch"] == .string(frame.epoch), reply["sequence"] == .integer(Int64(frame.sequence)) else { throw invalid }
        let hit = reply["hit"]
        if hit == .null { return nil }
        guard phase != .clear, hit["kind"].string == kind.rawValue, case .object(let fields) = hit else { throw invalid }
        switch kind {
        case .artwork:
            guard fields.count == 1 else { throw invalid }
            return .artwork
        case .floor:
            guard fields.count == 3, let roomID = hit["roomId"].string, UUID(uuidString: roomID) != nil,
                  let position = hit["position"].items?.compactMap(\.double), position.count == 3,
                  hit["position"].items?.count == 3, position.allSatisfy(\.isFinite) else { throw invalid }
            return .floor(roomID: roomID, position: position)
        case .frame:
            guard fields.count == 2, let id = hit["placementId"].string, UUID(uuidString: id) != nil else { throw invalid }
            return .frame(id)
        case .light:
            guard fields.count == 2, let id = hit["lightId"].string, UUID(uuidString: id) != nil else { throw invalid }
            return .light(id)
        }
    }
}

enum CatalogDragItem: Sendable {
    case artwork(ArtworkCatalogEntry)
    case workspace(WorkspaceFurnishingSummary)
    case trackSpot

    init?(furnishing row: BottomFurnishingCatalogRow) {
        switch row {
        case .workspace(let entry):
            switch entry.kind {
            case .pedestal, .bench, .stool, .partition, .barrier, .displayCase, .artworkFrame:
                self = .workspace(entry)
            case .infoStand, .portalFinish:
                return nil
            }
        case .shared("museum-track-spot", _, .light):
            self = .trackSpot
        default:
            return nil
        }
    }

    var kind: CatalogDropKind {
        switch self {
        case .artwork: .artwork
        case .workspace(let entry): entry.kind == .artworkFrame ? .frame : .floor
        case .trackSpot: .light
        }
    }
}

@MainActor
final class CatalogDragSession: ObservableObject {
    static let contentType = UTType(exportedAs: "org.exhibitos.manager.catalog-drag")
    @Published private(set) var feedback = ""
    private(set) var active: Source?
    private var hoverTask: Task<Void, Never>?
    private var hoverPoint: CGPoint?
    private var hoverSequence = 0
    private var hoverValid: Bool?
    private(set) var dropping = false

    struct Source: Sendable {
        let token: UUID
        let item: CatalogDragItem
        let access: UUID
        let draft: NativeExhibitionDraft

        @MainActor func matches(_ model: ExhibitionModel) -> Bool {
            access == model.viewportAccessID && draft == model.draft
        }
    }

    func provider(for item: CatalogDragItem, model: ExhibitionModel) -> NSItemProvider {
        guard let draft = model.draft, !dropping else { return NSItemProvider() }
        cancelHover()
        let source = Source(token: UUID(), item: item, access: model.viewportAccessID, draft: draft)
        active = source
        feedback = ""
        let provider = NSItemProvider()
        let bytes = Data(source.token.uuidString.utf8)
        provider.registerDataRepresentation(forTypeIdentifier: Self.contentType.identifier, visibility: .ownProcess) { completion in
            completion(bytes, nil)
            return nil
        }
        return provider
    }

    func updateHover(at point: CGPoint, source: Source, controller: NativeViewportController) -> Bool? {
        if source.item.kind == .artwork {
            if case .artwork(let entry) = source.item {
                feedback = entry.kind == "sculpture"
                    ? "선택한 공간의 바닥에 놓을 수 있는지 확인합니다."
                    : "선택한 공간의 벽에 놓을 수 있는지 확인합니다."
            }
            return true
        }
        let sequence = controller.acceptedSequence
        if point == hoverPoint, sequence == hoverSequence { return hoverValid }
        cancelHover()
        hoverPoint = point
        hoverSequence = sequence
        feedback = "배치할 대상을 확인하는 중입니다."
        hoverTask = Task {
            let query = CatalogDropQuery(kind: source.item.kind, phase: .hover, draft: source.draft, point: point)
            let hit = try? await controller.catalogDrop(query)
            guard !Task.isCancelled, active?.token == source.token, hoverPoint == point,
                  hoverSequence == sequence, controller.acceptedSequence == sequence else { return }
            hoverValid = hit != nil
            feedback = hit == nil ? "여기에 놓을 수 없습니다." : "놓아서 선택한 비품을 배치합니다."
        }
        return nil
    }

    func leave(_ controller: NativeViewportController) {
        cancelHover()
        feedback = ""
        guard let source = active, source.item.kind != .artwork else { return }
        Task {
            _ = try? await controller.catalogDrop(.init(kind: source.item.kind, phase: .clear, draft: source.draft))
        }
    }

    func claim(_ token: UUID) -> Bool {
        guard active?.token == token, !dropping else { return false }
        cancelHover()
        active = nil
        dropping = true
        feedback = "카탈로그 항목을 배치하는 중입니다."
        return true
    }

    func complete(_ message: String) {
        dropping = false
        feedback = message
    }

    private func cancelHover() {
        hoverTask?.cancel()
        hoverTask = nil
        hoverPoint = nil
        hoverValid = nil
    }
}

@MainActor
struct CatalogViewportDrop: DropDelegate {
    let model: ExhibitionModel
    let controller: NativeViewportController
    let session: CatalogDragSession
    let surfaceSize: CGSize
    let enabled: Bool

    private var source: CatalogDragSession.Source? {
        guard enabled, !session.dropping, controller.connected, controller.catalogDropSupported,
              controller.editingContextActive, !controller.boundedReadOnly, !model.immutable,
              model.boundedPlacementCapability == nil, !model.boundedPolicyPending, !model.boundedReadOnly,
              model.canSelectCatalog, model.viewportPresentation == .threeD,
              let source = session.active, source.matches(model) else { return nil }
        return source
    }

    private func normalized(_ location: CGPoint) -> CGPoint? {
        guard surfaceSize.width > 0, surfaceSize.height > 0,
              (0...surfaceSize.width).contains(location.x), (0...surfaceSize.height).contains(location.y) else { return nil }
        return CGPoint(x: location.x / surfaceSize.width, y: location.y / surfaceSize.height)
    }

    func validateDrop(info: DropInfo) -> Bool {
        source != nil && info.itemProviders(for: [CatalogDragSession.contentType]).count == 1
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        guard let source, let point = normalized(info.location) else { return DropProposal(operation: .forbidden) }
        let valid = session.updateHover(at: point, source: source, controller: controller)
        // The drop always rechecks the live target before writing.
        return DropProposal(operation: valid == false ? .forbidden : .copy)
    }

    func dropExited(info: DropInfo) { session.leave(controller) }

    func performDrop(info: DropInfo) -> Bool {
        guard let source, let point = normalized(info.location), validateDrop(info: info),
              let provider = info.itemProviders(for: [CatalogDragSession.contentType]).first else { return false }
        let sequence = controller.acceptedSequence
        let context = controller.editingContextGeneration
        provider.loadDataRepresentation(forTypeIdentifier: CatalogDragSession.contentType.identifier) { bytes, _ in
            guard let bytes, bytes == Data(source.token.uuidString.utf8) else { return }
            Task { @MainActor in
                guard self.source?.token == source.token, controller.acceptedSequence == sequence,
                      controller.editingContextGeneration == context, session.claim(source.token) else { return }
                await place(source, at: point, sequence: sequence)
            }
        }
        return true
    }

    private func place(_ source: CatalogDragSession.Source, at point: CGPoint, sequence: Int) async {
        var placement: ArtworkPlacementIntent?
        do {
            var query = CatalogDropQuery(kind: source.item.kind, phase: .drop, draft: source.draft, point: point)
            if case .artwork(let artwork) = source.item {
                placement = await model.beginVisualArtworkPlacement(artwork, requiresSelection: false)
                guard let placement, source.matches(model), controller.acceptedSequence == sequence else { throw AuthoringFailure("CATALOG_DROP_UNAVAILABLE") }
                query.placementRequestID = placement.requestId
            }
            guard let hit = try await controller.catalogDrop(query), source.matches(model) else {
                throw AuthoringFailure("CATALOG_DROP_INVALID")
            }
            switch (source.item, hit) {
            case (.artwork, .artwork):
                guard let placement else { throw AuthoringFailure("CATALOG_DROP_INVALID") }
                // WKWebView delivers the validated placement-target event on its message channel.
                for _ in 0..<20 {
                    if model.artworkPlacementTarget?.valid == true || model.artworkPlacement?.requestId != placement.requestId { break }
                    try await Task.sleep(for: .milliseconds(25))
                }
                guard source.matches(model), model.artworkPlacement?.requestId == placement.requestId,
                      model.artworkPlacementTarget?.valid == true else { throw AuthoringFailure("CATALOG_DROP_INVALID") }
                await model.commitVisualArtworkPlacement()
                guard model.draft?.placements.contains(where: { $0["id"].string == placement.placementId }) == true else {
                    throw AuthoringFailure("CATALOG_DROP_INVALID")
                }
            case (.workspace(let entry), .floor(let roomID, let position)):
                let value = WorkspaceFurnishingPlacement(id: UUID().uuidString.lowercased(), roomId: roomID,
                    furnishingId: entry.id, furnishingVersion: entry.version, position: position, yaw: 0, scale: 1)
                guard try await model.placeWorkspaceFurnishing(value, access: source.access) else {
                    throw AuthoringFailure("CATALOG_DROP_INVALID")
                }
            case (.workspace(let entry), .frame(let id)):
                guard !model.dirty, model.workspaceArtworkFrameAvailable,
                      let target = source.draft.placements.first(where: { $0["id"].string == id }),
                      let revision = target["artworkRevisionId"].string, let assetID = target["assetId"].string else {
                    throw AuthoringFailure("FRAME_SAVED_DRAFT_REQUIRED")
                }
                model.selectViewport(.init(kind: "placement", id: id))
                _ = try await model.applyWorkspaceArtworkFrame(to: id, access: source.access) { api, state in
                    try await api.prepareSelectedWorkspaceArtworkFrame(entry, placementId: id,
                        artworkRevisionId: revision, assetId: assetID, in: state)
                }
            case (.trackSpot, .light(let id)):
                model.selectLight(id)
                model.syncSharedFurnishing()
                let deadline = ContinuousClock.now + .seconds(2)
                while !controller.catalogDropFrameMatches(source.draft, selection: nil),
                      source.matches(model), model.selectedLightId == id, ContinuousClock.now < deadline {
                    try await Task.sleep(for: .milliseconds(25))
                }
                guard source.matches(model), controller.catalogDropFrameMatches(source.draft, selection: nil),
                      model.canEditSharedFurnishing, var form = model.sharedFurnishing,
                      form.host == .light, form.hostId == id else { throw AuthoringFailure("CATALOG_DROP_INVALID") }
                if !form.selected {
                    form.selected = true
                    model.sharedFurnishing = form
                    await model.previewSharedFurnishing()
                    guard source.matches(model), model.sharedFurnishing == form, model.canApplySharedFurnishing else {
                        if source.matches(model), model.sharedFurnishing == form { model.discardSharedFurnishing() }
                        throw AuthoringFailure("CATALOG_DROP_INVALID")
                    }
                    await model.applySharedFurnishing()
                    guard model.draft != source.draft else {
                        if source.matches(model), model.sharedFurnishing == form { model.discardSharedFurnishing() }
                        throw AuthoringFailure("CATALOG_DROP_INVALID")
                    }
                }
            default:
                throw AuthoringFailure("CATALOG_DROP_INVALID")
            }
            session.complete("카탈로그 항목을 배치했습니다. 전시를 저장하거나 되돌릴 수 있습니다.")
        } catch {
            if let placement, model.artworkPlacement?.requestId == placement.requestId {
                await model.cancelVisualArtworkPlacement()
            }
            session.complete("이 위치에 배치하지 못했습니다. 대상이나 저장 상태를 확인하세요.")
        }
    }
}
