// SPDX-License-Identifier: Apache-2.0
import Foundation

struct NativeFloorRecordingAttachment: Sendable {
    static let profile = "saved-head-floor-recording-attachment-v1"
    static let commandType = "attach-floor-recording"
    private static let entranceSurface = "bdedd1da-0a03-442d-a58d-181877fe0652"
    private static let entranceMaterial = "cc0bfbc1-b1c8-4250-9014-dfdb6467b810"
    private static let entranceRecording = "989659bd0b4e01e1cd80287e3c5932f2220cafa07485d6d1b1d23907dedcbce4"

    enum Scope {
        case entrance
        case selectedFloor
    }

    let surfaceID: String
    let materialID: String
    let materialVersion: Int64
    let audioID: String
    let expectedMedia: DocumentValue
    let initializeMissingStoneRow: Bool

    init(_ command: DocumentValue, scope: Scope = .entrance) throws {
        let invalid = AuthoringFailure("FLOOR_RECORDING_TARGET_INVALID")
        guard case .object(let fields) = command,
              let surfaceID = command["surfaceId"].string, let materialID = command["materialId"].string,
              let audioID = command["audioId"].string,
              [surfaceID, materialID, audioID].allSatisfy({ UUID(uuidString: $0)?.uuidString.lowercased() == $0 }),
              let version = command["materialVersion"].integer, (1...Int64(Int32.max)).contains(version),
              command["audioRevision"] == .integer(1),
              case .object = command["expectedMediaAsset"] else { throw invalid }
        let media = command["expectedMediaAsset"]
        guard media["id"] == .string(audioID), media["mime"] == .string("audio/wav"),
              let bytes = media["bytes"].integer, bytes > 0,
              let digest = media["sha256"].string, nativeImportDigest(digest) else { throw invalid }
        let initialize = fields["initializeMissingStoneRow"]
        switch scope {
        case .entrance:
            guard initialize == nil, surfaceID == Self.entranceSurface, materialID == Self.entranceMaterial,
                  version == 1, digest == Self.entranceRecording, bytes == 60_146 else { throw invalid }
        case .selectedFloor:
            guard initialize == nil || initialize == .bool(true),
                  media["path"] == .string("media/\(audioID)/audio.wav"),
                  media["rights"]["permissions"]["display"] == .bool(true),
                  media["rights"]["permissions"]["export"] == .bool(true) else { throw invalid }
        }
        self.surfaceID = surfaceID
        self.materialID = materialID
        materialVersion = version
        self.audioID = audioID
        expectedMedia = media
        initializeMissingStoneRow = initialize == .bool(true)
    }

    func checkClosingScope() throws {
        guard surfaceID == "5537fdc6-44a8-45c2-a867-a3481a754796",
              materialID == "5e97508e-ae16-4e54-a3a9-44c7bb30424d", materialVersion == 1,
              expectedMedia["bytes"] == .integer(59_564),
              expectedMedia["sha256"] == .string("9fb1851c98c8394c1639ace68e9a9091989cf3eac92ba2037b20798921e49226") else {
            throw AuthoringFailure("FLOOR_RECORDING_TARGET_INVALID")
        }
    }

    func expectedCandidate(in draft: NativeExhibitionDraft) throws -> DocumentValue {
        let target = FloorSettings.Target(kind: .surface, id: surfaceID)
        let surfaces = draft.candidate["surfaces"].items?.filter { $0["id"].string?.lowercased() == surfaceID }
        guard surfaces?.count == 1, surfaces?.first?["id"] == .string(surfaceID),
              surfaces?.first?["type"] == .string("floor"),
              try FloorSettings.targets(in: draft.candidate).contains(where: { $0.key == target.key }),
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
        let bindings = materials["bindings"].items?.filter { $0["surfaceId"].string?.lowercased() == surfaceID }
        guard materials["version"] == .integer(1), bindings?.count == 1,
              bindings?.first?["surfaceId"] == .string(surfaceID),
              bindings?.first?["materialId"] == .string(materialID),
              bindings?.first?["materialVersion"] == .integer(materialVersion),
              let descriptors = materials["materials"].items,
              descriptors.filter({ $0["id"] == .string(materialID) && $0["version"] == .integer(materialVersion)
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
        let conflict = AuthoringFailure("FLOOR_RECORDING_ALREADY_BOUND")
        var previousAsset = DocumentValue.null
        if let index = matches.first {
            guard matches.count == 1, footsteps[index]["surfaceId"] == .string(surfaceID),
                  footsteps[index]["material"] == .string("stone"), footsteps[index]["bank"] == .null else { throw conflict }
            previousAsset = footsteps[index]["assetId"]
            guard previousAsset == .null || previousAsset == .string(audioID) else { throw conflict }
            footsteps[index]["assetId"] = .string(audioID)
        } else {
            guard initializeMissingStoneRow else { throw conflict }
            footsteps.append(.object(["surfaceId": .string(surfaceID), "material": .string("stone"), "assetId": .string(audioID)]))
        }
        let existing = media.filter { $0["id"].string?.lowercased() == audioID }
        guard existing.isEmpty || existing == [expectedMedia] else { throw AuthoringFailure("AUDIO_APPROVAL_CHANGED") }
        if existing.isEmpty {
            guard previousAsset == .null, media.count < 32 else { throw AuthoringFailure("AUDIO_APPROVAL_CHANGED") }
            media.append(expectedMedia)
        }
        experience["footsteps"] = .array(footsteps)
        candidate["mediaAssets"] = .array(media)
        candidate["extensions"][namespace] = experience
        guard try JSONEncoder().encode(candidate).count <= 1_048_576 else { throw AuthoringFailure("EXHIBITION_SIZE_LIMIT") }
        return candidate
    }

    func proposal(for document: NativeDocumentState) -> DocumentValue {
        var proposal: DocumentValue = .object(["version": .integer(1), "epoch": .string(UUID().uuidString.lowercased()), "sequence": .integer(1),
                 "tenantId": .string(document.tenantId), "principalId": .string(document.principalId),
                 "requestId": .string(UUID().uuidString.lowercased()), "type": .string(Self.commandType),
                 "surfaceId": .string(surfaceID), "materialId": .string(materialID), "materialVersion": .integer(materialVersion),
                 "audioId": .string(audioID), "audioRevision": .integer(1), "expectedMediaAsset": expectedMedia])
        if initializeMissingStoneRow { proposal["initializeMissingStoneRow"] = .bool(true) }
        return proposal
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
