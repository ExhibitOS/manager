# Linux Debian package qualification

This original T10-05 workflow builds one `.deb` from reviewed public Manager source `78c32ee4d4fc95bb48dbfb78bb6f5292811f1905` on the existing standard Ubuntu 24.04 x64 runner. It does not introduce another distribution, package format or support promise. The existing Windows/macOS unsigned prototype command remains unchanged.

The manual `.github/workflows/linux-package-independent.yml` fixes official checkout/setup-node actions to the same reviewed immutable versions as the existing Linux workflows, uses `contents: read`, disables persisted checkout credentials, shares the existing concurrency group and has a 45-minute timeout. Its public-repository condition must be confirmed before dispatch. It uses no hosted cache, artifact upload, encoded package output, release publication, deployment or paid external service. [GitHub documents free standard public runners](https://docs.github.com/en/billing/concepts/product-billing/github-actions); repository visibility and the reviewed free-CI policy remain execution prerequisites.

## Build inputs and space

Node24.21.0/npm11.19.0 and the repository Rust1.99.0 toolchain consume existing locked dependencies. The job installs only public Ubuntu development dependencies. It separately downloads the official vendor Node Linux x64 archive to fresh runner temporary storage because setup-node does not promise the archive path consumed by `remote:prepare`. The official archive metadata observed on 2026-10-12 has 31,890,184 bytes; the download is bounded to that exact size and the existing SHA256 `fd8e59d5a511510f6a298afb548f18c7d2b1be404d8b4a27d94fbe49f56cb2d6` is verified before preparation. No local Mac binary, private operations/Capture source, credential or artwork enters this build.

The unchanged preparer already supports Linux x64 and records vendor Node/license/deployment resource hashes. The package is built with the existing Tauri CLI:

```sh
npm ci
npm run remote:prepare
npm run tauri -- build --ci --bundles deb -- --locked
```

The job requires the existing conservative 8GiB additional allowance plus 6GiB retained floor before dependencies. Rust uses two jobs without incremental compilation. Dependencies, compiler output, unpacked inspection files and the package remain on the disposable runner; no hosted caches or persistent upload storage are used. There is no unrelated full browser/workspace retest when source is unchanged; native compilation and actual packaging are necessary for this missing original deliverable.

## Actual evidence

The workflow requires exactly one regular `.deb` no larger than 300MiB, with version0.1.0 and amd64 architecture. In the exact [Tauri CLI2.12.1 Debian control implementation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/linux/debian.rs), the package field uses `heck::AsKebabCase(settings.product_name())`: the current `ExhibitOS Manager` product therefore maps to `exhibit-os-manager`, separately from Cargo/main binary `exhibitos-manager-desktop`. The package version is matched to package.json, Cargo.toml and tauri.conf.json; the actual desktop Name/Exec and sole installed main binary are checked against those exact source inputs. This preserves product identity without guessing that the Debian package name equals the Cargo binary name. It records source commit/tree, package bytes/SHA256 and vendor resource hash counts as compact JSON. It extracts the actual package and checks its desktop file, executable, manifest, vendor Node and all manifest resources against their declared hashes.

Only after confirming the package is not already registered does it install that exact generated package in the disposable runner. The ordinary non-root runner user must read the installed manifest/license/resources and execute the bundled Node24.21.0 binary. These checks exercise actual root-owned installed resources; extracting a package as its owner alone cannot prove normal-user access. Existing generated resource permissions are not changed speculatively to make inspection pass. If actual installation exposes a permission defect, preserve the failure and fix that existing packaging requirement with the smallest scoped change.

An `always()` step removes only this newly installed package and checks executable/vendor Node absence and package registration. Ubuntu dependencies and generated package bytes remain until automatic runner disposal. No existing user installation, artwork, key or backup is present or removed. A failed build or normal-user check remains a failed job even if removal succeeds.

## Scope and retention

A successful run proves this exact-source `.deb` build, contents/checksum, package-manager installation, normal-user vendor access and removal. It does not prove native GUI interaction, offline exhibition Runtime, upgrade/rollback, migration, publisher signing, another Linux version or production release. [Tauri Debian documentation](https://v2.tauri.app/distribute/debian/) describes this packaging format; the workflow does not broaden the frozen completion criteria.

Logs preserve small verification evidence, not package bytes. No-upload CI therefore completes only the actual build/install/remove portion. A downloadable, retained release artifact remains separately unfulfilled after runner disposal; checksum metadata cannot recreate the original package. Do not encode the package into logs or activate paid storage as a workaround. The final artifact custody/download decision and remaining production release conditions stay explicit in the original T10-05 evidence.
