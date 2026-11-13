// SPDX-License-Identifier: Apache-2.0
import XCTest
import Foundation
import ManagerCore
@testable import ManagerApp

@MainActor
final class FailedPNGReplacementTests: XCTestCase {
    private let manifest = "979489bc659e068e5cd4a24df36f500ca02e65c96c041cd2e9d88af127cb8c4c"

    private func job(_ fields: [String: Any] = [:]) throws -> NativeImportJob {
        let base: [String: Any] = ["id": UUID().uuidString.lowercased(), "state": "failed", "progress": 25,
            "attempts": 1, "errorCode": "FORMAT_REJECTED", "workerDrainVersion": 1, "workerDrained": true]
        return try JSONDecoder().decode(NativeImportJob.self, from: JSONSerialization.data(withJSONObject: base.merging(fields) { _, new in new }))
    }

    func testFailedPlainPNGRequiresExactRuntimeAndSupportedDrainWithoutChangingTerminalSemantics() throws {
        let intent = NativeImportIntent(artworkId: UUID().uuidString.lowercased(), mime: "image/png",
            sha256: String(repeating: "a", count: 64), bytes: 128, scaleMeters: 1, rights: ArtworkMetadata.Rights())
        let failed = try job()
        XCTAssertFalse(failed.terminal)
        XCTAssertTrue(AuthoringModel.permitsFailedPlainPNGReplacement(failed, intent: intent, manifest: manifest))
        for runtime in [nil, String(repeating: "0", count: 64),
                        "76200d7f27776a9e0060c1f071cc66deeffe5de271d92a02ef4048c56493bdbd"] as [String?] {
            XCTAssertFalse(AuthoringModel.permitsFailedPlainPNGReplacement(failed, intent: intent, manifest: runtime))
        }
        let deniedFields: [[String: Any]] = [["workerDrainVersion": 0], ["workerDrainVersion": 2],
                ["workerDrainVersion": NSNull()], ["workerDrained": false], ["workerDrained": NSNull()],
                ["state": "uploading"], ["state": "queued"], ["state": "processing"],
                ["state": "approved"], ["state": "cancelled"]]
        for fields in deniedFields {
            XCTAssertFalse(AuthoringModel.permitsFailedPlainPNGReplacement(try job(fields), intent: intent, manifest: manifest))
        }
        for mime in ["image/jpeg", "model/gltf-binary"] {
            let other = NativeImportIntent(artworkId: intent.artworkId, mime: mime, sha256: intent.sha256,
                bytes: intent.bytes, scaleMeters: 1, rights: intent.rights)
            XCTAssertFalse(AuthoringModel.permitsFailedPlainPNGReplacement(failed, intent: other, manifest: manifest))
        }
        let prepared = NativeImportIntent(artworkId: intent.artworkId, mime: "image/png", sha256: intent.sha256,
            bytes: intent.bytes, scaleMeters: 1, rights: intent.rights,
            productImagePreparation: NativeProductImagePreparationIntent(artworkRevision: 1, fresh: true))
        XCTAssertFalse(AuthoringModel.permitsFailedPlainPNGReplacement(failed, intent: prepared, manifest: manifest))
        XCTAssertTrue(try job(["state": "approved"]).terminal)
        XCTAssertTrue(try job(["state": "cancelled"]).terminal)
    }

    func testExistingClearActionRefreshesDrainThenClearsOnlyLocalIntent() async throws {
        let fixture = try await Fixture(manifest: manifest)
        defer { fixture.close() }
        XCTAssertTrue(fixture.model.canPrepareDifferentImport)
        await fixture.model.clearImport()
        XCTAssertNil(fixture.model.failure)
        XCTAssertEqual(fixture.store.writes, 1)
        XCTAssertEqual(fixture.store.state.revision, 3)
        XCTAssertNil(fixture.store.state.intent)
        XCTAssertNil(fixture.store.state.jobId)
        XCTAssertNil(fixture.model.importJob)
        XCTAssertEqual(fixture.model.selected?.id, fixture.artwork)
        let requests = try fixture.requests()
        XCTAssertGreaterThanOrEqual(requests.filter { $0.hasPrefix("GET ") && $0.contains("/imports/") }.count, 2)
        XCTAssertFalse(requests.contains { !$0.hasPrefix("GET ") })
        XCTAssertEqual(try fixture.serverJob()["state"] as? String, "failed")
        XCTAssertEqual(try fixture.serverJob()["errorCode"] as? String, "FORMAT_REJECTED")
    }

