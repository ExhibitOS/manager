# SPDX-License-Identifier: Apache-2.0
import importlib.util
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("launcher", Path(__file__).with_name("manager-validation-gui.py"))
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)


class ManagerValidationGuiTests(unittest.TestCase):
    def legal_fixture(self, root):
        legal = root / "legal"
        legal.mkdir()
        (legal / "index.html").write_text('<a href="source.txt">Source</a>')
        (legal / "source.txt").write_text("Reviewed source fixture")
        inventory = legal / "inventory.json"
        inventory.write_text(json.dumps({"files": [
            {"path": p.name, "bytes": p.stat().st_size, "sha256": launcher.sha(p)} for p in sorted(legal.iterdir())
        ]}))
        return legal, inventory

    def test_rejects_per_run_path_before_checking_bundle_or_profile(self):
        with patch.object(launcher, "output") as run, patch.object(launcher, "launch_services_has_app") as services:
            with self.assertRaisesRegex(launcher.ValidationError, "restricted to"):
                launcher.launch(Path("/private/tmp/ExhibitOS Manager Recipient.app"), Path("/invalid-profile"), {})
            run.assert_not_called()
            services.assert_not_called()

    def test_preview_assembly_keeps_helper_but_no_gui_launch_entry(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            binary = root / "built-executable"
            binary.write_bytes(b"isolated build fixture")
            binary.chmod(0o755)
            app = root / "ExhibitOS Manager Recipient.app"
            producer = Path(__file__).with_name("package-native-preview.py")
            subprocess.run([sys.executable, "-B", str(producer), "--app", str(app), "--swift-executable", str(binary),
                            "--rust-helper", str(binary), "--validation-profile", str(root / "new-profile")],
                           check=True, capture_output=True)
            self.assertNotIn("CFBundleExecutable", plistlib.loads((app / launcher.INFO).read_bytes()))
            self.assertEqual((app / launcher.GUI).read_bytes(), binary.read_bytes())
            self.assertEqual((app / launcher.GUI).stat().st_mode & 0o111, 0)
            self.assertNotEqual((app / launcher.HELPER).stat().st_mode & 0o111, 0)

    def test_fixed_path_and_id_still_reject_ad_hoc_before_open(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            app, profile = root / launcher.APP.name, root / "profile"
            profile.mkdir()
            for path in (launcher.GUI, launcher.HELPER, launcher.MANIFEST):
                file = app / path
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(b"fixture")
            (app / launcher.INFO).write_bytes(plistlib.dumps(dict(CFBundleIdentifier=launcher.BUNDLE_ID,
                                                                  CFBundleExecutable=launcher.GUI.name)))
            with patch.object(launcher, "APP", app), patch.object(launcher, "output", return_value="Signature=adhoc") as run, \
                 patch.object(launcher, "launch_services_has_app") as services:
                with self.assertRaisesRegex(launcher.ValidationError, "Unexpected signer"):
                    launcher.launch(app, profile, {})
                run.assert_called_once_with(launcher.CODESIGN, "--display", "--verbose=4", str(app))
                services.assert_not_called()

    def test_rejects_wrong_bundle_id_before_signature_verification(self):
        with tempfile.TemporaryDirectory() as temp:
            app = Path(temp).resolve() / "Wrong.app"
            info = app / launcher.INFO
            info.parent.mkdir(parents=True)
            info.write_bytes(plistlib.dumps(dict(CFBundleIdentifier="org.exhibitos.manager.other",
                                                 CFBundleExecutable=launcher.GUI.name)))
            with patch.object(launcher, "verify_signature") as verify:
                with self.assertRaisesRegex(launcher.ValidationError, "bundle identifier"):
                    launcher.verify_bundle(app, Path("/invalid-profile"), {})
                verify.assert_not_called()

    def test_rejects_ad_hoc_signature(self):
        with patch.object(launcher, "output", return_value="Signature=adhoc\nTeamIdentifier=not set"):
            with self.assertRaisesRegex(launcher.ValidationError, "Unexpected signer"):
                launcher.verify_signature(launcher.APP, launcher.BUNDLE_ID)

    def test_matching_team_and_signer_do_not_accept_wrong_designated_requirement(self):
        details = f"Authority={launcher.SIGNER}\nTeamIdentifier={launcher.TEAM}"
        for path, identifier in ((launcher.APP, launcher.BUNDLE_ID),
                                 (launcher.APP / launcher.HELPER, launcher.HELPER.name)):
            with self.subTest(identifier=identifier):
                requirement = launcher.REQUIREMENT.format(identifier=identifier + ".different")
                with patch.object(launcher, "output", side_effect=[details, "designated => " + requirement]):
                    with self.assertRaisesRegex(launcher.ValidationError, "Designated requirement mismatch"):
                        launcher.verify_signature(path, identifier)

    def test_rejects_signature_integrity_failure_after_matching_designated_requirement(self):
        details = f"Authority={launcher.SIGNER}\nTeamIdentifier={launcher.TEAM}"
        requirement = launcher.REQUIREMENT.format(identifier=launcher.BUNDLE_ID)
        invalid = subprocess.CalledProcessError(1, launcher.CODESIGN)
        with patch.object(launcher, "output", side_effect=[details, "designated => " + requirement, invalid]):
            with self.assertRaisesRegex(launcher.ValidationError, "failed verification"):
                launcher.verify_signature(launcher.APP, launcher.BUNDLE_ID)

    def test_launch_refuses_an_existing_process_without_opening_another(self):
        with patch.object(launcher, "preflight"), patch.object(launcher, "pids", return_value={1234}), \
             patch.object(launcher, "output") as run:
            with self.assertRaisesRegex(launcher.ValidationError, "already open"):
                launcher.launch(launcher.APP, Path("/fixed-profile"), {})
            run.assert_not_called()

    def test_prepare_and_preflight_bind_only_inventoried_legal_resources(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            app, profile, binary, runtime = (root / name for name in (launcher.APP.name, "profile", "bin", "runtime"))
            profile.mkdir()
            binary.write_bytes(b"build fixture")
            binary.chmod(0o755)
            runtime.mkdir()
            manifest = runtime / "manifest.json"
            manifest.write_text('{"mode":"local-validation","redistributionQualified":false}')
            legal, inventory = self.legal_fixture(root)
            pins = dict(unsignedGUI=launcher.sha(binary), unsignedHelper=launcher.sha(binary),
                        runtimeManifest=launcher.sha(manifest), legalInventory=launcher.sha(inventory))
            sign_calls = []

            def run(*args):
                if args[0] == "/usr/bin/python3":
                    return subprocess.check_output([sys.executable, "-B", *args[3:]], text=True)
                self.assertEqual(args[:4], (launcher.CODESIGN, "--force", "--sign", launcher.CERT))
                sign_calls.append(args[-1])

            with patch.object(launcher, "APP", app), patch.object(launcher, "output", side_effect=run), \
                 patch.object(launcher, "verify_signature"), patch.object(launcher, "pids", return_value=set()) as pids, \
                 patch.object(launcher, "launch_services_has_app", return_value=False):
                launcher.prepare(profile, pins, binary, binary, runtime, legal, inventory)
                self.assertEqual(len(sign_calls), 2)
                self.assertEqual((app / launcher.LEGAL_INVENTORY).read_bytes(), inventory.read_bytes())
                self.assertEqual({p.name for p in (app / launcher.LEGAL).iterdir()}, {"index.html", "source.txt"})
                launcher.preflight(app, profile, pins)
                pids.reset_mock()
                (app / launcher.LEGAL / "source.txt").write_text("changed")
                with self.assertRaisesRegex(launcher.ValidationError, "differs from"):
                    launcher.launch(app, profile, pins)
                pids.assert_not_called()

    def test_legal_input_refuses_wrong_pin_unlisted_file_and_link(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            legal, inventory = self.legal_fixture(root)
            with self.assertRaisesRegex(launcher.ValidationError, "SHA-256"):
                launcher.read_legal_inventory(inventory, "0" * 64)
            _, files = launcher.read_legal_inventory(inventory, launcher.sha(inventory))
            (legal / "extra.txt").write_text("unlisted")
            with self.assertRaisesRegex(launcher.ValidationError, "differs from"):
                launcher.verify_legal_resources(legal, files, inventory=inventory)
            (legal / "extra.txt").unlink()
            (legal / "extra.txt").symlink_to(legal / "source.txt")
            with self.assertRaisesRegex(launcher.ValidationError, "Links are not allowed"):
                launcher.verify_legal_resources(legal, files, inventory=inventory)


class ManagerSceneCaptureTests(unittest.TestCase):
    def capture_fixture(self, root):
        profile, app, directory = root / "profile", root / "app", root / "output"
        for path in (profile, app, directory): path.mkdir(mode=0o700)
        value = dict(version=1, requestId="11111111-1111-4111-8111-111111111111",
            workspaceId="22222222-2222-4222-8222-222222222222", tenantId="33333333-3333-4333-8333-333333333333",
            principalId="44444444-4444-4444-8444-444444444444", runtimeManifestSHA256="a" * 64,
            expectedNativeRevision=7, expectedEditVersion=2, exhibitionId="55555555-5555-4555-8555-555555555555",
            serverRevision=3, expectedETag='"studio-r3-' + "b" * 64 + '"',
            placementId="66666666-6666-4666-8666-666666666666", viewpointId="77777777-7777-4777-8777-777777777777",
            outputDirectory=str(directory))
        request = root / "request.json"; request.write_text(json.dumps(value)); request.chmod(0o600)
        return profile, app, directory, request, value

    def test_capture_request_scope_and_private_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); profile, app, directory, request, value = self.capture_fixture(root)
            with patch.object(launcher, "APP", app):
                actual, destination = launcher.capture_request(request, profile, dict(runtimeManifest="a" * 64))
                self.assertEqual(actual, value); self.assertEqual(destination, directory)
                for key, wrong in [("extra", True), ("expectedNativeRevision", True), ("expectedETag", '"studio-r2-' + "b" * 64 + '"'),
                                   ("runtimeManifestSHA256", "c" * 64), ("outputDirectory", str(profile)),
                                   ("outputDirectory", str(directory) + "/")]:
                    broken = dict(value); broken[key] = wrong; request.write_text(json.dumps(broken))
                    with self.assertRaises(launcher.ValidationError):
                        launcher.capture_request(request, profile, dict(runtimeManifest="a" * 64))
                request.write_text(json.dumps(value)); request.chmod(0o644)
                with self.assertRaises(launcher.ValidationError): launcher.capture_request(request, profile, dict(runtimeManifest="a" * 64))

    def test_capture_rejects_redirected_or_existing_output_before_launch(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); profile, app, directory, request, value = self.capture_fixture(root)
            with patch.object(launcher, "APP", app):
                (directory / "scene.png").write_bytes(b"retained")
                with self.assertRaises(launcher.ValidationError): launcher.capture_request(request, profile, dict(runtimeManifest="a" * 64))
                (directory / "scene.png").unlink()
                link = root / "redirected"; link.symlink_to(directory)
                value["outputDirectory"] = str(link); request.write_text(json.dumps(value))
                with self.assertRaises(launcher.ValidationError): launcher.capture_request(request, profile, dict(runtimeManifest="a" * 64))

    def test_capture_requires_explicit_signed_gui_admission(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); profile, app, directory, request, value = self.capture_fixture(root)
            (app / launcher.INFO).parent.mkdir(); (app / launcher.INFO).write_bytes(plistlib.dumps({}))
            with patch.object(launcher, "preflight"), patch.object(launcher, "launch") as start:
                with self.assertRaisesRegex(launcher.ValidationError, "not been admitted"):
                    launcher.capture(app, profile, dict(unsignedGUI="c" * 64), request)
                start.assert_not_called()

    def test_launch_passes_only_capture_request_to_fixed_app(self):
        with patch.object(launcher, "preflight"), patch.object(launcher, "pids", side_effect=[set(), {77}]), \
             patch.object(launcher, "launch_services_has_app", side_effect=[False, True]), patch.object(launcher, "output") as run:
            result = launcher.launch(launcher.APP, Path("/owned-profile"), {}, capture_request=Path("/owned-request.json"))
            self.assertEqual(result, 77)
            run.assert_called_once_with("/usr/bin/open", "-a", str(launcher.APP), "--args", "--native-scene-capture-request", "/owned-request.json")

    def test_result_binds_image_camera_saved_state_and_cleanup(self):
        import hashlib
        import struct
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); profile, app, directory, request, value = self.capture_fixture(root)
            png = b"\x89PNG\r\n\x1a\n" + b"\x00\x00\x00\x0dIHDR" + struct.pack(">II", 640, 360)
            image = directory / "scene.png"; image.write_bytes(png); image.chmod(0o600)
            result = {key: value[key] for key in ("workspaceId", "tenantId", "principalId", "runtimeManifestSHA256", "exhibitionId", "serverRevision", "placementId", "viewpointId")}
            result.update(version=1, requestId=value["requestId"], pid=77, bundleId=launcher.BUNDLE_ID, status="succeeded",
                nativeRevision=7, editVersion=2, etag=value["expectedETag"], documentUnchanged=True, runtimeDrained=True, helperClosed=True,
                candidateSha256="c" * 64, camera=dict(viewpointId=value["viewpointId"]), target=dict(placementId=value["placementId"]),
                png=dict(file="scene.png", bytes=len(png), sha256=hashlib.sha256(png).hexdigest(), width=640, height=360))
            receipt = directory / "capture.json"; receipt.write_text(json.dumps(result)); receipt.chmod(0o600)
            self.assertEqual(launcher.check_capture_result(value, directory, 77), result)
            for key, wrong in [("pid", 88), ("etag", "stale"), ("documentUnchanged", False), ("runtimeDrained", False),
                               ("nativeRevision", 8), ("camera", {}), ("candidateSha256", "bad")]:
                broken = dict(result); broken[key] = wrong; receipt.write_text(json.dumps(broken))
                with self.assertRaises(launcher.ValidationError): launcher.check_capture_result(value, directory, 77)
            receipt.write_text(json.dumps(result)); image.write_bytes(png + b"changed")
            with self.assertRaises(launcher.ValidationError): launcher.check_capture_result(value, directory, 77)


if __name__ == "__main__":
    unittest.main()
