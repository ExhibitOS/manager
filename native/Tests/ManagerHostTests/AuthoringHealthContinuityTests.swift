// SPDX-License-Identifier: Apache-2.0
import XCTest
import ManagerCore
@testable import ManagerApp

@MainActor
final class AuthoringHealthContinuityTests: XCTestCase {
    private let tenant = "10000000-0000-4000-8000-000000000001"
    private let user = "10000000-0000-4000-8000-000000000002"
    private func decode<T: Decodable>(_ type: T.Type, _ value: [String: Any]) throws -> T {
        try JSONDecoder().decode(type, from: JSONSerialization.data(withJSONObject: value))
    }
    private func context(_ token: String = "selection", installation: String = "installation") throws -> Context {
        try decode(Context.self, ["selectionToken": token, "activeId": installation, "installations": []])
    }
    private func status(port: UInt16 = 57841, ready: Bool = true, changes: [String: Any] = [:], runtimeChanges: [String: Any] = [:], configChanges: [String: Any] = [:]) throws -> NativeHostStatus {
        let ports: [String: Any] = ["web": port, "api": 57842, "database": 57843]
        var runtime: [String: Any] = ["jobId": "job", "state": ready ? "ready" : "unavailable", "ports": ports,
            "readiness": ["ready": ready], "nodePid": 101, "postgresPid": 102, "exits": [:], "logTruncated": false]
        runtime.merge(runtimeChanges) { _, new in new }
        var config: [String: Any] = ["kind": "personal", "tenantId": tenant, "manifestSha256": String(repeating: "1", count: 64), "ports": ports]
        config.merge(configChanges) { _, new in new }
        var value: [String: Any] = ["busy": false, "operation": "status", "accountRequired": false, "selectedOex": false, "configuration": config, "runtime": runtime]
        value.merge(changes) { _, new in new }
        return try decode(NativeHostStatus.self, value)
    }
    private func credentials(port: UInt16 = 57841) throws -> LocalAuthoringSession {
        try decode(LocalAuthoringSession.self, ["tenantId": tenant, "principalId": user, "origin": "http://127.0.0.1:\(port)", "token": String(repeating: "a", count: 43)])
    }
    private func session(expiry: String = "2099-01-01T00:00:00Z") throws -> AuthoringSession {
        try decode(AuthoringSession.self, ["tenantId": tenant, "userId": user, "role": "admin", "csrfToken": "fixture", "expiresAt": expiry])
    }
    private func binding(api: AuthoringAPI, session: AuthoringSession, port: UInt16 = 57841) throws -> ManagerModel.RetainedAuthoring {
        .init(selectedOex: false, selectionToken: "selection", installationId: "installation", runtimeJobId: "job", nodePid: 101, postgresPid: 102,
              manifest: String(repeating: "1", count: 64), port: port, credentials: try credentials(port: port), session: session, api: api)
    }
    func testHealthSamplesPreserveOnlyExactUnexpiredIdentity() throws {
        let api = try AuthoringAPI(port: 57841, tenantId: tenant)
        let held = try binding(api: api, session: session())
        let owner = try context()
        for ready in [true, false, true, false, true] {
            let observed = try status(ready: ready)
            XCTAssertTrue(held.matches(context: owner, status: observed, requireReady: false))
            XCTAssertEqual(held.matches(context: owner, status: observed), ready)
        }
        for changed in [try context("other"), try context(installation: "other")] {
            XCTAssertFalse(held.matches(context: changed, status: try status(), requireReady: false))
        }
        for changed in [
            try status(changes: ["busy": true]), try status(changes: ["selectedOex": true]),
            try status(runtimeChanges: ["jobId": "other"]), try status(runtimeChanges: ["nodePid": 201]),
            try status(runtimeChanges: ["postgresPid": 202]), try status(runtimeChanges: ["nodePid": NSNull()]),
            try status(runtimeChanges: ["state": "stopping"]), try status(runtimeChanges: ["failureCode": "NATIVE_PROCESS_EXITED"]),
            try status(configChanges: ["tenantId": user]), try status(configChanges: ["manifestSha256": String(repeating: "2", count: 64)]),
            try status(configChanges: ["ports": ["web": 57840, "api": 57842, "database": 57843]])
        ] { XCTAssertFalse(held.matches(context: owner, status: changed, requireReady: false)) }
        for expiry in ["2000-01-01T00:00:00Z", "invalid"] {
            XCTAssertFalse(try binding(api: api, session: session(expiry: expiry)).matches(context: owner, status: status(), requireReady: false))
        }
    }
    func testHealthGateDoesNotRotatePreviewPurposeButNavigationDoes() {
        let controller = NativeViewportController()
        let generation = controller.editingContextGeneration
        for available in [false, true, false, true] {
            controller.runtimeAvailable = available
            XCTAssertEqual(controller.editingContextActive, available)
            XCTAssertEqual(controller.editingContextGeneration, generation)
        }
        controller.editingContextActive = false
        XCTAssertNotEqual(controller.editingContextGeneration, generation)
        controller.runtimeAvailable = true
        XCTAssertFalse(controller.editingContextActive)
    }
    func testRecoveryRechecksExistingSessionAndRejectsAuthorityOrIdentityChanges() async throws {
        guard let node = ProcessInfo.processInfo.environment["EXHIBITOS_OEX_TEST_NODE"] else { throw XCTSkip("Requires owned Node fixture runtime") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("exhibitos-health-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let script = directory.appendingPathComponent("server.mjs"), mode = directory.appendingPathComponent("mode")
        try "good".write(to: mode, atomically: true, encoding: .utf8)
        try #"""
        import http from 'node:http';
        import {readFileSync} from 'node:fs';
        const [tenant,user,file]=process.argv.slice(2);
        const server=http.createServer((req,res)=>{
          const mode=readFileSync(file,'utf8');
          if(req.method!=='GET'||req.url!=='/api/v1/auth/session'||mode==='revoked'){
            res.writeHead(401);res.end('{"code":"AUTH_REQUIRED"}');return;
          }
          const value={tenantId:tenant,userId:user,role:mode==='role'?'viewer':'admin',csrfToken:'fixture',expiresAt:mode==='expiry'?'2000-01-01T00:00:00Z':'2099-01-01T00:00:00Z'};
          if(mode==='capability')value.workspaceCeilingRailProfile='changed-profile';
          res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(value));
        });
        server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
        """#.write(to: script, atomically: true, encoding: .utf8)
        let process = Process(), pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: node); process.arguments = [script.path, tenant, user, mode.path]; process.standardOutput = pipe
        try process.run()
        defer { if process.isRunning { process.terminate() }; process.waitUntilExit() }
        var data = Data()
        while data.count < 32 {
            let byte = try XCTUnwrap(try pipe.fileHandleForReading.read(upToCount: 1)); data.append(byte)
            if byte.first == 10 || byte.isEmpty { break }
        }
        let port = try XCTUnwrap(UInt16(String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        let owner = try context()
        for failure in ["none", "role", "capability", "expiry", "revoked", "runtime", "context", "generation"] {
            try "good".write(to: mode, atomically: true, encoding: .utf8)
            let api = try AuthoringAPI(port: port, tenantId: tenant)
            let authenticated = try await api.connect(credentials(port: port))
            let held = try binding(api: api, session: authenticated, port: port)
            var observations = 0
            let verify = {
                try await held.revalidate(context: { failure == "context" && observations > 0 ? try? self.context("changed") : owner }, status: {
                    observations += 1
                    return try self.status(port: port, runtimeChanges: failure == "runtime" && observations == 2 ? ["nodePid": 999] : [:])
                }, current: { failure != "generation" || observations < 2 })
            }
            if failure == "none" {
                for _ in 0..<2 {
                    XCTAssertTrue(held.matches(context: owner, status: try status(port: port, ready: false), requireReady: false))
                    try await verify()
                }
                XCTAssertEqual(observations, 4)
            } else {
                if ["role", "capability", "expiry", "revoked"].contains(failure) { try failure.write(to: mode, atomically: true, encoding: .utf8) }
                do { try await verify(); XCTFail("Changed authority/identity must fail closed: \(failure)") } catch {}
            }
        }
    }
}
