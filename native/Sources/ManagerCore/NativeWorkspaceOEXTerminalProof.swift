// SPDX-License-Identifier: Apache-2.0
import CryptoKit
import Foundation

/// Checks the Draft 12 terminal wire data against independently held native
/// state. The caller must establish the owner and settlement and bind the
/// reopened observation to the exact pending save request and receipt.
public enum NativeWorkspaceOEXTerminalProof {
    public static let proofByteLimit = 8 * 1024
    public static let resultByteLimit = 4 * 1024 * 1024

    public struct Scope: Decodable, Equatable, Sendable {
        public let workspaceId: String
        public let tenantId: String
        public let principalId: String
        public let jobId: String
        public let attemptId: String
        public let importRequestId: String
        public let sourceArchiveSha256: String

        public init(workspaceId: String, tenantId: String, principalId: String, jobId: String,
                    attemptId: String, importRequestId: String, sourceArchiveSha256: String) {
            self.workspaceId = workspaceId
            self.tenantId = tenantId
            self.principalId = principalId
            self.jobId = jobId
            self.attemptId = attemptId
            self.importRequestId = importRequestId
            self.sourceArchiveSha256 = sourceArchiveSha256
        }
    }

    public struct Verified: Equatable, Sendable {
        public let scope: Scope
        public let exhibitionId: String
        public let immutableRevisionId: String
        public let studioETag: String
        public let resultBytes: Int
        public let resultSha256: String
        public let proofSha256: String
    }

    public static func verify(proofBytes: Data, resultBytes: Data, scope: Scope, offer: NativeOEXSaveOffer,
                              held: NativeStudioResponse, reopened: NativeStudioResponse) throws -> Verified {
        let invalid = AuthoringFailure("OEX_TERMINAL_PROOF_INVALID")
        guard (1...proofByteLimit).contains(proofBytes.count),
              (1...resultByteLimit).contains(resultBytes.count),
              let immutableRevisionId = held.draft.candidate["revisionId"].string,
              [scope.workspaceId, scope.tenantId, scope.principalId, scope.jobId, scope.attemptId,
               scope.importRequestId, offer.exhibitionId, immutableRevisionId].allSatisfy(canonicalUUID),
              nativeImportDigest(scope.sourceArchiveSha256),
              scope.attemptId == offer.attemptId,
              held.revision == 1, held.etag == offer.etag, held.draft.id == offer.exhibitionId,
              held.draft.candidate["id"].string == offer.exhibitionId,
              let hash = studioDigest(held.etag, revision: 1),
              reopened.revision == 2, studioDigest(reopened.etag, revision: 2) == hash,
              reopened.draft == held.draft else { throw invalid }

        let resultSha256 = digest(resultBytes)
        // Migration 034 fixes the producer's exact proof field order and JSON
        // encoding. All inserted fields have been constrained to safe ASCII.
        let proof =
            #"{"version":1,"kind":"workspace-oex-import-terminal","workspaceId":"\#(scope.workspaceId)","tenantId":"\#(scope.tenantId)""# +
            #","principalId":"\#(scope.principalId)","jobId":"\#(scope.jobId)","attemptId":"\#(scope.attemptId)""# +
            #","importRequestId":"\#(scope.importRequestId)","sourceArchiveSha256":"\#(scope.sourceArchiveSha256)""# +
            #","archiveFormat":"1.0.0-draft.12","exhibitionId":"\#(offer.exhibitionId)","immutableRevisionId":"\#(immutableRevisionId)""# +
            #","studioETag":"\"studio-r1-\#(hash)\"","resultBytes":\#(resultBytes.count),"resultSha256":"\#(resultSha256)"}"#
        guard proofBytes == Data(proof.utf8) else { throw invalid }

        // The result digest covers the raw producer bytes. Decode only after
        // the byte count and SHA match, without normalizing the result.
        let decoder = JSONDecoder()
        let resultKeys: Set<String> = ["assetAliases", "draft", "etag", "exhibitionId", "idMap", "revision"]
        guard !resultBytes.starts(with: [0xef, 0xbb, 0xbf]),
              String(data: resultBytes, encoding: .utf8) != nil,
              let result = try? decoder.decode(DocumentValue.self, from: resultBytes),
              case .object(let fields) = result, Set(fields.keys) == resultKeys,
              hasUniqueObjectMembers(resultBytes, decoder: decoder),
              result["revision"].integer == 1, result["etag"].string == offer.etag,
              result["exhibitionId"].string == offer.exhibitionId,
              let draft = try? NativeExhibitionDraft(document: result["draft"]), draft == held.draft,
              case .object(let idMap) = result["idMap"],
              let aliases = result["assetAliases"].items, aliases.allSatisfy(validAlias),
              idMap.allSatisfy({ canonicalUUID($0.key) && $0.value.string.map(canonicalUUID) == true }) else {
            throw invalid
        }
        let destinations = Set(idMap.values.compactMap(\.string))
        guard destinations.count == idMap.count,
              validMaterials(in: draft, destinations: destinations) else { throw invalid }

        return Verified(scope: scope, exhibitionId: offer.exhibitionId, immutableRevisionId: immutableRevisionId,
                        studioETag: offer.etag, resultBytes: resultBytes.count, resultSha256: resultSha256,
                        proofSha256: digest(proofBytes))
    }

    private static func studioDigest(_ value: String, revision: Int) -> String? {
        let prefix = "\"studio-r\(revision)-"
        guard value.hasPrefix(prefix), value.hasSuffix("\"") else { return nil }
        let hash = String(value.dropFirst(prefix.count).dropLast())
        return nativeImportDigest(hash) ? hash : nil
    }

