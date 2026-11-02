// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class PublicationReleaseTransportTests: XCTestCase {
    func testAuthenticatedReleaseBindsExactPublicationAndLostResponseCanBeReadAfterReconnect() async throws {
        guard let node = ProcessInfo.processInfo.environment["EXHIBITOS_PUBLICATION_TEST_NODE"] else { throw XCTSkip("Explicit owned Node fixture runtime only") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("exhibitos-publication-wire-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let script = directory.appendingPathComponent("server.mjs")
        try #"""
        import http from 'node:http';
        const [tenant,user]=process.argv.slice(2), csrf='owned-publication-wire-csrf', token='a'.repeat(43), prefix='/api/v1/tenants/'+tenant+'/studio', hash='b'.repeat(64), date='2026-10-29T00:00:00Z';
        let lostStatus='published',lostPrepare=false;const prepared=new Map(),etag='"studio-r1-'+ 'a'.repeat(64)+'"';
        const publication=(id,status='unpublished')=>({publicationId:id,publicUrl:'/p/'+id,revisionSha256:hash,publishedAt:date,status});
        const server=http.createServer((req,res)=>{
          let body='';req.on('data',b=>body+=b);req.on('end',()=>{
            const send=(code,value)=>{res.writeHead(code,{'content-type':'application/json'});res.end(JSON.stringify(value));};
            if(req.url==='/api/v1/auth/session'){send(200,{tenantId:tenant,userId:user,role:'admin',csrfToken:csrf,expiresAt:new Date(Date.now()+3600000).toISOString(),...(req.headers.cookie?.includes('exhibitos_local_session='+token)?{nativePublicationProfile:'bounded-local-v1'}:{})});return;}
            const base='http://127.0.0.1:'+server.address().port;
            if(req.headers.origin!==base||!req.headers.cookie?.includes('exhibitos_local_session='+token)){send(403,{code:'FIXTURE_AUTH_INVALID'});return;}
            const selected=req.url.match(new RegExp('^'+prefix+'/exhibitions/(00000000-0000-4000-8000-00000000000[89])/(ready|publications)$'));
            if(selected){
              const id=selected[1],bounded=id.endsWith('8'),profile=req.headers['x-exhibitos-publication-profile'];
              if((bounded?profile!=='bounded-local-v1':profile!==undefined)||((bounded||req.method==='POST')?req.headers['if-match']!==etag:req.headers['if-match']!==undefined)){send(400,{code:'FIXTURE_PROFILE_INVALID'});return;}
              if(req.method==='GET'&&selected[2]==='ready'){send(200,{status:'READY',etag,issues:[]});return;}
              const requestId=JSON.parse(body).requestId;if(req.method!=='POST'||selected[2]!=='publications'||req.headers['x-csrf-token']!==csrf||requestId!==(bounded?'11111111-1111-4111-8111-111111111111':'22222222-2222-4222-8222-222222222222')){send(400,{code:'FIXTURE_MUTATION_INVALID'});return;}
              if(!prepared.has(requestId))prepared.set(requestId,publication(id,'published'));
              if(bounded&&!lostPrepare){lostPrepare=true;req.socket.destroy();return;}
              send(200,prepared.get(requestId));return;
            }
            const lostId='00000000-0000-4000-8000-000000000007';
            if(req.method==='GET'&&req.url===prefix+'/exhibitions/'+lostId+'/publications'){send(200,{items:[publication(lostId,lostStatus)]});return;}
            const match=req.url.match(new RegExp('^'+prefix+'/publications/(00000000-0000-4000-8000-00000000000[1-7])/unpublish$'));
            if(req.method!=='POST'||req.headers['x-csrf-token']!==csrf||body!=='{}'||!match){send(400,{code:'FIXTURE_MUTATION_INVALID'});return;}
            const id=match[1],kind=id.at(-1),value=publication(id);
            if(kind==='2'){value.publicationId='00000000-0000-4000-8000-000000000009';value.publicUrl='/p/'+value.publicationId;}
            if(kind==='3')value.revisionSha256='c'.repeat(64);
            if(kind==='4')value.publishedAt='2026-10-29T00:01:00Z';
            if(kind==='5')value.status='published';
            if(kind==='6'){res.writeHead(302,{location:base+'/redirect-target'});res.end();return;}
            if(kind==='7'&&lostStatus==='published'){lostStatus='unpublished';req.socket.destroy();return;}
            send(200,value);
          });
        });
        server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
        process.on('SIGTERM',()=>server.close(()=>process.exit(0)));
        """#.write(to: script, atomically: true, encoding: .utf8)
        let process = Process(), pipe = Pipe()
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased()
        process.executableURL = URL(fileURLWithPath: node); process.arguments = [script.path, tenant, user]; process.standardOutput = pipe
        try process.run()
        defer { if process.isRunning { process.terminate() }; process.waitUntilExit(); try? FileManager.default.removeItem(at: directory) }
        var line = Data()
        while line.count < 32 {
            let byte = try XCTUnwrap(try pipe.fileHandleForReading.read(upToCount: 1)); if byte.isEmpty { break }; line.append(byte); if byte.first == 10 { break }
        }
        let port = try XCTUnwrap(UInt16(String(decoding: line, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        let credentials = LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:\(port)", token: String(repeating: "a", count: 43))
        let api = try AuthoringAPI(port: port, tenantId: tenant); _ = try await api.connect(credentials)
        func expected(_ suffix: String) -> NativePublication {
            let id = "00000000-0000-4000-8000-00000000000" + suffix
            return NativePublication(publicationId: id, publicUrl: "/p/" + id, revisionSha256: String(repeating: "b", count: 64), publishedAt: "2026-10-29T00:00:00Z", status: "published")
        }
        let selectedID = "00000000-0000-4000-8000-000000000008", legacyID = "00000000-0000-4000-8000-000000000009"
        let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\"", requestID = "11111111-1111-4111-8111-111111111111"
        let legacyReady = try await api.publicationReady(exhibitionId: legacyID)
        XCTAssertEqual(legacyReady.status, "READY")
        let legacyPrepared = try await api.preparePublication(exhibitionId: legacyID, etag: etag, requestId: "22222222-2222-4222-8222-222222222222")
        XCTAssertEqual(legacyPrepared.publicationId, legacyID)
        let boundedReady = try await api.publicationReady(exhibitionId: selectedID, etag: etag, publicationProfile: "bounded-local-v1")
        XCTAssertEqual(boundedReady.etag, etag)
        do { _ = try await api.preparePublication(exhibitionId: selectedID, etag: etag, requestId: requestID, publicationProfile: "bounded-local-v1"); XCTFail("A lost bounded response must stay unresolved") } catch { }
        let sameRequest = try await api.preparePublication(exhibitionId: selectedID, etag: etag, requestId: requestID, publicationProfile: "bounded-local-v1")
        XCTAssertEqual(sameRequest.publicationId, selectedID)
        let unavailable = try AuthoringAPI(port: port, tenantId: tenant)
        _ = try await unavailable.connect(LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:\(port)", token: String(repeating: "b", count: 43)))
        do { _ = try await unavailable.publicationReady(exhibitionId: selectedID, etag: etag, publicationProfile: "bounded-local-v1"); XCTFail("A missing affirmative capability must reject before request") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "PUBLICATION_STAGE_UNAVAILABLE") }
        let first = try await api.unpublish(expected("1"))
        XCTAssertEqual(first.status, "unpublished")
        for suffix in ["2", "3", "4", "5", "6"] {
            do { _ = try await api.unpublish(expected(suffix)); XCTFail("Changed status identity must fail") }
            catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, suffix == "6" ? "REQUEST_FAILED" : "PUBLICATION_SCOPE_CHANGED") }
        }
        do { _ = try await api.unpublish(expected("7")); XCTFail("Lost response must not be reported as success") } catch {}
        let reopened = try AuthoringAPI(port: port, tenantId: tenant); _ = try await reopened.connect(credentials)
        let records = try await reopened.publications(exhibitionId: expected("7").publicationId)
        XCTAssertEqual(records.count, 1)
        XCTAssertEqual(try expected("7").acceptingStatus(XCTUnwrap(records.first)).status, "unpublished")
        // Replaying the same state change cannot create a second publication.
        let replayed = try await reopened.unpublish(expected("7"))
        XCTAssertEqual(replayed.status, "unpublished")
    }
}
