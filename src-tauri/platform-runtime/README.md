# Platform runtime application resources

This directory is deliberately unprepared in a source checkout. No invented download URL or automatic source build is used on a user's Mac.

An operator first produces and qualifies an architecture-matching bundle with Platform's existing `scripts/package-local-runtime.mjs`. Then run:

```
node scripts/prepare-platform-runtime.mjs EXISTING_BUNDLE arm64 EXACT_PLATFORM_COMMIT
```

Use `x86_64` for an independently qualified Intel bundle. The operator must verify the image architecture and bind the supplied commit to the producer's actual source evidence; the preparer does not infer those facts from a Docker archive. The generated directory is not committed. Only the manifest, Compose and declared image archives are copied, never runtime.env, keys, DB files or user works. Existing output is refused.

The application stages these fixed resources in a new selected installation before calling the existing lifecycle install. Existing installations and bundles are preserved. Each new installation receives its own existing-format bundle/project identity and Compose labels; a small local binding retains that identity across retries, without changing original image bytes. Only the producer’s JSON-as-YAML Compose output is accepted. PostgreSQL without a declared archive uses the existing digest-pinned first-install pull; offline first installation is not claimed. Missing prepared resources are reported as unavailable, not as successful setup.

Checksums bind bytes; they do not authenticate the publisher. Production resources must ship inside the Developer ID signed/notarized Manager application. A local source build remains a development path and is not production authenticity proof. This does not change the existing signed-update trust policy or add a trust-on-first-use mechanism.

## Scoped verification — 2026-10-17

On macOS arm64 with Node 24.21.0 and Rust 1.99.0, the operator packager's four small-file Node tests passed. Seven lifecycle tests passed for complete preparation, existing bundle/data preservation, exact interrupted-prefix continuation, changed-prefix refusal, unavailable resources and distinct installation project/volume labels. The desktop crate passed `cargo check --offline --locked --jobs 1`; no native application was executed by that check. Source byte hashes, commands, inputs, observed results and limits are preserved in [the scoped verification record](../../docs/runtime-preparation-verification-2026-10-17.json).

These are synthetic-file and compile results, not a completed first installation. No real Platform image archive was prepared/copied, no image was pulled, and no user root or key was touched. Native GUI setup/start/readiness, interrupted application relaunch and production Developer ID/notarization remain unverified. Supplied architecture/source commit metadata requires independent producer qualification; the packager does not authenticate or infer it. At that synthetic-check checkpoint the generated resource was absent; the later real-artifact preparation below does not retroactively turn those tests into native installation evidence.

## Actual development artifact preparation

Later on 2026-10-17, the unchanged clean Platform source `99ae38d44106a276f2675f4118a7ea4d498d4e9a` produced a real `linux/arm64` bundle through its existing local-runtime producer. Docker inspection confirmed the exact image ID and architecture. The 97,028,096-byte Platform archive, Compose and manifest hashes matched their declarations. This preparer then copied the three declared payload files into the ignored `generated/` resource directory. No registry publication or lifecycle installation/start was performed by that artifact step. The database image remains digest-pinned for first-install acquisition. This is a local development artifact, not a signed/notarized public distribution or a completed native setup. Exact identity and file hashes are in the same verification record.
