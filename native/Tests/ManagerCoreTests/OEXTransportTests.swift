// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class OEXTransportTests: XCTestCase {
    func testLoopbackBinaryTransportRejectsRedirectWrongMimeAndDeclaredOrStreamedOversize() async throws {
        guard let node = ProcessInfo.processInfo.environment["EXHIBITOS_OEX_TEST_NODE"] else { throw XCTSkip("Explicit owned Node fixture runtime only") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("exhibitos-oex-wire-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let script = directory.appendingPathComponent("server.mjs")
        try #"""
        import http from 'node:http';
        const [tenant,user]=process.argv.slice(2),csrf='owned-oex-wire-csrf',token='a'.repeat(43),limit=64*1024*1024,mime='application/vnd.exhibitos.oex+zip';
        const server=http.createServer((req,res)=>{
          req.resume();
          if(req.url==='/api/v1/auth/session'){
            res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({tenantId:tenant,userId:user,role:'admin',csrfToken:csrf,expiresAt:new Date(Date.now()+3600000).toISOString()}));return;
          }
          const base='http://127.0.0.1:'+server.address().port;
          if(req.method!=='POST'||req.headers.origin!==base||req.headers['x-csrf-token']!==csrf||!req.headers.cookie?.includes('exhibitos_local_session='+token)||req.headers['if-match']!=='"studio-r1-'+ '0'.repeat(64)+'"'){
            res.writeHead(400,{'content-type':'application/json'});res.end('{"code":"FIXTURE_AUTH_INVALID"}');return;
          }
          if(req.url.includes('000000000001/oex/export')){res.writeHead(200,{'content-type':mime,'content-length':limit+1});res.flushHeaders();res.write(Buffer.from([80]));return;}
          if(req.url.includes('000000000002/oex/export')){res.writeHead(200,{'content-type':'text/html'});res.end('<html>not an archive</html>');return;}
          if(req.url.includes('000000000003/oex/export')){res.writeHead(302,{location:base+'/redirect-target'});res.end();return;}
          if(req.url.includes('000000000004/oex/export')){
            res.writeHead(200,{'content-type':mime});let sent=0;const chunk=Buffer.alloc(65536);
            const send=()=>{if(res.destroyed)return;while(sent<limit+1){const n=Math.min(chunk.length,limit+1-sent);sent+=n;if(!res.write(chunk.subarray(0,n))){res.once('drain',send);return;}}res.end();};send();return;
          }
          res.writeHead(200,{'content-type':mime});res.end(Buffer.concat([Buffer.from([80,75,3,4]),Buffer.alloc(22)]));
        });
        server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
        """#.write(to: script, atomically: true, encoding: .utf8)
        let process = Process(), pipe = Pipe(), errors = Pipe()
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased()
        process.executableURL = URL(fileURLWithPath: node); process.arguments = [script.path,tenant,user]; process.standardOutput = pipe; process.standardError = errors
        try process.run()
        defer { if process.isRunning { process.terminate() }; process.waitUntilExit(); try? FileManager.default.removeItem(at: directory) }
        var portData = Data()
        while portData.count < 32 {
            let byte = try XCTUnwrap(try pipe.fileHandleForReading.read(upToCount: 1))
            guard !byte.isEmpty else { break }
            portData.append(byte)
            if byte.first == 10 { break }
        }
        let port = try XCTUnwrap(UInt16(String(decoding: portData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        let api = try AuthoringAPI(port: port, tenantId: tenant)
        _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:\(port)", token: String(repeating: "a", count: 43)))
        let etag = "\"studio-r1-" + String(repeating: "0", count: 64) + "\""
        for (suffix,code) in [("1","OEX_LIMIT"),("2","OEX_RESPONSE_INVALID"),("3","REQUEST_FAILED"),("4","OEX_LIMIT")] {
            let id = "00000000-0000-4000-8000-00000000000" + suffix
            do { _ = try await api.exportOEX(exhibitionId: id, etag: etag); XCTFail("Invalid binary response must fail: \(suffix)") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.code, code); if suffix == "3" { XCTAssertEqual(error.status, 302) } }
        }
    }
}
