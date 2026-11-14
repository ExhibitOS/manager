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
import shutil
import stat
import struct
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
LEGAL = Path("Contents/Resources/NativeRuntime-Legal")
LEGAL_INVENTORY = Path("Contents/Resources/NativeRuntime-Legal-Inventory.json")
MAX_LEGAL_INVENTORY_BYTES = 2 * 1024 ** 2
MAX_LEGAL_FILES = 4096
MAX_LEGAL_BYTES = 1024 ** 3
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


def read_legal_inventory(path, pin):
    require(path.is_absolute() and path.resolve(strict=True) == path and path.is_file(),
            "Supply a canonical regular legal inventory file.")
    with path.open("rb") as file:
        data = file.read(MAX_LEGAL_INVENTORY_BYTES + 1)
    require(len(data) <= MAX_LEGAL_INVENTORY_BYTES and hashlib.sha256(data).hexdigest() == pin,
            "Legal inventory size or SHA-256 does not match the reviewed input.")
    inventory = json.loads(data)
    rows = inventory.get("files") if isinstance(inventory, dict) else None
    require(isinstance(rows, list) and 0 < len(rows) <= MAX_LEGAL_FILES,
            "Legal inventory must contain a bounded file list.")
    files = {}
    total = 0
    for row in rows:
        require(isinstance(row, dict) and set(row) == {"path", "bytes", "sha256"}
                and isinstance(row["path"], str), "Invalid legal inventory entry.")
        name = Path(row["path"])
        require(name.parts and not name.is_absolute() and ".." not in name.parts
                and name.as_posix() == row["path"] and name not in files
                and type(row["bytes"]) is int and 0 <= row["bytes"] <= MAX_LEGAL_BYTES
                and isinstance(row["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", row["sha256"]),
                "Invalid or duplicate legal inventory entry.")
        files[name] = (row["bytes"], row["sha256"])
        total += row["bytes"]
    require(Path("index.html") in files and total <= MAX_LEGAL_BYTES,
            "Legal inventory must include index.html and fit within 1 GiB.")
    return data, files


def verify_legal_resources(root, files, *, inventory=None):
    require(root.is_absolute() and root.resolve(strict=True) == root and root.is_dir(),
            "Supply a canonical legal resource directory.")
    actual = set()
    for path in root.rglob("*"):
        require(not path.is_symlink(), "Links are not allowed in the legal resource tree.")
        if path == inventory:
            continue
        if path.is_dir():
            continue
        name = path.relative_to(root)
        expected = files.get(name)
        require(expected is not None and path.is_file()
                and path.stat().st_size == expected[0] and sha(path) == expected[1],
                f"Legal resource differs from the reviewed inventory: {name}")
        actual.add(name)
    require(actual == files.keys(), "The legal resource tree is missing reviewed files.")


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
    legal, inventory = bundle / LEGAL, bundle / LEGAL_INVENTORY
    if "legalInventory" in pins:
        _, files = read_legal_inventory(inventory, pins["legalInventory"])
        verify_legal_resources(legal, files)
    else:
        require(not any(path.exists() or path.is_symlink() for path in (legal, inventory)),
                "The validation app contains unpinned legal resources.")


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


def prepare(profile, pins, gui, helper, runtime, legal=None, inventory=None, scene_capture_cli=False):
    pinned_legal = "legalInventory" in pins
    require((legal is not None) == pinned_legal and (inventory is not None) == pinned_legal,
            "Legal resources, inventory and its reviewed SHA-256 must be supplied together.")
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
            existing = plistlib.loads((APP / INFO).read_bytes())
            if not scene_capture_cli or existing.get("ExhibitOSValidationSceneCaptureCLI") == pins["unsignedGUI"]:
                return
    require(all(path.is_absolute() and path.resolve(strict=True) == path for path in (gui, helper, runtime)),
            "Supply the canonical paths of the reviewed GUI executable, helper and runtime.")
    require(sha(gui) == pins["unsignedGUI"] and sha(helper) == pins["unsignedHelper"]
            and sha(runtime / "manifest.json") == pins["runtimeManifest"],
            "Input hashes differ from the reviewed build or sealed runtime. Supply the matching artifacts.")
    if legal is not None:
        inventory_bytes, legal_files = read_legal_inventory(inventory, pins["legalInventory"])
        verify_legal_resources(legal, legal_files, inventory=inventory)
    producer = Path(__file__).resolve().with_name("package-native-preview.py")
    with tempfile.TemporaryDirectory(prefix=".build-", dir=APP.parent) as temporary:
        scratch = Path(temporary)
        candidate = scratch / APP.name
        output("/usr/bin/python3", "-I", "-B", str(producer), "--app", str(candidate),
               "--swift-executable", str(gui), "--rust-helper", str(helper),
               "--validation-profile", str(scratch / "uncreated-profile"), "--runtime-payload", str(runtime))
        if legal is not None:
            for name in legal_files:
                target = candidate / LEGAL / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(legal / name, target, follow_symlinks=False)
            verify_legal_resources(candidate / LEGAL, legal_files)
            (candidate / LEGAL_INVENTORY).write_bytes(inventory_bytes)
        info_path = candidate / INFO
        info = plistlib.loads(info_path.read_bytes())
        info.update(CFBundleIdentifier=BUNDLE_ID, CFBundleName="ExhibitOS Manager Validation",
                    CFBundleDisplayName="ExhibitOS Manager Validation", ExhibitOSValidationProfile=str(profile),
                    CFBundleExecutable=GUI.name, ExhibitOSValidationInputs=pins)
        if scene_capture_cli:
            info["ExhibitOSValidationSceneCaptureCLI"] = pins["unsignedGUI"]
        info_path.write_bytes(plistlib.dumps(info))
        require(sha(candidate / GUI) == pins["unsignedGUI"] and sha(candidate / HELPER) == pins["unsignedHelper"],
                "Assembled executable hashes differ from the reviewed build.")
        (candidate / GUI).chmod(0o755)
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


def preflight(bundle, profile, pins):
    check_fixed_path(bundle)
    require(profile.is_absolute() and profile.resolve(strict=True) == profile and profile.is_dir(),
            "The signed validation profile path is missing or redirected.")
    verify_bundle(bundle, profile, pins)


def launch(bundle, profile, pins, capture_request=None):
    preflight(bundle, profile, pins)
    require(not pids() and not launch_services_has_app(),
            "The validation app is already open. Verify and attach to its owned PID instead of opening it again.")
    if capture_request is None:
        output("/usr/bin/open", "-a", str(bundle))
    else:
        output("/usr/bin/open", "-a", str(bundle), "--args", "--native-scene-capture-request", str(capture_request))
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        running = pids()
        if running and launch_services_has_app():
            require(len(running) == 1, "Multiple validation processes are running. Quit them normally before retrying.")
            return next(iter(running))
        time.sleep(.25)
    raise ValidationError("The fixed validation process did not register. Inspect the current app before retrying.")


CAPTURE_KEYS = frozenset(("version", "requestId", "workspaceId", "tenantId", "principalId", "runtimeManifestSHA256",
    "expectedNativeRevision", "expectedEditVersion", "exhibitionId", "serverRevision", "expectedETag",
    "placementId", "viewpointId", "outputDirectory"))


def capture_request(path, profile, pins):
    require(path.is_absolute() and path.resolve(strict=True) == path, "Use a canonical owned capture request file.")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        item = os.fstat(stream.fileno())
        require(stat.S_ISREG(item.st_mode) and item.st_uid == os.getuid() and item.st_nlink == 1
                and item.st_mode & 0o077 == 0 and 0 < item.st_size <= 65536, "Capture request must be a small private owned regular file.")
        value = json.load(stream)
    require(type(value) is dict and set(value) == CAPTURE_KEYS and type(value["version"]) is int and value["version"] == 1,
            "Capture request has an unsupported shape or version.")
    for key in ("requestId", "workspaceId", "tenantId", "principalId", "exhibitionId", "placementId", "viewpointId"):
        require(type(value[key]) is str and re.fullmatch(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}", value[key]),
                "Capture request identifiers must be UUIDs.")
    require(type(value["expectedNativeRevision"]) is int and 0 < value["expectedNativeRevision"] <= 2**63 - 1
            and type(value["expectedEditVersion"]) is int and 0 < value["expectedEditVersion"] <= 2**63 - 1
            and type(value["serverRevision"]) is int and 0 < value["serverRevision"] <= 2**31 - 1,
            "Capture revisions must be positive integers in the supported range.")
    require(value["runtimeManifestSHA256"] == pins["runtimeManifest"] and type(value["expectedETag"]) is str
            and re.fullmatch(r'"studio-r' + str(value["serverRevision"]) + r'-[0-9a-f]{64}"', value["expectedETag"]),
            "Capture manifest or saved revision/ETag differs from the admitted request.")
    require(type(value["outputDirectory"]) is str, "Capture output must be a private directory.")
    directory = Path(value["outputDirectory"])
    require(directory.is_absolute() and str(directory) == value["outputDirectory"] and directory.resolve(strict=True) == directory and directory.is_dir(),
            "Use a canonical existing capture output directory.")
    info = directory.stat()
    require(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700,
            "Capture output directory must be private and owned.")
    for protected in (profile.resolve(strict=True), APP.resolve(strict=True)):
        require(directory != protected and protected not in directory.parents, "Capture output must be outside the profile and App.")
    require(not any((directory / name).exists() or (directory / name).is_symlink() for name in ("scene.png", "capture.json", "launch.json")),
            "Capture output already contains a result; do not overwrite or retry it.")
    return value, directory


def check_capture_result(value, directory, pid):
    path = directory / "capture.json"
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1
            and info.st_size <= 192 * 1024 and info.st_mode & 0o077 == 0, "Capture receipt is not a private owned result.")
    result = json.loads(path.read_bytes())
    require(type(result) is dict, "Capture receipt has an unsupported shape.")
    require(type(result.get("version")) is int and result.get("version") == 1 and result.get("requestId") == value["requestId"] and result.get("pid") == pid
            and result.get("bundleId") == BUNDLE_ID, "Capture receipt is not bound to the owned App invocation.")
    require(result.get("status") == "succeeded", "Native scene capture failed: " + str(result.get("errorCode", "NATIVE_SCENE_CAPTURE_FAILED")))
    bindings = {key: value[key] for key in ("workspaceId", "tenantId", "principalId", "runtimeManifestSHA256", "exhibitionId", "serverRevision", "placementId", "viewpointId")}
    bindings.update(nativeRevision=value["expectedNativeRevision"], editVersion=value["expectedEditVersion"], etag=value["expectedETag"])
    require(all(result.get(key) == expected for key, expected in bindings.items())
            and all(result.get(key) is True for key in ("documentUnchanged", "runtimeDrained", "helperClosed")),
            "Capture scope or normal preservation/cleanup proof does not match.")
    png = directory / "scene.png"
    item = png.lstat()
    require(stat.S_ISREG(item.st_mode) and item.st_uid == os.getuid() and item.st_nlink == 1
            and item.st_mode & 0o077 == 0 and 0 < item.st_size <= 512 * 1024, "Capture PNG is not a bounded private owned file.")
    body = png.read_bytes()
    require(len(body) >= 24 and body[:8] == b"\x89PNG\r\n\x1a\n" and body[12:16] == b"IHDR"
            and struct.unpack(">II", body[16:24]) == (640, 360), "Capture PNG dimensions or signature differ from the protocol.")
    require(type(result.get("candidateSha256")) is str and re.fullmatch(r"[0-9a-f]{64}", result["candidateSha256"]), "Capture scene digest is invalid.")
    expected_png = dict(file="scene.png", bytes=len(body), sha256=hashlib.sha256(body).hexdigest(), width=640, height=360)
    require(type(result.get("camera")) is dict and type(result.get("target")) is dict, "Capture camera/target metadata has an unsupported shape.")
    require(result.get("png") == expected_png and result.get("camera", {}).get("viewpointId") == value["viewpointId"]
            and result.get("target", {}).get("placementId") == value["placementId"], "Capture image or camera/placement binding failed integrity checks.")
    return result


