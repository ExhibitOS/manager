# SPDX-License-Identifier: Apache-2.0
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
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

    def test_retirement_removes_only_reviewed_candidate_after_rebuild_checks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            app = root / "fixed.app"
            app.mkdir()
            candidate = root / "retained-aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa.app"
            kept = root / "retained-bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb.app"
            kept.mkdir()
            profile = root / "profile"
            profile.mkdir()
            gui, helper, runtime = (root / name for name in ("gui", "helper", "runtime"))
            gui.write_bytes(b"unsigned gui")
            helper.write_bytes(b"unsigned helper")
            runtime.mkdir()
            (runtime / "manifest.json").write_bytes(b"runtime manifest")
            for name in (launcher.GUI, launcher.HELPER):
                (candidate / name).parent.mkdir(parents=True)
                (candidate / name).write_bytes(b"signed specimen")
            (candidate / launcher.INFO).write_bytes(b"info")
            shutil.copytree(runtime, candidate / launcher.MANIFEST.parent)
            plan_path, proof_path, release_path = (root / name for name in ("plan.json", "producer.json", "consumer.json"))
            proof_path.write_text("reviewed producer fixture")
            release_path.write_text("accepted return fixture")
            pins = dict(unsignedGUI=launcher.sha(gui), unsignedHelper=launcher.sha(helper),
                        runtimeManifest=launcher.sha(runtime / "manifest.json"))
            replacement = dict(app=str(app), profile=str(profile), pins=pins, pid=1234, owner="Native")
            plan = dict(candidate=str(candidate), candidatePins=pins, replacement=replacement,
                candidateSigned={key: launcher.sha(candidate / name) for key, name in
                                 (("gui", launcher.GUI), ("helper", launcher.HELPER), ("info", launcher.INFO))},
                rebuild=dict(gui=str(gui), helper=str(helper), runtime=str(runtime), buildRecord=str(proof_path),
                             buildRecordSha256=launcher.sha(proof_path)),
                consumerRelease=dict(path=str(release_path), sha256=launcher.sha(release_path)))
            plan_path.write_text(json.dumps(plan))
            returned = {**replacement, "returnVerified": True}
            with patch.object(launcher, "APP", app), patch.object(launcher, "verify_bundle"), \
                 patch.object(launcher, "pids", return_value=set()), \
                 patch.object(launcher, "launch_services_has_app", return_value=False), \
                 patch.object(launcher.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, b"", b"")):
                with self.assertRaisesRegex(launcher.ValidationError, "returned replacement"):
                    launcher.retire_approved_bundle(plan_path, launcher.sha(plan_path), profile,
                                                    {**returned, "pid": 4321})
                (runtime / "foreign").touch()
                with self.assertRaisesRegex(launcher.ValidationError, "exact equivalent"):
                    launcher.retire_approved_bundle(plan_path, launcher.sha(plan_path), profile, returned)
                self.assertTrue(candidate.exists())
                self.assertEqual(list(root.glob("*.PRUNED.json")), [])
                inside = candidate / "plan.json"
                inside.write_bytes(plan_path.read_bytes())
                with self.assertRaisesRegex(launcher.ValidationError, "plan outside"):
                    launcher.retire_approved_bundle(inside, launcher.sha(inside), profile, returned)
                inside.unlink()
                (runtime / "foreign").unlink()
                original = root / "candidate-before-swap"
                def swap_candidate_during_handle_check(*args, **kwargs):
                    candidate.rename(original)
                    candidate.mkdir()
                    return subprocess.CompletedProcess([], 1, b"", b"")

                with patch.object(launcher.subprocess, "run", side_effect=swap_candidate_during_handle_check):
                    with self.assertRaisesRegex(launcher.ValidationError, "directory identity changed"):
                        launcher.retire_approved_bundle(plan_path, launcher.sha(plan_path), profile, returned)
                self.assertTrue(candidate.is_dir() and original.is_dir())
                self.assertEqual(list(root.glob("*.PRUNED.json")), [])
                candidate.rmdir()
                original.rename(candidate)
                (candidate / launcher.MANIFEST.parent).chmod(0o555)
                runtime.chmod(0o555)
                def remove_one_file_then_fail(path):
                    self.assertEqual(path, candidate)
                    pending, = root.glob("*.PRUNED.json")
                    self.assertEqual(json.loads(pending.read_text())["status"], "pending")
                    (candidate / launcher.MANIFEST).unlink()
                    raise PermissionError("synthetic partial removal")

                with patch.object(launcher.shutil, "rmtree", side_effect=remove_one_file_then_fail) as remove:
                    remove.avoids_symlink_attacks = True
                    with self.assertRaisesRegex(launcher.ValidationError, "Retirement did not complete"):
                        launcher.retire_approved_bundle(plan_path, launcher.sha(plan_path), profile, returned)
                failed, = root.glob("*.outcome.json")
                evidence = json.loads(failed.read_text())
                self.assertEqual(evidence["status"], "failed")
                self.assertTrue(evidence["existsAfter"])
                self.assertIn("observedExternalFreeAfter", evidence)
                self.assertFalse((candidate / launcher.MANIFEST).exists())
                self.assertEqual((candidate / launcher.MANIFEST.parent).stat().st_mode & 0o777, 0o555)
                # A partially removed signed candidate cannot pass revalidation.
                with self.assertRaisesRegex(launcher.ValidationError, "exact equivalent"):
                    launcher.retire_approved_bundle(plan_path, launcher.sha(plan_path), profile, returned)
                # Simulate an owner restoring the lost fixture byte from the approved input.
                (candidate / launcher.MANIFEST.parent).chmod(0o755)
                shutil.copy2(runtime / "manifest.json", candidate / launcher.MANIFEST)
                (candidate / launcher.MANIFEST.parent).chmod(0o555)
                outcome = launcher.retire_approved_bundle(plan_path, launcher.sha(plan_path), profile, returned)
            self.assertFalse(outcome["existsAfter"])
            self.assertEqual(json.loads(Path(outcome["outcome"]).read_text())["status"], "completed")
            self.assertEqual(json.loads(failed.read_text()), evidence)
            self.assertEqual(len(list(root.glob("*.PRUNED.json"))), 2)
            self.assertFalse(candidate.exists())
            self.assertTrue(app.exists() and kept.exists() and runtime.exists() and profile.exists())

    def test_return_check_requires_matching_receipt_and_both_free_profile_leases(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(launcher, "preflight"), \
             patch.object(launcher, "pids", return_value=set()) as running, \
             patch.object(launcher, "launch_services_has_app", return_value=False):
            root = Path(temp).resolve()
            profile = root / "profile"
            profile.mkdir()
            anchor = hashlib.sha256(str(profile).encode()).hexdigest()
            paths = (root / f".exhibitos-profile-session-{anchor}.lock", profile / "profile-session.lock")
            for path in paths:
                path.touch(mode=0o600)
            receipt = root / "launch.json"
            pins = {"runtimeManifest": "f" * 64}
            value = {"app": str(launcher.APP), "bundleId": launcher.BUNDLE_ID, "profile": str(profile),
                     "pins": pins, "pid": 1234, "owner": "Manager Native"}
            receipt.write_text(json.dumps(value))
            returned = launcher.check_return(launcher.APP, profile, pins, receipt)
            self.assertEqual(returned["pid"], 1234)
            self.assertEqual(returned["owner"], "Manager Native")
            self.assertTrue(returned["returnVerified"])
            for path in paths:
                with path.open("r") as held:
                    fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    with self.assertRaisesRegex(launcher.ValidationError, "profile lease is still held"):
                        launcher.check_return(launcher.APP, profile, pins, receipt)
            paths[1].chmod(0o644)
            with self.assertRaisesRegex(launcher.ValidationError, "unexpected custody"):
                launcher.check_return(launcher.APP, profile, pins, receipt)
            paths[1].unlink()
            os.mkfifo(paths[1], mode=0o600)
            with self.assertRaisesRegex(launcher.ValidationError, "unexpected custody"):
                launcher.check_return(launcher.APP, profile, pins, receipt)
            paths[1].unlink()
            paths[1].touch(mode=0o600)

            checks = 0

            def registry_replaces_anchor(*, strict=False):
                nonlocal checks
                checks += 1
                self.assertTrue(strict)
                if checks == 2:
                    paths[0].rename(root / "old-anchor")
                    paths[0].touch(mode=0o600)
                return False

            with patch.object(launcher, "launch_services_has_app", side_effect=registry_replaces_anchor):
                with self.assertRaisesRegex(launcher.ValidationError, "no longer matches"):
                    launcher.check_return(launcher.APP, profile, pins, receipt)
            running.side_effect = lambda executable=launcher.GUI: {1235} if executable == launcher.HELPER else set()
            with self.assertRaisesRegex(launcher.ValidationError, "helper is still registered"):
                launcher.check_return(launcher.APP, profile, pins, receipt)
            receipt.write_text(json.dumps({**value, "profile": str(root / "other")}))
            with self.assertRaisesRegex(launcher.ValidationError, "launch receipt must match"):
                launcher.check_return(launcher.APP, profile, pins, receipt)

    def test_return_registry_check_does_not_treat_probe_failure_as_absence(self):
        failed = subprocess.CompletedProcess([], 1, stdout="", stderr="unavailable")
        with patch.object(launcher.subprocess, "run", return_value=failed):
            with self.assertRaisesRegex(launcher.ValidationError, "Unable to verify"):
                launcher.launch_services_has_app(strict=True)

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


class InternalPublicationTests(unittest.TestCase):
    def fixture(self, root):
        app, candidate, profile = (root / name for name in ("fixed.app", "staged.app", "profile"))
        profile.mkdir()
        old = dict(unsignedGUI="a" * 64, unsignedHelper="b" * 64, runtimeManifest="c" * 64)
        pins = dict(old, unsignedHelper="d" * 64)
        for bundle, selected in [(app, old), (candidate, pins)]:
            for relative, data in [(launcher.GUI, b"same GUI"), (launcher.HELPER, selected["unsignedHelper"].encode()),
                    (launcher.MANIFEST, b"same runtime"), (Path("Contents/Resources/native-runtime/payload"), b"immutable payload"),
                    (Path("Contents/_CodeSignature/CodeResources"), selected["unsignedHelper"].encode()),
                    (launcher.INFO, plistlib.dumps({"ExhibitOSValidationInputs": selected}))]:
                path = bundle / relative; path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data); path.chmod(0o500 if relative in (launcher.GUI, launcher.HELPER) else 0o444)
            alias = bundle / "Contents/Resources/native-runtime/alias"
            alias.symlink_to("payload")
            os.chmod(alias, 0o700, follow_symlinks=False)
        return app, candidate, profile, pins

    def test_real_clone_and_atomic_replacement_preserve_fixed_and_match_signed_stage(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); app, candidate, profile, pins = self.fixture(root)
            before = launcher.bundle_inventory(app)
            with patch.object(launcher, "APP", app), patch.object(launcher, "verify_bundle"):
                launcher.clone_publication(candidate, root / "publication.app", profile, pins)
            published = root / "publication.app"
            self.assertEqual(launcher.bundle_inventory(app), before)
            self.assertEqual(launcher.bundle_inventory(published), launcher.bundle_inventory(candidate))
            for relative in (launcher.HELPER, launcher.MANIFEST):
                self.assertNotEqual((app / relative).stat().st_ino, (published / relative).stat().st_ino)
                self.assertEqual((app / relative).stat().st_nlink, 1)

    def test_clone_failure_never_falls_back_or_changes_current(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); app, candidate, profile, pins = self.fixture(root)
            before = launcher.bundle_inventory(app)
            with patch.object(launcher, "APP", app), patch.object(launcher, "verify_bundle"), \
                 patch.object(launcher, "clone_file", side_effect=launcher.ValidationError("clone refused")), \
                 patch.object(launcher.shutil, "copy2") as copy:
                with self.assertRaisesRegex(launcher.ValidationError, "clone refused"):
                    launcher.clone_publication(candidate, root / "publication.app", profile, pins)
                copy.assert_not_called()
            self.assertEqual(launcher.bundle_inventory(app), before)

    def test_runtime_inventory_pin_and_space_drift_refuse_before_clone(self):
        for mutation in ("runtime", "inventory", "pin", "space"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                root = Path(temp).resolve(); app, candidate, profile, pins = self.fixture(root)
                before = launcher.bundle_inventory(app)
                if mutation == "runtime":
                    target = candidate / launcher.MANIFEST; target.chmod(0o600); target.write_bytes(b"changed")
                elif mutation == "inventory": (candidate / "extra").write_bytes(b"extra")
                elif mutation == "pin": pins["runtimeManifest"] = "e" * 64
                free = 3 * 1024 ** 3 if mutation == "space" else 20 * 1024 ** 3
                with patch.object(launcher, "APP", app), patch.object(launcher, "verify_bundle"), \
                     patch.object(launcher.shutil, "disk_usage", return_value=shutil._ntuple_diskusage(0, 0, free)), \
                     patch.object(launcher, "clone_bundle") as clone:
                    with self.assertRaises(launcher.ValidationError):
                        launcher.clone_publication(candidate, root / "publication.app", profile, pins)
                    clone.assert_not_called()
                self.assertEqual(launcher.bundle_inventory(app), before)

    def test_bad_clone_inventory_refuses_before_replacing_entries(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); app, candidate, profile, pins = self.fixture(root)
            before = launcher.bundle_inventory(app)
            def bad_clone(source, destination):
                shutil.copytree(source, destination)
                (destination / "extra").write_bytes(b"corrupt")
            with patch.object(launcher, "APP", app), patch.object(launcher, "verify_bundle"), \
                 patch.object(launcher, "clone_bundle", side_effect=bad_clone), patch.object(launcher.shutil, "copy2") as copy:
                with self.assertRaisesRegex(launcher.ValidationError, "cloned fixed bundle differs"):
                    launcher.clone_publication(candidate, root / "publication.app", profile, pins)
                copy.assert_not_called()
            self.assertEqual(launcher.bundle_inventory(app), before)

    def test_internal_prepare_reuses_verified_stage_and_retains_old_bundle(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); app, profile, runtime, staging = (root / name for name in ("fixed.app", "profile", "runtime", "staging"))
            for path in (profile, runtime, staging): path.mkdir(mode=0o700)
            (runtime / "manifest.json").write_text('{"mode":"local-validation","redistributionQualified":false}')
            gui, old_helper, helper = (root / name for name in ("gui", "old-helper", "helper"))
            for path, data in [(gui, b"GUI fixture"), (old_helper, b"old helper fixture"), (helper, b"new helper fixture")]:
                path.write_bytes(data); path.chmod(0o500)
            old_pins = dict(unsignedGUI=launcher.sha(gui), unsignedHelper=launcher.sha(old_helper), runtimeManifest=launcher.sha(runtime / "manifest.json"))
            pins = dict(old_pins, unsignedHelper=launcher.sha(helper))
            def run(*args):
                if args[0] == "/usr/bin/python3":
                    return subprocess.check_output([sys.executable, "-B", *args[3:]], text=True)
                self.assertEqual(args[:4], (launcher.CODESIGN, "--force", "--sign", launcher.CERT))
                target = Path(args[-1])
                if target.is_dir():
                    signature = target / "Contents/_CodeSignature/CodeResources"
                    signature.parent.mkdir(); signature.write_text(launcher.sha(target / launcher.HELPER))
                return ""
            with patch.object(launcher, "APP", app), patch.object(launcher, "output", side_effect=run), \
                 patch.object(launcher, "verify_signature"), patch.object(launcher, "pids", return_value=set()), \
                 patch.object(launcher, "launch_services_has_app", return_value=False), \
                 patch.object(launcher.shutil, "disk_usage", return_value=SimpleNamespace(free=12 << 30)):
                launcher.prepare(profile, old_pins, gui, old_helper, runtime)
                before = launcher.bundle_inventory(app)
                with patch.object(launcher, "clone_publication", side_effect=launcher.ValidationError("retain signed stage")):
                    with self.assertRaisesRegex(launcher.ValidationError, "retain signed stage"):
                        launcher.prepare(profile, pins, gui, helper, runtime, staging_root=staging)
                self.assertEqual(launcher.bundle_inventory(app), before)
                stage, = staging.iterdir(); staged_app = stage / app.name
                original_info = (staged_app / launcher.INFO).read_bytes()
                for field, value in [("ExhibitOSValidationProfile", "/foreign-profile"),
                        ("ExhibitOSValidationInputs", dict(pins, unsignedHelper="e" * 64))]:
                    bad = plistlib.loads(original_info); bad[field] = value
                    (staged_app / launcher.INFO).write_bytes(plistlib.dumps(bad))
                    with patch.object(launcher, "clone_publication") as clone:
                        with self.assertRaises(launcher.ValidationError):
                            launcher.prepare(profile, pins, gui, helper, runtime, staging_root=staging, staged_app=staged_app)
                        clone.assert_not_called()
                    (staged_app / launcher.INFO).write_bytes(original_info)
                with patch.object(launcher, "output", side_effect=AssertionError("No reassembly or signing allowed")):
                    launcher.prepare(profile, pins, gui, helper, runtime, staging_root=staging, staged_app=staged_app)
                launcher.verify_bundle(app, profile, pins)
            retained, = root.glob("retained-*.app")
            self.assertEqual(launcher.bundle_inventory(retained), before)
            self.assertEqual((app / launcher.HELPER).read_bytes(), helper.read_bytes())
            stage, = staging.iterdir()
            self.assertEqual(launcher.bundle_inventory(stage / app.name), launcher.bundle_inventory(app))
            refused, = root.glob(".publish-*")
            self.assertEqual(list(refused.iterdir()), [])

    def test_staging_floor_and_redirect_refuse_before_assembly(self):
        for mutation in ("floor", "redirect", "permissions"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                root = Path(temp).resolve(); app, candidate, profile, _ = self.fixture(root)
                before = launcher.bundle_inventory(app)
                gui, helper, runtime = candidate / launcher.GUI, candidate / launcher.HELPER, candidate / launcher.MANIFEST.parent
                pins = dict(unsignedGUI=launcher.sha(gui), unsignedHelper=launcher.sha(helper), runtimeManifest=launcher.sha(runtime / "manifest.json"))
                staging = root / "staging"; staging.mkdir(mode=0o700)
                if mutation == "redirect":
                    link = root / "link"; link.symlink_to(staging); staging = link
                elif mutation == "permissions": staging.chmod(0o755)
                free = 6 * 1024 ** 3 if mutation == "floor" else 20 * 1024 ** 3
                with patch.object(launcher, "APP", app), patch.object(launcher, "pids", return_value=set()), \
                     patch.object(launcher, "launch_services_has_app", return_value=False), \
                     patch.object(launcher, "verify_bundle", side_effect=launcher.ValidationError("old pins")), \
                     patch.object(launcher.shutil, "disk_usage", return_value=shutil._ntuple_diskusage(0, 0, free)), \
                     patch.object(launcher, "output") as run:
                    with self.assertRaises(launcher.ValidationError):
                        launcher.prepare(profile, pins, gui, helper, runtime, staging_root=staging)
                    run.assert_not_called()
                self.assertEqual(launcher.bundle_inventory(app), before)

    def test_reused_stage_refuses_redirect_wrong_parent_and_name(self):
        for mutation in ("redirect", "parent", "name"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                root = Path(temp).resolve(); app, candidate, profile, _ = self.fixture(root)
                before = launcher.bundle_inventory(app)
                gui, helper, runtime = candidate / launcher.GUI, candidate / launcher.HELPER, candidate / launcher.MANIFEST.parent
                pins = dict(unsignedGUI=launcher.sha(gui), unsignedHelper=launcher.sha(helper), runtimeManifest=launcher.sha(runtime / "manifest.json"))
                staging = root / "staging"; staging.mkdir(mode=0o700)
                build = staging / ".build-fixture"; build.mkdir(mode=0o700)
                staged = build / app.name; shutil.copytree(candidate, staged)
                if mutation == "redirect":
                    link = build / "linked.app"; link.symlink_to(staged); staged = link
                elif mutation == "parent": staging = root
                elif mutation == "name": staged = build / "other.app"; staged.mkdir()
                with patch.object(launcher, "APP", app), patch.object(launcher, "pids", return_value=set()), \
                     patch.object(launcher, "launch_services_has_app", return_value=False), \
                     patch.object(launcher, "verify_bundle", side_effect=launcher.ValidationError("old pins")), \
                     patch.object(launcher, "clone_publication") as clone, patch.object(launcher, "output") as run:
                    with self.assertRaises(launcher.ValidationError):
                        launcher.prepare(profile, pins, gui, helper, runtime, staging_root=staging, staged_app=staged)
                    run.assert_not_called(); clone.assert_not_called()
                self.assertEqual(launcher.bundle_inventory(app), before)

    def test_escaping_symlink_refuses_inventory(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); app, _, _, _ = self.fixture(root)
            (app / "escape").symlink_to(root / "outside")
            with self.assertRaisesRegex(launcher.ValidationError, "links must remain"):
                launcher.bundle_inventory(app)

    def test_clonefile_syscall_failure_has_no_copy_fallback(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); source = root / "source"; source.write_bytes(b"same")
            destination = root / "existing"; destination.write_bytes(b"preserve")
            with self.assertRaisesRegex(launcher.ValidationError, "clonefile refused"):
                launcher.clone_file(source, destination)
            self.assertEqual(destination.read_bytes(), b"preserve")


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
            # The signed Native consumer validates graph completeness and keeps
            # non-ready renderer evidence; the CLI must retain that distinction.
            result["furnishings"] = dict(catalog=[dict(kind="light", id=value["placementId"],
                assetId=value["viewpointId"], modelSha256="d" * 64, status="fallback")], workspace=[])
            receipt.write_text(json.dumps(result))
            self.assertEqual(launcher.check_capture_result(value, directory, 77)["furnishings"], result["furnishings"])
            for key, wrong in [("pid", 88), ("etag", "stale"), ("documentUnchanged", False), ("runtimeDrained", False),
                               ("nativeRevision", 8), ("camera", {}), ("candidateSha256", "bad")]:
                broken = dict(result); broken[key] = wrong; receipt.write_text(json.dumps(broken))
                with self.assertRaises(launcher.ValidationError): launcher.check_capture_result(value, directory, 77)
            receipt.write_text(json.dumps(result)); image.write_bytes(png + b"changed")
            with self.assertRaises(launcher.ValidationError): launcher.check_capture_result(value, directory, 77)


if __name__ == "__main__":
    unittest.main()
