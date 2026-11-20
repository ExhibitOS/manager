// SPDX-License-Identifier: Apache-2.0
import Foundation

struct NativeFloorRecordingAttachment: Sendable {
    static let profile = "saved-head-floor-recording-attachment-v1"
    static let commandType = "attach-floor-recording"
    private static let entranceSurface = "bdedd1da-0a03-442d-a58d-181877fe0652"
    private static let entranceMaterial = "cc0bfbc1-b1c8-4250-9014-dfdb6467b810"
    private static let entranceRecording = "989659bd0b4e01e1cd80287e3c5932f2220cafa07485d6d1b1d23907dedcbce4"

    let surfaceID: String
    let materialID: String
    let audioID: String
    let expectedMedia: DocumentValue

    init(_ command: DocumentValue) throws {
        guard command["surfaceId"] == .string(Self.entranceSurface),
              command["materialId"] == .string(Self.entranceMaterial),
              command["materialVersion"] == .integer(1), command["audioRevision"] == .integer(1),
              let audioID = command["audioId"].string, UUID(uuidString: audioID)?.uuidString.lowercased() == audioID,
              case .object = command["expectedMediaAsset"],
              command["expectedMediaAsset"]["id"] == .string(audioID),
              command["expectedMediaAsset"]["sha256"] == .string(Self.entranceRecording),
              command["expectedMediaAsset"]["bytes"] == .integer(60_146),
              command["expectedMediaAsset"]["mime"] == .string("audio/wav") else {
            throw AuthoringFailure("FLOOR_RECORDING_TARGET_INVALID")
        }
        surfaceID = Self.entranceSurface
        materialID = Self.entranceMaterial
        self.audioID = audioID
        expectedMedia = command["expectedMediaAsset"]
    }

    func expectedCandidate(in draft: NativeExhibitionDraft) throws -> DocumentValue {
        let target = FloorSettings.Target(kind: .surface, id: surfaceID)
        guard try FloorSettings.targets(in: draft.candidate).contains(where: { $0.key == target.key }),
              FloorSettings.isActive(target, in: draft.candidate) else {
            throw AuthoringFailure("FLOOR_RECORDING_TARGET_INVALID")
        }
        let sound = try FloorSettings.all(in: draft.candidate).first(where: { $0.target.key == target.key })?.value["sound"] ?? .null
        guard sound["mode"] != .string("none") else { throw AuthoringFailure("FLOOR_RECORDING_MUTED") }
        guard sound["mode"] != .string("explicit") || (sound["assetIds"].items?.map { $0.string?.lowercased() } == [audioID]) else {
            throw AuthoringFailure("FLOOR_RECORDING_ALREADY_BOUND")
        }
        var candidate = draft.candidate
        let materials = candidate["extensions"][WorkspaceFloorMaterialBinding.namespace]
        let bindings = materials["bindings"].items?.filter { $0["surfaceId"] == .string(surfaceID) }
        guard materials["version"] == .integer(1), bindings?.count == 1,
              bindings?.first?["materialId"] == .string(materialID),
              bindings?.first?["materialVersion"] == .integer(1),
              let descriptors = materials["materials"].items,
              descriptors.filter({ $0["id"] == .string(materialID) && $0["version"] == .integer(1)
                  && $0["kind"] == .string("floor") }).count == 1 else {
            throw AuthoringFailure("FLOOR_RECORDING_TARGET_INVALID")
        }

        let namespace = ConcreteFootsteps.experienceNamespace
        var experience = candidate["extensions"][namespace]
        guard experience["version"] == .integer(1), var footsteps = experience["footsteps"].items,
              var media = candidate["mediaAssets"].items, media.count <= 32 else {
            throw AuthoringFailure("FLOOR_RECORDING_TARGET_INVALID")
        }
        let matches = footsteps.indices.filter { footsteps[$0]["surfaceId"].string?.lowercased() == surfaceID }
        guard matches.count == 1, let index = matches.first,
              footsteps[index]["surfaceId"] == .string(surfaceID), footsteps[index]["material"] == .string("stone"),
              footsteps[index]["bank"] == .null else { throw AuthoringFailure("FLOOR_RECORDING_ALREADY_BOUND") }
        let previousAsset = footsteps[index]["assetId"]
        guard previousAsset == .null || previousAsset == .string(audioID) else {
            throw AuthoringFailure("FLOOR_RECORDING_ALREADY_BOUND")
        }
        let existing = media.filter { $0["id"].string?.lowercased() == audioID }
        guard existing.isEmpty || existing == [expectedMedia] else { throw AuthoringFailure("AUDIO_APPROVAL_CHANGED") }
        if existing.isEmpty {
            guard previousAsset == .null, media.count < 32 else { throw AuthoringFailure("AUDIO_APPROVAL_CHANGED") }
            media.append(expectedMedia)
        }
        footsteps[index]["assetId"] = .string(audioID)
        experience["footsteps"] = .array(footsteps)
        candidate["mediaAssets"] = .array(media)
        candidate["extensions"][namespace] = experience
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }

    func proposal(for document: NativeDocumentState) -> DocumentValue {
        .object(["version": .integer(1), "epoch": .string(UUID().uuidString.lowercased()), "sequence": .integer(1),
                 "tenantId": .string(document.tenantId), "principalId": .string(document.principalId),
                 "requestId": .string(UUID().uuidString.lowercased()), "type": .string(Self.commandType),
                 "surfaceId": .string(surfaceID), "materialId": .string(materialID), "materialVersion": .integer(1),
                 "audioId": .string(audioID), "audioRevision": .integer(1), "expectedMediaAsset": expectedMedia])
    }

    func checkReply(_ reply: DocumentValue, proposal: DocumentValue, expected: DocumentValue) throws {
        let envelope: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]
        guard case .object(let fields) = reply, Set(fields.keys) == envelope.union(["surfaceId", "candidate"]),
              try JSONEncoder().encode(reply).count <= 1_100_000, envelope.allSatisfy({ reply[$0] == proposal[$0] }),
              reply["surfaceId"] == .string(surfaceID), reply["candidate"] == expected else {
            throw AuthoringFailure("VIEWPORT_DOCUMENT_CHANGED")
        }
    }
}
