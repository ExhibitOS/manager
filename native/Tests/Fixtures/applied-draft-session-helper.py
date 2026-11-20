#!/usr/bin/python3
# SPDX-License-Identifier: Apache-2.0
"""Synthetic HTTP/stdio fixture for public NativeAutomationSession entry tests."""
import copy
import json
import os
from pathlib import Path
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

ROOT = Path(__file__).resolve().parent
FIXTURE = json.loads((ROOT / "fixture.json").read_bytes())
SOURCE = FIXTURE["document"]

if sys.argv[1:] == ["--serve"]:
    floor = FIXTURE.get("floorRecording")
    revision = int(SOURCE["etag"].split("-")[1][1:])
    head = dict(revision=revision, etag=SOURCE["etag"], draft=SOURCE["draft"])
    receipt = None
    original = None
    stats = dict(puts=0, commits=0, lastRequestId=None, audioReads=0, floorChecks=0)

    def write_stats():
        (ROOT / "server-state.json").write_text(json.dumps(stats))

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def send_json(self, value, status=200, etag=None):
            body = json.dumps(value, separators=(",", ":")).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "close")
            if etag:
                self.send_header("ETag", etag)
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path == "/api/v1/auth/session":
                self.send_json(dict(tenantId=SOURCE["tenantId"], userId=SOURCE["principalId"],
                    role="admin", csrfToken="synthetic-csrf", expiresAt="2099-01-01T00:00:00Z",
                    workspaceFurnishingProfile="workspace-furnishing-intake-v1",
                    nativeDefaultSeatProposalProfile="default-seat-proposal-v1",
                    nativeFurnishingWalkingCollisionProfile="furnishing-walking-collision-v1",
                    nativeAuthoringProposalProfile="saved-head-v1", nativePublicationProfile="bounded-local-v1",
                    nativeSceneAuthoringProposalProfile="saved-head-scene-v1",
                    nativeFixtureAuthoringProposalProfile="saved-head-fixture-v1",
                    nativeFloorRecordingAttachmentProfile="saved-head-floor-recording-attachment-v1"))
            elif floor and self.path.endswith("/audio/" + floor["audioId"]):
                stats["audioReads"] += 1
                write_stats()
                self.send_json(dict(id=floor["audioId"], state="approved", revision=1,
                    revoked=(ROOT / "revoke-floor-recording").exists(), durationSeconds=0.6260625,
                    mediaAsset=floor["expectedMediaAsset"]))
            elif self.path.endswith("/native-viewport-policy"):
                self.send_json(dict(exhibitionId=SOURCE["draft"]["exhibitionId"], revision=head["revision"],
                    etag=head["etag"], boundedReadOnly=False, boundedProfile=False, boundedEditRequired=False), etag=head["etag"])
            elif self.path.endswith("/" + SOURCE["draft"]["exhibitionId"]):
                if floor and (ROOT / "advance-floor-head").exists():
                    etag = '"studio-r' + str(head["revision"] + 1) + '-' + "b" * 64 + '"'
                    return self.send_json(dict(head, revision=head["revision"] + 1, etag=etag), etag=etag)
                self.send_json(head, etag=head["etag"])
            else:
                self.send_json(dict(error=dict(code="FIXTURE_UNEXPECTED_REQUEST")), 404)

        def do_POST(self):
            if not self.path.endswith("/native-authoring-proposal"):
                return self.send_json(dict(error=dict(code="FIXTURE_UNEXPECTED_REQUEST")), 404)
            envelope = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            request = envelope["request"]
            candidate = copy.deepcopy(head["draft"]["candidate"])
            reply = {key: request[key] for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"]}
            if floor and request["type"] == "attach-floor-recording":
                assert self.headers["If-Match"] == head["etag"]
                assert all(request[key] == value for key, value in floor.items())
                stats["floorChecks"] += 1
                write_stats()
                return self.send_json(dict(**reply, surfaceId=floor["surfaceId"], candidate=candidate), etag=head["etag"])
            assert request["type"] == "set-furnishing-visual"
            candidate.setdefault("extensions", {})["org.exhibitos.studio/furnishing-visuals"] = dict(
                version=1, assets=[FIXTURE["asset"]], bindings=[dict(hostKind="light", hostId=request["hostId"],
                    assetId=FIXTURE["asset"]["id"], pose=request["pose"])])
            self.send_json(dict(**reply, candidate=candidate), etag=head["etag"])

        def do_PUT(self):
            global head, receipt, original
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            intent = [self.headers.get("If-Match"), body]
            stats["puts"] += 1
            stats["lastRequestId"] = body.get("requestId")
            if receipt is not None:
                if intent != original:
                    return self.send_json(dict(error=dict(code="REQUEST_CONFLICT")), 409)
            else:
                if intent[0] != head["etag"]:
                    return self.send_json(dict(error=dict(code="REMOTE_CONFLICT")), 412)
                stats["commits"] += 1
                original = intent
                head = dict(revision=13, etag='"studio-r13-' + "b" * 64 + '"', draft=body["draft"])
                receipt = copy.deepcopy(head)
            write_stats()
            self.send_json(receipt, etag=receipt["etag"])

    with HTTPServer(("127.0.0.1", 0), Handler) as server:
        server.timeout = .1
        ready = ROOT / "server-ready.tmp"
        ready.write_text(json.dumps(dict(web=server.server_port)))
        ready.rename(ROOT / "server-ready.json")
        while not (ROOT / "stop").exists():
            server.handle_request()
    sys.exit(0)

assert len(sys.argv) == 3 and sys.argv[1] == "--profile" and Path(sys.argv[2]).resolve(strict=True) == ROOT / "profile"
port = json.loads((ROOT / "server-ready.json").read_bytes())["web"]
ports = dict(web=port, api=18102 if port != 18102 else 18104, database=18103 if port != 18103 else 18105)
document = copy.deepcopy(SOURCE)
active = False
writes = 0
for line in sys.stdin:
    request = json.loads(line)
    command = request["command"]
    if command == "context":
        result = dict(selectionToken="55555555-5555-4555-8555-555555555555",
            activeId="66666666-6666-4666-8666-666666666666", installations=[])
    elif command == "document_state":
        result = document
    elif command == "save_document_state":
        assert request["document"]["revision"] == document["revision"]
        document = request["document"]
        document["revision"] += 1
        writes += 1
        (ROOT / "helper-state.json").write_text(json.dumps(dict(documentWrites=writes)))
        result = document
    elif command in ["native_status", "native_start"]:
        active |= command == "native_start"
        runtime = dict(jobId="77777777-7777-4777-8777-777777777777", state="ready", ports=ports,
            readiness=dict(ready=True), postgresPid=12345, nodePid=12346, auxiliaryPid=12347,
            exits={}, logTruncated=False) if active else None
        result = dict(busy=False, operation="idle", accountRequired=False, selectedOex=False,
            configuration=dict(kind="personal", tenantId=SOURCE["tenantId"], ports=ports,
                manifestSha256=FIXTURE["manifest"]), runtime=runtime)
    elif command == "native_authoring_session":
        result = dict(tenantId=SOURCE["tenantId"], principalId=SOURCE["principalId"],
            origin="http://127.0.0.1:" + str(port), token="a" * 43)
    else:
        raise AssertionError("Unexpected fixture RPC: " + command)
    print(json.dumps(dict(version=1, id=request["id"], result=result)), flush=True)
