#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Prepare and launch the approved, fixed local Manager validation app."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import tempfile
import time
import uuid

APP = Path("/Volumes/ExhibitSSD/ExhibitOS-Backups/manager-validation-stable/ExhibitOS Manager Validation.app")
BUNDLE_ID = "org.exhibitos.manager.validation"
CERT = "F5F459A256A6159A159F7E536963FFE7DDA5C0F0"
TEAM = "W563ZQD9G6"
SIGNER = "Apple Development: Jongho Jung (ANCRBTJGB7)"
GUI = Path("Contents/MacOS/ExhibitOSManager")
HELPER = Path("Contents/Helpers/exhibitos-native")
INFO = Path("Contents/Info.plist")
MANIFEST = Path("Contents/Resources/native-runtime/manifest.json")
CODESIGN = "/usr/bin/codesign"
REQUIREMENT = (
    'identifier "{identifier}" and anchor apple generic and '
    'certificate leaf[subject.CN] = "' + SIGNER + '" and '
    'certificate 1[field.1.2.840.113635.100.6.2.1] /* exists */'
)


class ValidationError(RuntimeError):
    pass


def require(condition, message):
    if not condition:
        raise ValidationError(message)


def sha(path):
    with path.open("rb") as file:
        digest = hashlib.sha256()
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
        return digest.hexdigest()


def output(*argv):
    return subprocess.check_output(argv, text=True, stderr=subprocess.STDOUT)


def verify_signature(path, identifier):
    expected = "designated => " + REQUIREMENT.format(identifier=identifier)
    details = output(CODESIGN, "--display", "--verbose=4", str(path)).splitlines()
    require(f"Authority={SIGNER}" in details and f"TeamIdentifier={TEAM}" in details
            and "Signature=adhoc" not in details,
            f"Unexpected signer for {path}. Prepare the app with the approved Apple Development identity.")
    designated = output(CODESIGN, "--display", "--requirements", "-", str(path)).splitlines()
    require([line for line in designated if line.startswith("designated =>")] == [expected],
            f"Designated requirement mismatch for {path}. Restore the approved fixed validation signature.")
    try:
        output(CODESIGN, "--verify", "--strict", "--test-requirement",
               f'=identifier "{identifier}" and certificate leaf = H"{CERT}"', str(path))
    except subprocess.CalledProcessError:
        raise ValidationError(
            f"Code or sealed resources failed verification for {path}. Re-prepare the approved fixed bundle."
        ) from None


def verify_bundle(bundle, profile, pins):
    require(bundle.is_dir() and bundle.resolve() == bundle,
            f"Missing or redirected validation bundle: {bundle}. Prepare the approved fixed bundle first.")
    info = plistlib.loads((bundle / INFO).read_bytes())
    require(info.get("CFBundleIdentifier") == BUNDLE_ID
            and info.get("CFBundleExecutable") == GUI.name,
            f"Unexpected bundle identifier or executable. Prepare {BUNDLE_ID} with this launcher.")
    require(set((bundle / HELPER.parent).iterdir()) == {bundle / HELPER}
            and (bundle / HELPER).is_file() and not (bundle / HELPER).is_symlink(),
            "Unexpected validation helper inventory. Prepare the fixed bundle with the reviewed native helper.")
    require(all((bundle / path).is_file() and (bundle / path).resolve() == bundle / path
                for path in (GUI, MANIFEST)),
            "The validation executable or runtime manifest is missing or redirected. Re-prepare the fixed bundle.")
    verify_signature(bundle, BUNDLE_ID)
    verify_signature(bundle / HELPER, HELPER.name)
    require(info.get("ExhibitOSValidationInputs") == pins and sha(bundle / MANIFEST) == pins["runtimeManifest"],
            "Validation build or runtime pins differ. Prepare the fixed bundle with the reviewed artifacts.")
    require(info.get("ExhibitOSValidationProfile") == str(profile),
            "The signed bundle is bound to a different validation profile. Prepare it with the intended profile.")


def check_fixed_path(path):
    require(path == APP, f"Validation GUI launch is restricted to {APP}. Use the fixed bundle, not {path}.")
    require(path.parent.resolve() == path.parent,
            f"Validation bundle parent is redirected: {path.parent}. Restore the fixed directory.")


def pids():
    rows = (line.strip().split(maxsplit=1) for line in output("/bin/ps", "-axo", "pid=,comm=").splitlines())
    return {int(row[0]) for row in rows if len(row) == 2 and row[1] == str(APP / GUI)}


def launch_services_has_app():
    result = subprocess.run(("/usr/bin/lsappinfo", "find", f"bundleID={BUNDLE_ID}"),
                            text=True, capture_output=True, check=False)
    return result.returncode == 0 and bool(result.stdout.strip())


