// SPDX-License-Identifier: Apache-2.0
import XCTest
import ManagerCore
@testable import ManagerApp

@MainActor
final class AuthoringRuntimeSessionTests: XCTestCase {
    func testNewRuntimeReconnectsCMSWithoutDiscardingForm() async throws {
        guard let node = ProcessInfo.processInfo.environment["EXHIBITOS_OEX_TEST_NODE"] else {
            throw XCTSkip("Requires owned Node fixture runtime")
        }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("exhibitos-cms-session-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let script = directory.appendingPathComponent("server.mjs")
        let active = directory.appendingPathComponent("active-session")
        let delayed = directory.appendingPathComponent("delayed-session")
        try "a".write(to: active, atomically: true, encoding: .utf8)
        try #"""
        import http from 'node:http';
        import {readFileSync,writeFileSync} from 'node:fs';
        const [tenant,user,artist,active,delayed]=process.argv.slice(2);
        const prefix='/api/v1/tenants/'+tenant+'/cms';
        let oldConnections=0;
        const server=http.createServer((req,res)=>{
          const key=readFileSync(active,'utf8'), csrf='csrf-'+key;
          const send=(status,body)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(body));};
          if(req.headers.cookie!=='exhibitos_local_session='+key.repeat(43))return send(401,{code:'AUTH_REQUIRED'});
          if(req.url==='/api/v1/auth/session'){
            const answer=()=>send(200,{tenantId:tenant,userId:user,role:'admin',csrfToken:csrf,expiresAt:'2099-01-01T00:00:00.000Z'});
            if(key==='a'&&++oldConnections===2){writeFileSync(delayed,'');setTimeout(answer,300);return;}
            return answer();
          }
          if(req.method==='GET'&&[prefix+'/artists?limit=50&archived=false',prefix+'/artworks?limit=20&archived=false&q='].includes(req.url))return send(200,{items:[]});
          if(req.method==='POST'&&req.url===prefix+'/artists'){
            if(req.headers['x-csrf-token']!==csrf)return send(403,{code:'CSRF_REJECTED'});
            let body='';req.on('data',chunk=>body+=chunk);
            req.on('end',()=>{const {name,bio}=JSON.parse(body);send(201,{id:artist,name,bio,revision:1,archived:false});});
            return;
          }
          send(404,{code:'NOT_FOUND'});
        });
        server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
        """#.write(to: script, atomically: true, encoding: .utf8)
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased()
        let artist = UUID().uuidString.lowercased()
        let process = Process(), pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: node)
        process.arguments = [script.path, tenant, user, artist, active.path, delayed.path]
        process.standardOutput = pipe
        try process.run()
        defer { if process.isRunning { process.terminate() }; process.waitUntilExit() }
        var portData = Data()
        while portData.count < 32 {
            let byte = try XCTUnwrap(try pipe.fileHandleForReading.read(upToCount: 1))
            guard !byte.isEmpty else { break }
            portData.append(byte)
            if byte.first == 10 { break }
        }
        let port = try XCTUnwrap(UInt16(String(decoding: portData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        let config = try JSONDecoder().decode(NativeConfiguration.self, from: JSONSerialization.data(withJSONObject: [
            "kind": "personal", "tenantId": tenant, "manifestSha256": String(repeating: "0", count: 64),
            "ports": ["web": port, "api": port, "database": port]
        ]))
        func credentials(_ key: String) throws -> LocalAuthoringSession {
            try JSONDecoder().decode(LocalAuthoringSession.self, from: JSONSerialization.data(withJSONObject: [
                "tenantId": tenant, "principalId": user, "origin": "http://127.0.0.1:\(port)", "token": String(repeating: key, count: 43)
            ]))
        }

        let model = AuthoringModel()
        let oldJob = UUID().uuidString, newJob = UUID().uuidString
        XCTAssertTrue(model.configure(config, ready: true, runtimeJobID: oldJob))
        await model.connect(try credentials("a"))
        XCTAssertEqual(model.session?.userId, user)
        XCTAssertNil(model.failure)
        model.newArtwork()
        model.draft.title = "Preserved test input"
        model.artistName = "Fixture artist"
        XCTAssertTrue(model.configure(config, ready: true, runtimeJobID: oldJob))
        XCTAssertNotNil(model.session)

        let oldCredentials = try credentials("a")
        let oldConnect = Task { await model.connect(oldCredentials) }
        let deadline = ContinuousClock.now.advanced(by: .seconds(5))
        while !FileManager.default.fileExists(atPath: delayed.path), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: delayed.path))
        XCTAssertTrue(model.busy)
        try "b".write(to: active, atomically: true, encoding: .utf8)
        XCTAssertTrue(model.configure(config, ready: true, runtimeJobID: newJob))
        XCTAssertNil(model.session)
        XCTAssertEqual(model.draft.title, "Preserved test input")
        XCTAssertEqual(model.artistName, "Fixture artist")
        let canConnect = await model.waitForConnectionTurn(runtimeJobID: newJob)
        XCTAssertTrue(canConnect)
        await oldConnect.value
        XCTAssertNil(model.session)
        await model.connect(try credentials("b"))
        XCTAssertEqual(model.session?.userId, user)
        XCTAssertNil(model.failure)
        await model.createArtist()
        XCTAssertNil(model.failure)
        XCTAssertEqual(model.artistId, artist)
        XCTAssertEqual(model.draft.title, "Preserved test input")
    }
}