    func testLostDrainOnFreshReadPreservesIntentAndDoesNotCancelOrRetry() async throws {
        let fixture = try await Fixture(manifest: manifest)
        defer { fixture.close() }
        XCTAssertTrue(fixture.model.canPrepareDifferentImport)
        try fixture.setDrained(false)
        await fixture.model.clearImport()
        XCTAssertEqual(fixture.model.failure?.code, "IMPORT_DRAIN_PENDING")
        XCTAssertEqual(fixture.store.writes, 0)
        XCTAssertEqual(fixture.store.state.revision, 2)
        XCTAssertEqual(fixture.store.state.jobId, fixture.jobID)
        XCTAssertNotNil(fixture.store.state.intent)
        XCTAssertFalse(fixture.model.canPrepareDifferentImport)
        XCTAssertFalse(try fixture.requests().contains { !$0.hasPrefix("GET ") })
        XCTAssertEqual(try fixture.serverJob()["state"] as? String, "failed")
    }

    func testDifferentSelectedArtworkCannotClearRetainedFailedIntent() async throws {
        let fixture = try await Fixture(manifest: manifest)
        defer { fixture.close() }
        await fixture.model.open(UUID().uuidString.lowercased())
        XCTAssertFalse(fixture.model.canPrepareDifferentImport)
        await fixture.model.clearImport()
        XCTAssertEqual(fixture.model.failure?.code, "IMPORT_ACTIVE")
        XCTAssertEqual(fixture.store.writes, 0)
        XCTAssertEqual(fixture.store.state.jobId, fixture.jobID)
    }
}

@MainActor
private final class Store {
    var state: NativeUploadState
    var writes = 0
    init(_ state: NativeUploadState) { self.state = state }
    func save(_ proposed: NativeUploadState) throws -> NativeUploadState {
        guard proposed.revision == state.revision else { throw BridgeFailure("NATIVE_UPLOAD_STATE_CHANGED") }
        var next = proposed; next.revision += 1; state = next; writes += 1
        return next
    }
}

@MainActor
private final class Fixture {
    let root: URL
    let model = AuthoringModel()
    let process = Process()
    let artwork = UUID().uuidString.lowercased()
    let jobID = UUID().uuidString.lowercased()
    let store: Store