    private static func digest(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    private static func canonicalUUID(_ value: String) -> Bool {
        validUUID(value) && value == value.lowercased()
    }

    private static func validUUID(_ value: String) -> Bool {
        guard value.utf8.count == 36, UUID(uuidString: value) != nil else { return false }
        let bytes = Array(value.utf8)
        return (49...56).contains(bytes[14]) && "89abAB".utf8.contains(bytes[19])
    }

    /// Foundation has already parsed the JSON. Walk its wire keys as well,
    /// since decoding into a dictionary can silently discard duplicates.
    private static func hasUniqueObjectMembers(_ data: Data, decoder: JSONDecoder) -> Bool {
        var keysByDepth: [Set<String>?] = []
        var stringStart: Data.Index?
        var escaped = false
        for index in data.indices {
            let byte = data[index]
            if let start = stringStart {
                if escaped {
                    escaped = false
                } else if byte == 92 {
                    escaped = true
                } else if byte == 34 {
                    stringStart = nil
                    var next = index + 1
                    while next < data.endIndex, [9, 10, 13, 32].contains(data[next]) { next += 1 }
                    guard next < data.endIndex, data[next] == 58 else { continue }

                    let contents = data[(start + 1)..<index]
                    let key: String
                    if contents.contains(92) {
                        guard let decoded = try? decoder.decode(String.self, from: data[start...index]) else { return false }
                        key = decoded
                    } else {
                        key = String(decoding: contents, as: UTF8.self)
                    }
                    guard let frame = keysByDepth.indices.last,
                          keysByDepth[frame]?.insert(key).inserted == true else { return false }
                }
                continue
            }
            switch byte {
            case 34: stringStart = index
            case 123: keysByDepth.append(Set<String>())
            case 91: keysByDepth.append(nil)
            case 125, 93:
                guard !keysByDepth.isEmpty else { return false }
                keysByDepth.removeLast()
            default: break
            }
        }
        return keysByDepth.isEmpty && stringStart == nil
    }

    private static func validAlias(_ value: DocumentValue) -> Bool {
        let ids: Set<String> = ["destinationArtworkId", "destinationArtworkRevisionId", "destinationAssetId",
                                "sourceArtworkRevisionId", "sourceAssetId"]
        let paths: Set<String> = ["sourceArtifactPath", "destinationArtifactPath"]
        guard case .object(let fields) = value, Set(fields.keys) == ids.union(paths) else { return false }
        return ids.allSatisfy { fields[$0]?.string.map(validUUID) == true }
            && paths.allSatisfy { fields[$0]?.string.map { (1...1024).contains($0.utf8.count) } == true }
    }

    private struct MaterialVersion: Hashable {
        let id: String
        let version: Int
    }

    private static func validMaterials(in draft: NativeExhibitionDraft, destinations: Set<String>) -> Bool {
        let value = draft.candidate["extensions"][WorkspaceSurfaceMaterialBinding.namespace]
        guard case .object(let fields) = value, Set(fields.keys) == ["version", "materials", "bindings"],
              value["version"].integer == 1,
              let materials = value["materials"].items, (1...64).contains(materials.count),
              let bindings = value["bindings"].items, (1...128).contains(bindings.count),
              let surfaces = draft.candidate["surfaces"].items else { return false }

        let encoder = JSONEncoder(), decoder = JSONDecoder()
        var known: [MaterialVersion: WorkspaceMaterialKind] = [:]
        for descriptor in materials {
            guard let data = try? encoder.encode(descriptor),
                  let material = try? decoder.decode(WorkspaceMaterial.self, from: data),
                  (try? material.check()) != nil, Set(material.maps.keys) == ["color"],
                  material.rights.permissions.display, destinations.contains(material.id),
                  known.updateValue(material.kind, forKey: MaterialVersion(id: material.id, version: material.version)) == nil else { return false }
        }

        var surfaceTypes: [String: String] = [:]
        for surface in surfaces {
            guard let id = surface["id"].string, let type = surface["type"].string,
                  surfaceTypes.updateValue(type, forKey: id.lowercased()) == nil else { return false }
        }

        let required: Set<String> = ["surfaceId", "materialId", "materialVersion", "repeatMeters", "rotation"]
        let sampling = ["normalScale": 0.0...2.0, "aoStrength": 0.0...1.0]
        var used = Set<MaterialVersion>(), bound = Set<String>()
        for row in bindings {
            guard case .object(let fields) = row, required.isSubset(of: fields.keys),
                  Set(fields.keys).isSubset(of: required.union(sampling.keys)),
                  sampling.allSatisfy({ key, range in fields[key].map { $0.double.map(range.contains) == true } ?? true }),
                  let surfaceId = row["surfaceId"].string, let materialId = row["materialId"].string,
                  let version = row["materialVersion"].integer.flatMap(Int.init(exactly:)),
                  let repeatMeters = row["repeatMeters"].items, let rotation = row["rotation"].double,
                  repeatMeters.allSatisfy({ $0.double != nil }),
                  let kind = known[MaterialVersion(id: materialId, version: version)],
                  let binding = try? WorkspaceSurfaceMaterialBinding(surfaceKind: kind, surfaceId: surfaceId,
                        materialId: materialId, materialVersion: version,
                        repeatMeters: repeatMeters.compactMap(\.double), rotation: rotation),
                  surfaceTypes[binding.surfaceId] == kind.rawValue, destinations.contains(binding.surfaceId),
                  bound.insert(binding.surfaceId).inserted else { return false }
            used.insert(MaterialVersion(id: binding.materialId, version: binding.materialVersion))
        }
        return used == Set(known.keys)
    }
}
