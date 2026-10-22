# Manager release SBOM

The release deliverable includes a dependency SBOM, source/build provenance and
review of their bindings to the actual artifact. A lockfile, successful scanner
exit or empty BOM alone does not establish those bindings. Coverage and the scope
of each observation must be reported accurately; a perfect all-shipped dependency
graph is not a separate release acceptance gate. These development results do not qualify production signing,
notarization, a vulnerability assessment or another build.

## Observed development artifact

On 2026-10-12, Trivy 0.75.0 scanned the reviewed Apple Development-signed macOS
arm64 Manager App built from `e1581e5558b2dc8aee2e1fa93204fbf733009f49`.
The scanner procedure ran from the documentation/tool-only pipeline commit
`8283d444d09298f67369697facd7e3102bb8460a`. The App contained 115 files,
166,005,082 bytes; its reviewed inventory SHA256 was
`5ff4c524386464b7ccbf2a3c471591585fc36dd25f12c1dda87052244e746852`.
The complete file inventory and all six source manifest inputs were unchanged
after the two offline scans. No rebuild, download, app launch, sensor access or
existing-data mutation occurred during the scans.

| Observation | Actual result | Scope |
| --- | --- | --- |
| Delivered App scan | CycloneDX 1.7; scanner exit 0; **0 dependency components detected** | Artifact SBOM coverage is incomplete. This is not a complete-SBOM PASS. |
| Separate source manifest scan | CycloneDX 1.7; scanner exit 0; 455 components: 448 Cargo packages, 4 npm packages, 3 file/root components | The source dependency graph, including target/build-related Cargo lock entries, is not proof that all packages are linked or delivered. |
| Bundled Node observation | Actual embedded executable reports Node 24.21.0 and embedded component versions | Separate runtime evidence, not packages inferred from the source lock scan. |

App BOM SHA256:
`542094b7f6fc7ed86b329d36f757a60a5dc8be122506c78c36a5563792a6bf94`.
Source dependency BOM SHA256:
`97704e8a74a24b9f06c2d47a70fb7d5ddbfd39930ad69107f770e82bb69f2788`.
The source npm components are `@tauri-apps/api` 2.12.1, `react` 19.3.0,
`react-dom` 19.3.0 and `scheduler` 0.28.0. Minification and the Rust-linked
frontend do not preserve installed npm manifests in this App. The native Rust
binary was not built with embedded cargo-auditable dependency metadata.

The packaged Node manifest and Node executable hash bind the existing official
Node archive. The observed executable reports, among others, V8
13.6.233.17-node.53, OpenSSL 3.5.8, libuv 1.52.1, ICU 78.3, SQLite 3.53.4,
c-ares 1.34.8, undici 7.29.1, simdjson 4.6.7 and zstd 1.5.7.
`modules` and `napi` are ABI versions; `cldr`, `tz` and `unicode` describe data
versions. Empty `nghttp3`/`ngtcp2` fields do not assert installed packages.
The delivered `NODE-LICENSE` also includes notices for source/test/build
components, so every notice cannot be presented as an installed dependency.
The bundled public Deployment source and its license remain separately pinned
by `vendor/deployment/source.json` and the packaged remote manifest.

## Reproduce without a product build

Use the reviewed, checksum-verified Trivy executable and fresh private output
and cache directories. `APP` must identify the actual artifact; do not substitute
a checkout, `node_modules`, unrelated source or another App. Preserve the
existing source-to-artifact build receipt, source commit and complete before/after
file inventory. Confirm the tool version and checksum before execution.

```sh
"$TRIVY" filesystem \
  --format cyclonedx --output "$OUTPUT/artifact.cdx.json" \
  --offline-scan --skip-db-update --skip-java-db-update \
  --skip-check-update --skip-vex-repo-update --skip-version-check \
  --disable-telemetry --parallel 1 --timeout 90s \
  --cache-dir "$FRESH_CACHE" "$APP"
```

