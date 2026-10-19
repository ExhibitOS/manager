# Packaged public publication exporter

`scripts/prepare-publication-runtime.mjs PUBLIC_PLATFORM_CHECKOUT [NEW_OUTPUT] [DIST_STATIC]`
prepares a new directory without downloading dependencies, starting services or building
the Manager. It requires a clean public Platform checkout, its existing installed locked
production dependencies, compiled API/storage/Studio contract outputs, and the separate
`apps/web/dist-static` output. Missing output fails preparation. Existing output is never
overwritten. Use Node 24.21.0; no private operations or Capture tree is accepted.

The stable consumer entry is `publication-runtime/generated/exporter.mjs`, which exports
the existing `exportStaticPublication` function. Pass the sibling `viewer` directory as
`runtimeRoot`; native code supplies its validated loopback origin, selected publication ID
and new output directory. The existing `remote-runtime/generated/node` remains the single
bundled Node runtime. Preparation does not implement authentication or native credential
transport and does not place credentials in the resource tree.

No esbuild dependency exists in the current public lock. Instead of rewriting the
exporter or flattening validators that use `import.meta.url`, preparation preserves the
locked production package tree and package metadata. API dist and its production
dependency closure include storage, public Studio contracts, the complete spec package,
AJV schemas, glTF validator Dart payload and original dependency licenses. The two
workspace packages are materialized as real directories under `node_modules`, with
their exact file hashes included in the manifest; no symlink is required. Dev
dependencies and host node_modules links are excluded. Unused API capabilities are present as code but are not
started or exposed by the exporter entry. No API server is run by preparation or import.

Viewer inputs are rejected unless every file is on the existing exporter allowlist;
the entry must contain exactly one `STATIC_INDEX_SHA256` placeholder. Platform AGPL
license, pinned public spec artifact and provenance, original package notices and a
public Git source archive at the recorded commit are retained. `manifest.json` records
the source commit, lock digest, dependency versions/integrities and packaged file hashes.
The source archive does not include untracked files, installed dependencies or local data.

Focused checks:

```sh
node --test scripts/publication-runtime.test.mjs
EXHIBITOS_PUBLICATION_TEST_PLATFORM=/path/to/public/platform node --test scripts/publication-runtime.test.mjs
```

The second check prepares an isolated temporary package with a synthetic Viewer and runs
the Platform's unchanged ten exporter regressions inside its copied dependency tree,
including synthetic GLB/PNG/WAV validation, corrupt asset refusal, withdrawal, unsafe paths,
existing-output preservation and budgets. The source tree's module cache cannot satisfy
the fresh child process. A separate fresh-process import is also part of preparation.
These two focused tests do not prove a real static Viewer build, native Manager integration,
local CMS authentication, a running engine or a provider deployment. Full native packaging must
verify `manifest.json` before invoking the runner; `manifest.links` is an empty map.

On 2026-10-17, Node 24.21.0 isolated packaging passed both focused tests and all ten
unchanged exporter regressions against Platform commit
`99ae38d44106a276f2675f4118a7ea4d498d4e9a`. The synthetic Viewer package contained
7,661 files, 32,606,550 bytes and 108 dependency/workspace records with no
workspace links.

The same day, the existing `apps/web/vite.static.config.ts` built the actual static
Viewer into a new temporary directory with Vite 8.3.1 and `--configLoader native`.
This avoids Vite's config-bundling cache writes in the source checkout. The 251-module
build completed; Vite reported its existing warning about chunks larger than 500 kB.
The source's existing dist directories were not changed. The actual Viewer has 19 files,
7,508,089 bytes; its largest file is `assets/navigation-camera-D1DqsdbE.js` at 4,354,922
bytes, below the exporter's existing per-file bound.

Preparing `src-tauri/publication-runtime/generated` with that Viewer succeeded:
7,677 files, 40,116,674 bytes, 108 dependency/workspace records, zero symlinks and
103 license/notice files. Every manifest file hash, pinned spec archive hash, source
archive, Platform/spec license and Viewer notices were checked. The existing exporter
then successfully exported the packaged synthetic GLB/PNG/WAV fixture using this actual
Viewer and an in-memory public API projection: 25 files, 7,740,023 bytes. All exported
manifest hashes matched and exporting to the same destination was refused without
changing the previous manifest. No HTTP server or account was used.

The missing static Viewer output is therefore resolved for this prepared resource
package. No Manager native build, running app, operating API, engine, credential store,
provider or external network was used. Native resource integration and a real local
CMS catalog/login/export workflow remain separate checks.

## Manager local catalog bridge

The native bridge uses the selected running installation's existing `runtime.env`
CMS identity for a short-lived loopback login, catalog request and CSRF logout.
Credentials, cookies, CSRF, runtime origin and output paths never reach the webview.
Only published metadata is shown; listing does not scan or hash artwork files.
A selection is pinned to the installation selection token, loopback origin, publication
and revision. Preparing it uses the unchanged packaged anonymous static exporter,
which rechecks current public rights, revision and asset hashes. It writes a new unique
`publication-exports/publication-UUID` under the selected service and changes the
hosted source only after the output identity and manifest match. Existing outputs and
the previously selected bundle survive failures. This preparation never uploads.

The packaged exporter closure is verified only when preparing an export, not on list
refresh. Native commands run off the UI thread and the close guard covers their owned
child. The reusable Node executable and fixed runner are hash-pinned with existing
remote resources. Exporter dependencies and Viewer assets are pinned by a build-time
manifest, with bounded sequential verification rather than thousands of open files.

Validation: synthetic credential parsing/filesystem tests 2 passed; native closed
catalog and prepared-bundle tests 2 passed; local HTTP auth/client tests 10 and staged
bundled-Node runner tests 5 passed, including actual packaged Viewer + synthetic GLB/PNG/WAV export.
Typecheck/lint passed. Browser/native-mock checks do not validate a real CMS login.
An actual selected local service login→catalog→export and packaged desktop UI remain
unexecuted; the running hosted-deployment app has not been replaced.

The publication runner alone uses Node `--disable-proto=delete`. The pinned GLB
validator's Dart import feature probe reads `__proto__` and throws before processing
under `--disable-proto=throw`; delete mode removes the accessor and uses Dart's
`Object.create` fallback. A regression proves the getter/setter is absent and assigning
an own `__proto__` property does not change the object's prototype. Existing remote
and hosted runners retain throw mode. Input bounds and existing validator checks are
unchanged; no library was patched or validator bypassed.

The new catalog UI passed desktop and narrow browser/native-mock checks (2); the
existing hosted flow passed its 12 browser regressions. These are presentation/IPC
binding checks, not a real local CMS or native app integration result.
