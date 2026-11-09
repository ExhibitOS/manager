# Build a separate local Manager app with recipient-built image libraries

Use the matching Manager and Platform source snapshots, build inputs, and
recipient worker verification record. The Platform record identifies the source
commit, producer SHA-256 and complete runtime manifest SHA-256. Verify the
worker with the locally built library before assembling Manager.

This route uses a separate local-validation runtime and a separate private
personal workspace. The runtime manifest uses
`exhibitos-native-runtime-v1`, `mode: "local-validation"`,
`officialSigned: false` and `redistributionQualified: false`.

## Build Manager from the matching source

Work in a separate checkout of the recorded Manager commit and tree. In that
checkout, replace the three values of `CATALOG47_FRESH_IMAGE_RECOVERY` in
`crates/lifecycle/src/native_payload.rs` with the recipient runtime source
commit, producer SHA-256 and manifest SHA-256. Set the same manifest SHA-256
in the debug branch of `supportsFreshImageRuntime` in
`native/Sources/ManagerCore/AuthoringAPI.swift`. Record the resulting local
source tree along with the runtime inventory. The existing 047 capability,
ledger and owner checks still apply.

Use Xcode Swift and the Rust version selected by `rust-toolchain.toml` on an
arm64 Mac. Build the existing validation targets from this checkout:

```sh
swift build --package-path native --product ExhibitOSManager
swift build --package-path native --product exhibitos-automation
cargo build --locked -p exhibitos-lifecycle \
  --features workspace-v5-owner-validation --bin exhibitos-native
```

Use the preview assembler with the built app and helper, the locally built
runtime, the reviewed source resources and a new private validation profile:

```sh
python3 scripts/package-native-preview.py \
  --app "$LOCAL_APP" --swift-executable "$MANAGER_BINARY" \
  --rust-helper "$HELPER_BINARY" --runtime-payload "$RECIPIENT_RUNTIME" \
  --legal-resources "$SOURCE_RESOURCES" --validation-profile "$NEW_PROFILE"
```

Use the [existing private local automation entry](private-agent-interface.md)
with the bundled helper, the same fresh profile, private output and temporary
directories, and an unused port triple to initialize the owned workspace.
Close that owner session after verifying preparation. The preview is headless.
For GUI validation, use the existing fixed `scripts/manager-validation-gui.py`
entry with the reviewed source build, runtime, profile, and approved signature.
The fixed launcher must verify the installed bundle before launch. Verify Help
from that approved instance when its source resources have been prepared. Keep
the source checkout, runtime inventory, and verification results together.