Do not add `--scanners vuln` to this SBOM-only command: that requests a different
operation requiring an actual vulnerability database. Use a clean `HOME` and
`TMPDIR` to avoid inherited scanner configuration or credentials. Admit the
combined workspace storage requirement before running; the observed execution
reserved 256MiB for new outputs/staging, 192MiB for other work and a 6GiB free
space floor, with a 120-second outer deadline for each serial scan.

For separate source dependency evidence, stage only the reviewed build commit's
`Cargo.lock`, root `Cargo.toml`, `crates/lifecycle/Cargo.toml`,
`src-tauri/Cargo.toml`, `package.json` and `package-lock.json`, preserving their
relative paths. Verify each staged file against that commit. Run the same
command against this staging directory with a distinct output name. Keep its
result separate from the artifact BOM; do not merge source-only packages into
an artifact BOM while asserting installed membership.

## Remaining release coverage

The empty App scan records incomplete scanner coverage of compiled dependencies.
The source BOM and observed bundled runtime versions remain useful, separately
scoped evidence. Build-produced metadata and inventories can improve their link
to the selected artifact without claiming that every linked symbol or shipped
byte has been attributed to a dependency. Rust build dependencies, frontend
rendered modules, bundled packages and OS libraries need distinct coverage
statements. This does not add a standalone graph-completeness or signing gate.

