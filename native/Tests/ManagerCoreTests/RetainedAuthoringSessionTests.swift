// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class RetainedAuthoringSessionTests: XCTestCase {
    func testRetainedCookieSessionIsRecheckedAndRevocationFailsClosed() async throws {
        guard let node = ProcessInfo.processInfo.environment["EXHIBITOS_OEX_TEST_NODE"] else { throw XCTSkip("Requires owned Node fixture runtime") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("exhibitos-session-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let script = directory.appendingPathComponent("server.mjs")
        try #"""
        import http from 'node:http';
        const [tenant,user]=process.argv.slice(2), token='a'.repeat(43); let count=0;
        const server=http.createServer((req,res)=>{
          if(req.url!=='/api/v1/auth/session'||!req.headers.cookie?.includes('exhibitos_local_session='+token)){res.writeHead(401);res.end('{"code":"AUTH_REQUIRED"}');return;}
          count++;
          if(count>=4){res.writeHead(401);res.end('{"code":"AUTH_REQUIRED"}');return;}
          res.writeHead(200,{'content-type':'application/json'});
          res.end(JSON.stringify({tenantId:tenant,userId:user,role:count===3?'viewer':'admin',csrfToken:'fixture-csrf',expiresAt:'2099-01-01T00:00:00.000Z'}));
        });
        server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
        """#.write(to: script, atomically: true, encoding: .utf8)
        let process = Process(), pipe = Pipe()
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased()
        process.executableURL = URL(fileURLWithPath: node); process.arguments = [script.path, tenant, user]; process.standardOutput = pipe
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
        let session = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:\(port)", token: String(repeating: "a", count: 43)))
        let verified = try await api.verifyRetainedSession(session)
        XCTAssertEqual(verified.userId, session.userId)
        do { _ = try await api.verifyRetainedSession(session); XCTFail("Changed role must be refused") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "ACCOUNT_BINDING_MISMATCH") }
        do { _ = try await api.verifyRetainedSession(session); XCTFail("Revoked session must be refused") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.status, 401) }
        do { _ = try await api.verifyRetainedSession(session); XCTFail("Cleared session must remain refused") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "ACCOUNT_BINDING_MISMATCH") }
    }
}