    init(manifest: String) async throws {
        guard let node = ProcessInfo.processInfo.environment["EXHIBITOS_OEX_TEST_NODE"] else {
            throw XCTSkip("Requires the existing owned Node fixture runtime")
        }
        root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("failed-png-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        let tenant = UUID().uuidString.lowercased(), principal = UUID().uuidString.lowercased(), artist = UUID().uuidString.lowercased()
        let intent = NativeImportIntent(artworkId: artwork, mime: "image/png", sha256: String(repeating: "a", count: 64),
            bytes: 128, scaleMeters: 1, rights: ArtworkMetadata.Rights())
        let intentJSON = try JSONSerialization.jsonObject(with: JSONEncoder().encode(intent))
        let state = try JSONDecoder().decode(NativeUploadState.self, from: JSONSerialization.data(withJSONObject: [
            "format": 1, "revision": 2, "workspaceId": UUID().uuidString.lowercased(), "tenantId": tenant,
            "principalId": principal, "intent": intentJSON, "jobId": jobID, "fileName": "synthetic.png"
        ]))
        store = Store(state)
        let config: [String: Any] = ["tenant": tenant, "principal": principal, "artist": artist, "job": jobID]
        try JSONSerialization.data(withJSONObject: config).write(to: root.appendingPathComponent("config.json"))
        try "1".write(to: root.appendingPathComponent("drained"), atomically: true, encoding: .utf8)
        let script = root.appendingPathComponent("server.mjs")
        try #"""
        import http from 'node:http';
        import {readFileSync,appendFileSync,writeFileSync} from 'node:fs';
        const root=process.argv[2], c=JSON.parse(readFileSync(root+'/config.json'));
        const server=http.createServer((req,res)=>{
          appendFileSync(root+'/requests',req.method+' '+req.url+'\n');
          const path=new URL(req.url,'http://127.0.0.1').pathname;
          const send=body=>{res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(body));};
          if(req.headers.cookie!=='exhibitos_local_session='+'a'.repeat(43)){res.writeHead(401);return res.end('{}');}
          if(path==='/api/v1/auth/session')return send({tenantId:c.tenant,userId:c.principal,role:'admin',csrfToken:'synthetic-csrf',expiresAt:'2099-01-01T00:00:00Z'});
          if(path.endsWith('/imports/'+c.job)){
            const job={id:c.job,state:'failed',progress:25,attempts:1,errorCode:'FORMAT_REJECTED',workerDrainVersion:1,workerDrained:readFileSync(root+'/drained','utf8')==='1'};
            writeFileSync(root+'/job.json',JSON.stringify(job));return send(job);
          }
          if(path.endsWith('/artists'))return send({items:[{id:c.artist,name:'Synthetic',bio:'',revision:1,archived:false}]});
          if(path.endsWith('/artists/'+c.artist))return send({id:c.artist,name:'Synthetic',bio:'',revision:1,archived:false});
          if(path.endsWith('/artworks'))return send({items:[]});
          if(path.includes('/artworks/'))return send({id:path.split('/').at(-1),artistId:c.artist,revision:1,metadata:null,assets:[],archived:false});
          res.writeHead(404);res.end('{}');
        });
        server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
        """#.write(to: script, atomically: true, encoding: .utf8)
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: node); process.arguments = [script.path, root.path]
        process.standardOutput = pipe
        try process.run()
        var bytes = Data()
        while bytes.count < 32 {
            let byte = try XCTUnwrap(try pipe.fileHandleForReading.read(upToCount: 1)); guard !byte.isEmpty else { break }
            bytes.append(byte); if byte.first == 10 { break }
        }
        let port = try XCTUnwrap(UInt16(String(decoding: bytes, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        let runtime = try JSONDecoder().decode(NativeConfiguration.self, from: JSONSerialization.data(withJSONObject: [
            "kind": "personal", "tenantId": tenant, "manifestSha256": manifest, "ports": ["web": port, "api": port, "database": port]
        ]))
        XCTAssertTrue(model.configure(runtime, ready: true, runtimeJobID: UUID().uuidString.lowercased()))
        let memory = store
        model.bindImportStore(read: { memory.state }, save: { try memory.save($0) })
        let credentials = try JSONDecoder().decode(LocalAuthoringSession.self, from: JSONSerialization.data(withJSONObject: [
            "tenantId": tenant, "principalId": principal, "origin": "http://127.0.0.1:\(port)", "token": String(repeating: "a", count: 43)
        ]))
        await model.connect(credentials)
        await model.open(artwork)
        XCTAssertNil(model.failure)
    }

    func setDrained(_ value: Bool) throws { try (value ? "1" : "0").write(to: root.appendingPathComponent("drained"), atomically: true, encoding: .utf8) }
    func requests() throws -> [String] { try String(contentsOf: root.appendingPathComponent("requests"), encoding: .utf8).split(separator: "\n").map(String.init) }
    func serverJob() throws -> [String: Any] { try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("job.json"))) as? [String: Any]) }
    func close() {
        _ = model.configure(nil, ready: false, runtimeJobID: nil)
        if process.isRunning { process.terminate() }; process.waitUntilExit()
        try? FileManager.default.removeItem(at: root)
    }
}