def prepare(profile, pins, gui, helper, runtime):
    require(not pids() and not launch_services_has_app(),
            "The validation app is already open. Stop its owned work and quit normally before preparing it.")
    if APP.exists() or APP.is_symlink():
        require(APP.resolve() == APP,
                "The fixed validation bundle is redirected. Restore the approved directory before preparing it.")
        try:
            verify_bundle(APP, profile, pins)
        except (ValidationError, OSError, ValueError, subprocess.CalledProcessError):
            pass
        else:
            return
    require(all(path.is_absolute() and path.resolve(strict=True) == path for path in (gui, helper, runtime)),
            "Supply the canonical paths of the reviewed GUI executable, helper and runtime.")
    require(sha(gui) == pins["unsignedGUI"] and sha(helper) == pins["unsignedHelper"]
            and sha(runtime / "manifest.json") == pins["runtimeManifest"],
            "Input hashes differ from the reviewed build or sealed runtime. Supply the matching artifacts.")
    producer = Path(__file__).resolve().with_name("package-native-preview.py")
    with tempfile.TemporaryDirectory(prefix=".build-", dir=APP.parent) as temporary:
        scratch = Path(temporary)
        candidate = scratch / APP.name
        output("/usr/bin/python3", "-I", "-B", str(producer), "--app", str(candidate),
               "--swift-executable", str(gui), "--rust-helper", str(helper),
               "--validation-profile", str(scratch / "uncreated-profile"), "--runtime-payload", str(runtime))
        info_path = candidate / INFO
        info = plistlib.loads(info_path.read_bytes())
        info.update(CFBundleIdentifier=BUNDLE_ID, CFBundleName="ExhibitOS Manager Validation",
                    CFBundleDisplayName="ExhibitOS Manager Validation", ExhibitOSValidationProfile=str(profile),
                    ExhibitOSValidationInputs=pins)
        info_path.write_bytes(plistlib.dumps(info))
        require(sha(candidate / GUI) == pins["unsignedGUI"] and sha(candidate / HELPER) == pins["unsignedHelper"],
                "Assembled executable hashes differ from the reviewed build.")
        output(CODESIGN, "--force", "--sign", CERT, str(candidate / HELPER))
        output(CODESIGN, "--force", "--sign", CERT, str(candidate))
        verify_bundle(candidate, profile, pins)
        require(not pids() and not launch_services_has_app(),
                "The validation app opened during preparation. Quit it normally before installing the signed bundle.")
        previous = None
        if APP.exists():
            previous = APP.parent / ("retained-" + str(uuid.uuid4()) + ".app")
            APP.rename(previous)
        try:
            candidate.rename(APP)
        except OSError:
            if previous is not None and not APP.exists():
                previous.rename(APP)
            raise


def launch():
    require(not pids() and not launch_services_has_app(),
            "The validation app is already open. Verify and attach to its owned PID instead of opening it again.")
    output("/usr/bin/open", "-a", str(APP))
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        running = pids()
        if running and launch_services_has_app():
            require(len(running) == 1, "Multiple validation processes are running. Quit them normally before retrying.")
            return next(iter(running))
        time.sleep(.25)
    raise ValidationError("The fixed validation process did not register. Inspect the current app before retrying.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "verify", "launch", "attach"))
    parser.add_argument("--app", type=Path, default=APP)
    parser.add_argument("--profile", type=Path, required=True)
    for option in ("gui-sha", "helper-sha", "runtime-manifest-sha"):
        parser.add_argument("--" + option, required=True)
    for option in ("gui", "helper", "runtime"):
        parser.add_argument("--" + option, type=Path)
    parser.add_argument("--pid", type=int)
    args = parser.parse_args()
    try:
        check_fixed_path(args.app)
        pins = dict(unsignedGUI=args.gui_sha, unsignedHelper=args.helper_sha, runtimeManifest=args.runtime_manifest_sha)
        require(all(re.fullmatch(r"[0-9a-f]{64}", pin) for pin in pins.values()),
                "Provide the three reviewed SHA-256 build and runtime pins.")
        require(args.profile.is_absolute(), "Supply the absolute validation profile path.")
        require(args.action != "prepare" or all((args.gui, args.helper, args.runtime)),
                "Prepare requires --gui, --helper and --runtime paths.")
        require(args.action != "attach" or (args.pid is not None and args.pid > 0),
                "Attach requires the owned PID returned by the launch command.")
        if args.action == "prepare":
            APP.parent.mkdir(mode=0o700, exist_ok=True)
        lock_fd = os.open(APP.parent / ".bundle.lock", os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        with os.fdopen(lock_fd, "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            if args.action == "prepare":
                require(args.profile.resolve(strict=True) == args.profile and args.profile.is_dir(),
                        "Use the canonical existing validation profile.")
                prepare(args.profile, pins, args.gui, args.helper, args.runtime)
            verify_bundle(APP, args.profile, pins)
            require(args.profile.resolve(strict=True) == args.profile and args.profile.is_dir(),
                    "The signed validation profile path is missing or redirected.")
            result = dict(app=str(APP), bundleId=BUNDLE_ID, profile=str(args.profile), pins=pins)
            if args.action == "launch":
                result["pid"] = launch()
            if args.action == "attach":
                require(pids() == {args.pid} and launch_services_has_app(),
                        "The single owned validation PID is no longer registered. Inspect the app; do not launch through CUA.")
                result["pid"] = args.pid
        print(json.dumps(result))
    except (ValidationError, OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Manager validation GUI rejected: {error}\n")


if __name__ == "__main__":
    main()
