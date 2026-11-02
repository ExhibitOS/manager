// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class BoundedOEXExportTests: XCTestCase {
    func testSelectedCompletionIntegrityRetryAndExclusiveDestination() async throws {
        guard let node = ProcessInfo.processInfo.environment["EXHIBITOS_OEX_TEST_NODE"] else {
            throw XCTSkip("Requires the owned local Node fixture")
        }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("bounded-export-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        let script = directory.appendingPathComponent("server.mjs")
        try #"""
        import http from 'node:http'; import {createHash} from 'node:crypto';
        const [tenant,user]=process.argv.slice(2),csrf='synthetic-csrf',token='a'.repeat(43),etag='"studio-r1-'+ '0'.repeat(64)+'"';
        const data=Buffer.concat([Buffer.from([80,75,3,4]),Buffer.alloc(22)]),sha256=createHash('sha256').update(data).digest('hex'),operations=new Map();
        const server=http.createServer(async(req,res)=>{
          const json=(status,value)=>{res.writeHead(status,{'content-type':'application/json','cache-control':'no-store'});res.end(JSON.stringify(value));};
          if(req.url==='/api/v1/auth/session'){req.resume();json(200,{tenantId:tenant,userId:user,role:'admin',csrfToken:csrf,expiresAt:'2099-01-01T00:00:00Z',nativeOexExportProfile:'bounded-file-v1'});return;}
          const base='http://127.0.0.1:'+server.address().port;
          if(req.headers.origin!==base||!req.headers.cookie?.includes('exhibitos_local_session='+token)||req.headers['if-match']!==etag){req.resume();json(403,{code:'FORBIDDEN'});return;}
          const match=req.url.match(new RegExp(`^/api/v1/tenants/${tenant}/studio/exhibitions/00000000-0000-4000-8000-(\\d{12})/native-oex/(export|exports/([a-f0-9-]+))$`));
          if(!match){req.resume();json(404,{code:'NOT_FOUND'});return;}
          const n=Number(match[1]);
          if(req.method==='GET'){
            req.resume();const operationId=match[3],op=operations.get(operationId);if(!op){json(404,{code:'NOT_FOUND'});return;}
            if(op.state==='failed'){json(200,{operationId,state:'failed'});return;}
            op.polls++;if(n===8||n===11||op.polls===1){json(200,{operationId,state:'pending'});return;}
            if(n===5||n===6){json(200,{operationId,state:n===5?'failed':'uncertain'});return;}
            if(n===7){json(404,{code:'NOT_FOUND'});return;}
            json(200,{operationId,state:'complete',etag,bytes:data.length,sha256:n===4?'0'.repeat(64):sha256});return;
          }
          if(req.method!=='POST'||req.headers['x-csrf-token']!==csrf){req.resume();json(403,{code:'FORBIDDEN'});return;}
          let body='';for await(const part of req)body+=part;const {requestId}=JSON.parse(body);
          if(!/^[a-f0-9-]{36}$/.test(requestId)||operations.has(requestId)){json(409,{code:'CONFLICT'});return;}
          if(n===9){res.setHeader('x-exhibitos-oex-operation',requestId);res.setHeader('x-exhibitos-oex-admission','attempt-not-admitted');json(403,{code:'FORBIDDEN'});return;}
          const op={polls:0};operations.set(requestId,op);
          if(n===10){op.state='failed';req.socket.destroy();return;}
          if(n===11){
            const stream=Buffer.alloc(1024*1024);data.copy(stream);res.writeHead(200,{'content-type':'application/vnd.exhibitos.oex+zip','content-length':String(stream.length),'cache-control':'no-store','x-exhibitos-oex-profile':'bounded-file-v1','x-exhibitos-oex-sha256':createHash('sha256').update(stream).digest('hex'),'x-exhibitos-oex-operation':requestId});
            let offset=0;const timer=setInterval(()=>{const end=Math.min(offset+65536,stream.length);res.write(stream.subarray(offset,end));offset=end;if(offset===stream.length){clearInterval(timer);res.end();}},50);res.on('close',()=>{clearInterval(timer);op.state='failed';});return;
          }
          res.writeHead(200,{'content-type':'application/vnd.exhibitos.oex+zip','content-length':n===13?'536870913':String(data.length),'cache-control':'no-store','x-exhibitos-oex-profile':'bounded-file-v1','x-exhibitos-oex-sha256':n===3?'0'.repeat(64):sha256,'x-exhibitos-oex-operation':n===2?'invalid':(n===12?'11111111-1111-4111-8111-111111111111':requestId)});res.end(data);
        });server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
        """#.write(to: script, atomically: true, encoding: .utf8)
        let process = Process(), pipe = Pipe()
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased()
        process.executableURL = URL(fileURLWithPath: node); process.arguments = [script.path, tenant, user]
        process.standardOutput = pipe; process.standardError = FileHandle.standardError
        try process.run()
        defer {
            if process.isRunning { process.terminate() }
            process.waitUntilExit()
            try? FileManager.default.removeItem(at: directory)
        }
        var portData = Data()
        while portData.count < 32 {
            let byte = try XCTUnwrap(try pipe.fileHandleForReading.read(upToCount: 1))
            guard !byte.isEmpty else { break }
            portData.append(byte); if byte.first == 10 { break }
        }
        let port = try XCTUnwrap(UInt16(String(decoding: portData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        func client() async throws -> AuthoringAPI {
            let api = try AuthoringAPI(port: port, tenantId: tenant)
            _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:\(port)", token: String(repeating: "a", count: 43)))
            return api
        }
        let etag = "\"studio-r1-" + String(repeating: "0", count: 64) + "\""
        func id(_ suffix: Int) -> String { "00000000-0000-4000-8000-" + String(format: "%012d", suffix) }
        let api = try await client(), output = directory.appendingPathComponent("verified.oex")
        do { _ = try await api.exportBoundedOEX(exhibitionId: id(1), etag: "invalid"); XCTFail("A local invalid ETag must not create an intent") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "ETAG_INVALID") }
        do { _ = try await api.exportBoundedOEX(exhibitionId: id(9), etag: etag); XCTFail("Pre-admission denial must reject output") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.status, 403) }
        let archive = try await api.exportBoundedOEX(exhibitionId: id(1), etag: etag)
        XCTAssertEqual(archive.count, 26)
        try await archive.write(to: output)
        let original = try Data(contentsOf: output)
        XCTAssertEqual(original.count, 26)
        do { try await archive.write(to: output); XCTFail("An existing output must remain unchanged") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "EXPORT_WRITE_FAILED") }
        XCTAssertEqual(try Data(contentsOf: output), original)
        for suffix in 2...7 {
            let invalid = try await client()
            do { _ = try await invalid.exportBoundedOEX(exhibitionId: id(suffix), etag: etag); XCTFail("Invalid response or terminal \(suffix) accepted") }
            catch let failure as AuthoringFailure {
                let expected = suffix <= 4 ? "OEX_RESPONSE_INVALID" : (suffix <= 6 ? "NATIVE_OEX_EXPORT_UNCERTAIN" : "NOT_FOUND")
                XCTAssertEqual(failure.code, expected)
            }
        }
        for (suffix, code) in [(12, "OEX_RESPONSE_INVALID"), (13, "NATIVE_OEX_EXPORT_LIMIT")] {
            let invalid = try await client()
            do { _ = try await invalid.exportBoundedOEX(exhibitionId: id(suffix), etag: etag); XCTFail("Mismatched operation or declared oversize accepted") }
            catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, code) }
        }
        // A lost/rejected operation header retains the pre-POST request identity.
        // Explicit retry correlates that terminal before starting another request.
        let retrying = try await client()
        do { _ = try await retrying.exportBoundedOEX(exhibitionId: id(2), etag: etag); XCTFail("Invalid operation header accepted") }
        catch is AuthoringFailure { }
        let retried = try await retrying.exportBoundedOEX(exhibitionId: id(1), etag: etag)
        XCTAssertEqual(retried.sha256, archive.sha256)
        let lost = try await client()
        do { _ = try await lost.exportBoundedOEX(exhibitionId: id(10), etag: etag); XCTFail("A dropped socket must not expose output") }
        catch { XCTAssertTrue(error is URLError || error is CancellationError) }
        let afterLoss = try await lost.exportBoundedOEX(exhibitionId: id(1), etag: etag)
        XCTAssertEqual(afterLoss.sha256, archive.sha256)
        let streaming = try await client()
        let transfer = Task { try await streaming.exportBoundedOEX(exhibitionId: id(11), etag: etag) }
        try await Task.sleep(for: .milliseconds(150)); transfer.cancel()
        do { _ = try await transfer.value; XCTFail("Cancelling body transfer must not expose output") }
        catch { XCTAssertTrue(error is CancellationError || (error as? URLError)?.code == .cancelled) }
        let afterCancel = try await streaming.exportBoundedOEX(exhibitionId: id(1), etag: etag)
        XCTAssertEqual(afterCancel.sha256, archive.sha256)
        let cancelling = try await client()
        let task = Task { try await cancelling.exportBoundedOEX(exhibitionId: id(8), etag: etag) }
        try await Task.sleep(for: .milliseconds(200))
        do { try await cancelling.logout(); XCTFail("Authentication must stay fenced during terminal polling") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "AUTH_BUSY") }
        task.cancel()
        do { _ = try await task.value; XCTFail("Cancellation must not expose the staged file") }
        catch is CancellationError { }
    }
}