def capture(bundle, profile, pins, path):
    preflight(bundle, profile, pins)
    info = plistlib.loads((bundle / INFO).read_bytes())
    require(info.get("ExhibitOSValidationSceneCaptureCLI") == pins["unsignedGUI"],
            "This reviewed App has not been admitted for scene-capture CLI; prepare the qualified artifact explicitly.")
    value, directory = capture_request(path, profile, pins)
    pid = launch(bundle, profile, pins, capture_request=path)
    started = dict(app=str(bundle), bundleId=BUNDLE_ID, profile=str(profile), pins=pins, pid=pid, requestId=value["requestId"])
    fd = os.open(directory / "launch.json", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "w") as stream:
        json.dump(started, stream, indent=2); stream.write("\n"); stream.flush(); os.fsync(stream.fileno())
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        running = pids()
        require(not running or running == {pid}, "Capture App identity changed; preserve the current process for owner inspection.")
        if not running and not launch_services_has_app():
            require((directory / "capture.json").exists(), "Capture App exited without a receipt. Do not blindly relaunch.")
            return {**started, "capture": check_capture_result(value, directory, pid), "normalAppExit": True}
        time.sleep(.25)
    raise ValidationError("Capture is still active; preserve the owned App and resources for inspection. No timeout kill was issued.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "verify", "launch", "attach", "capture"))
    parser.add_argument("--app", type=Path, default=APP)
    parser.add_argument("--profile", type=Path, required=True)
    for option in ("gui-sha", "helper-sha", "runtime-manifest-sha"):
        parser.add_argument("--" + option, required=True)
    for option in ("gui", "helper", "runtime"):
        parser.add_argument("--" + option, type=Path)
    parser.add_argument("--legal-resources", type=Path)
    parser.add_argument("--legal-inventory", type=Path)
    parser.add_argument("--legal-inventory-sha")
    parser.add_argument("--pid", type=int)
    parser.add_argument("--request", type=Path)
    parser.add_argument("--scene-capture-cli", action="store_true")
    args = parser.parse_args()
    try:
        require(not args.scene_capture_cli or args.action == "prepare", "--scene-capture-cli is a reviewed prepare-only admission.")
        require((args.request is not None) == (args.action == "capture"), "Only capture requires --request.")
        check_fixed_path(args.app)
        pins = dict(unsignedGUI=args.gui_sha, unsignedHelper=args.helper_sha, runtimeManifest=args.runtime_manifest_sha)
        require(all(re.fullmatch(r"[0-9a-f]{64}", pin) for pin in pins.values()),
                "Provide the three reviewed SHA-256 build and runtime pins.")
        legal_paths = (args.legal_resources, args.legal_inventory)
        if args.legal_inventory_sha is not None:
            require(re.fullmatch(r"[0-9a-f]{64}", args.legal_inventory_sha), "Provide a reviewed legal inventory SHA-256.")
            pins["legalInventory"] = args.legal_inventory_sha
        if args.action == "prepare":
            legal_inputs = (*legal_paths, args.legal_inventory_sha)
            require(all(legal_inputs) or not any(legal_inputs),
                    "For prepare, supply --legal-resources, --legal-inventory and --legal-inventory-sha together.")
        else:
            require(not any(legal_paths), "For verify, launch or attach, supply only the legal inventory SHA-256.")
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
                prepare(args.profile, pins, args.gui, args.helper, args.runtime, *legal_paths, scene_capture_cli=args.scene_capture_cli)
            result = dict(app=str(APP), bundleId=BUNDLE_ID, profile=str(args.profile), pins=pins)
            if args.action == "capture":
                result = capture(args.app, args.profile, pins, args.request)
            elif args.action == "launch":
                result["pid"] = launch(args.app, args.profile, pins)
            else:
                preflight(args.app, args.profile, pins)
            if args.action == "attach":
                require(pids() == {args.pid} and launch_services_has_app(),
                        "The single owned validation PID is no longer registered. Inspect the app; do not launch through CUA.")
                result["pid"] = args.pid
        print(json.dumps(result))
    except (ValidationError, OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Manager validation GUI rejected: {error}\n")


if __name__ == "__main__":
    main()
