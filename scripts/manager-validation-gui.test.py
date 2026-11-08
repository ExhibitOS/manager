# SPDX-License-Identifier: Apache-2.0
import importlib.util
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("launcher", Path(__file__).with_name("manager-validation-gui.py"))
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)


class ManagerValidationGuiTests(unittest.TestCase):
    def test_rejects_per_run_path_before_checking_bundle_or_profile(self):
        with self.assertRaisesRegex(launcher.ValidationError, "restricted to"):
            launcher.check_fixed_path(Path("/private/tmp/Another Manager.app"))

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
        with patch.object(launcher, "pids", return_value={1234}), patch.object(launcher, "output") as run:
            with self.assertRaisesRegex(launcher.ValidationError, "already open"):
                launcher.launch()
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