Authoritative tool coverage:
[Trivy Rust package and auditable binary coverage](https://trivy.dev/docs/latest/coverage/language/rust/),
[Trivy filesystem scanning](https://trivy.dev/docs/latest/target/filesystem/),
[Trivy SBOM generation](https://trivy.dev/docs/latest/supply-chain/sbom/).


## Pinned Python notice supplement

The update-runtime selector preserves the original `20261013` CPython archive
pin and `PYTHON.json`. That archive declares `licenses/LICENSE.zlib-ng.txt` for
its zlib extension but omits that file. The upstream metadata generator matches
both zlib and zlib-ng by library name `z`; the selected metadata describes a
system link. This is not evidence that a zlib-ng implementation is bundled.

The generator now supplies only that exact missing notice from
`python/cpython-source-deps`, tag `zlib-ng-2.2.4`, commit
`a502e047655c31db29601def76ad6539acacf43d`. Its unmodified `LICENSE.md` is
867 bytes, SHA256
`6c9f0d975b41afaa34d22f55bb8986ce69e5cb7ad327cb2b28820cd425edf5ee`.
The source descriptor and verbatim copyright/terms are retained under
`scripts/update-runtime-notices/`. No generator network fetch is performed.

Before writing a fresh runtime, the generator checks the exact descriptor and
notice bytes, rejects any change to the archive's unique missing-license set,
and checks every declared license path against its selected `notices/` file.
Existing notice bytes and original metadata remain unchanged. The discrepancy
record distinguishes the original omission from this explicit supplement and
records the path, origin, byte count and SHA256 of each declared notice.
Unknown missing paths, empty or changed notices and altered metadata are refused.

The originally generated runtime remains `private-validation-only`; its recorded
results and sealed Apps are not rewritten by this source change. The notice
supplement alone does not establish linked dependency membership, an exhaustive
artifact SBOM, signing or notarization. The fixed distribution qualification
below is a separate producer/consumer contract. Tiny
synthetic tar/selection regressions exercise the mapping and refusal boundaries;
no full archive extraction or Python execution is required for those checks.

Upstream evidence:
[release dependency metadata](https://github.com/astral-sh/python-build-standalone/blob/20261013/pythonbuild/downloads.json),
[license annotation generator](https://github.com/astral-sh/python-build-standalone/blob/20261013/pythonbuild/utils.py#L513-L549),
[exact supplemental notice](https://github.com/python/cpython-source-deps/blob/a502e047655c31db29601def76ad6539acacf43d/LICENSE.md).


## Frontend emitted-module provenance

The Vite/Rolldown build hook emits `frontend-provenance.json` beside the actual
frontend outputs. It records chunk byte hashes, static/dynamic chunk imports,
positive rendered-length modules, exact npm package/version and installed
package.json/lock-entry hashes, and safe relative local-source hashes. Build-tool
bindings and recognized generated helpers are separate. Zero-rendered modules
are counted without claiming delivered membership. CSS and other assets have
separate byte inventories; their source package membership is not inferred.

Unknown npm ownership, unsupported virtual/query identifiers, external imports
and unsafe or inconsistent inputs refuse this instrumentation rather than emit
misleading provenance. The deterministic bounded report excludes its own bytes.
A final write hook verifies the observed output set and its actual written hashes.
No absolute module paths, raw source code or arbitrary query values are emitted.

This is frontend build evidence, not Rust/C/native/Node/Python coverage or proof
that a module executes. A selected Manager build can bind the unchanged frontend
report/output inventory before and after compilation to its source/build receipt
and actual packaged executable. Tauri may compress embedded assets, so raw JSON
or JavaScript string presence in the executable is not assumed. Existing release
SBOM and supply-chain review retain their accurately stated scopes.

Focused helper/hook checks: `node --test scripts/frontend-provenance.test.mjs`.
Actual frontend/native artifact validation is recorded separately from synthetic
hook checks; this source change alone does not qualify signing or publication.

## Fixed runtime distribution qualification

The current producer can emit `pinned-notices-qualified-v1` for the same fixed
archive. `scripts/update-runtime-notices/distribution-files.json` pins the exact
original `PYTHON.json`, all 20 preserved license files (19 archive notices plus
the explicit supplement), and `DISTRIBUTION.txt`, by SHA256, byte count and mode.
The producer checks their actual selected bytes. A changed/missing/extra notice
or changed metadata cannot inherit qualification. Existing archive checksum,
selection, path, expansion/size and no-host-Python guards remain in force.

The declarations cover Python/PSF and CNRI, bzip2, MIT, X11, BSD-2/3-Clause,
Apache-2.0 and OpenSSL/SSLeay, 0BSD, SQLite's public-domain dedication, TCL and
Zlib. Their texts permit redistribution subject to the retained conditions;
the fixed metadata declares no GPL/LGPL/AGPL component. `DISTRIBUTION.txt`
provides the packaging change summary, provenance and OpenSSL/Eric Young
attribution, while retaining the complete original terms. Relevant advertising
must preserve the applicable OpenSSL acknowledgments and avoid endorsement.
The Python change-summary condition is in the retained Python license;
[Python's terms](https://docs.python.org/3.11/license.html),
[Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) and the
[OpenSSL/SSLeay terms](https://github.com/openssl/openssl/blob/OpenSSL_1_1_1w/LICENSE)
provide the upstream basis.

Extra archive notices are retained without inferring linked membership. In
particular, Berkeley DB's source-availability condition is not waived: its
notice is extra to the declared set; the fixed metadata declares no BDB link,
`_dbm` has no such link and its observed Mach-O dependency is system libSystem.
The declared readline provider is system libedit/ncurses, not GNU readline;
liblzma is declared 0BSD. This bounded audit does not assert a universal linked
component graph or transfer other dependencies to these licenses.

The native consumer accepts the old private contract unchanged or the new fixed
qualification with every pinned entry, then retains the compiled-manifest and
all-file hash/size/mode/path/identity verification before using Python. Old
consumers reject the new value; regenerate the runtime and build the matching
consumer together. The desktop prebuild guard admits official signing intent
only for the exact qualified source/notice manifest contract; arbitrary
`public`/`approved` labels, unknown metadata and older private resources still
refuse. Unread Tauri configuration overlays remain refused even with a qualified
source runtime, since they can replace the qualified resource mapping. That guard is a prebuild admission, not an on-disk byte verifier or
Apple approval: the native verifier supplies the artifact byte checks.

This source qualification does not generate or sign a package. Existing
Developer ID/notarization/stapling/Gatekeeper, checksum/source binding and clean
user install/update/offline acceptance remain separate release requirements.
