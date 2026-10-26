# Native macOS application (in progress)

Responsibility and build boundaries are defined once in the [Platform architecture](https://github.com/ExhibitOS/platform/blob/main/docs/architecture.md#repository-roles-build-boundaries). This page is for developers, not end-user installation prerequisites. The current native development slice has bounded synthetic save/cold-reopen/OEX and existing-publication-to-static-output evidence. Native settings GUI/accessibility, real hosted delivery, legacy migration and final signed installation remain separate. Dated coverage below records its original checkpoint and must not override later scoped results.

The SwiftUI window is connected to the existing Rust installation controller by
the bundled `exhibitos-native` helper. The helper has no Tauri dependency. Both
presentations share the same selected installation and existing durable jobs.
Mode, current screen and100–200% text preference are revision-checked and saved
in the private managed profile; changing modes never changes installation or
claims a workflow step complete. Corrupt state is preserved and refused.

The application uses system navigation, menus, settings, semantic colors and
scaled text. Preview configurations cover light, dark and a narrow200% layout.
These previews are source configurations, not accessibility acceptance evidence.
The `docs/decisions/native-authoring-macos.md` decision describes unified native
Studio/CMS controls and reuse of the existing shared web3D viewport/API.

Development with the pinned Rust toolchain and Xcode:

```sh
cargo test -p exhibitos-lifecycle --locked --test native_ui
cargo clippy -p exhibitos-lifecycle --locked --bin exhibitos-native --test native_ui -- -D warnings
EXHIBITOS_NATIVE_TEST_HELPER=/absolute/path/to/target/debug/exhibitos-native swift test --package-path native
```

`scripts/package-native-preview.py` assembles existing built outputs into a new
development app/profile; it never replaces an installation, builds or signs an
artifact, or accesses credentials. Supply the debug Swift executable and built
Rust helper explicitly. The validation-profile Info key is recognized only in
debug builds. Use an isolated profile for validation and retain original apps.
Release delivery/signing is a separate existing distribution pipeline.

The native packager copies the approved Manager icon and integrated Studio/CMS
marks from `native/Resources/Brand` into the app's existing Resources directory.
`CFBundleIconFile` names `manager.icns`; the existing screen header uses the
Manager PNG and Studio/CMS SVGs at 32 points or larger through AppKit. The small
system sidebar symbols remain available. Attribution and license copies accompany
these resources, including the rounded MDI floor-plan modification notice. No
external font or brand-generation tool is required by the app. Existing bundles
acquire these assets only when a subsequent package includes them; source changes
alone do not establish package, GUI or distribution qualification.

An optional `--runtime-payload /absolute/newly-produced-runtime` embeds the
Platform producer's local validation payload under `Contents/Resources/native-runtime`.
The helper's closed `runtime_payload` query resolves that fixed app-relative path,
verifies its complete declared file set and internal links, and returns byte
identity only. Self-declared release flags never establish signed distribution
admission. The optional legacy installation API retains first-account validation and its
private `runtime.env` contract. New personal workspaces instead use the
Mac-user binding described below; both paths preserve existing account data and
refuse automatic Docker/account installation adoption or damaged settings. Configuration
is distinct from initialized database, service readiness and release acceptance.

`native_runtime::Session` now owns the prepared installation's PostgreSQL and
Node processes under the existing operation fence. It uses fixed payload paths,
a minimal environment, bounded private per-run logs and durable run records.
The PostgreSQL connection must return this run's instance nonce before any
database creation; Node must report its own successful listener startup before
the existing five-component readiness contract is accepted. Stop sends SIGTERM
only to retained child handles, keeps ownership while termination is pending,
and preserves the database/configuration for reopen or explicit retry. Occupied
ports and unknown live PIDs refuse; there is no automatic PID adoption or kill.
The fixed helper now routes `native_prepare`, `native_start`, `native_status` and
`native_stop` through one asynchronous owner. Preparation/start/stop return a
background state; presentation settings remain available while they run. A stop
arriving before startup publishes its cancellation handle is retained. EOF and
SIGTERM/SIGINT trigger owned shutdown before releasing the profile. Oversized or
truncated input also drains owned services before exiting with a protocol error.
The existing core checks are supplemented by actual helper pipe cancellation,
EOF/SIGTERM termination, durable terminal exit records and same-profile reopen.

SwiftUI now provides first-account secure entry, explicit preparation/start,
progress and stop controls, and live native readiness. Successful preparation
starts services only as part of the explicit “prepare and start” action; reopening
never starts automatically. Closing a window keeps the app alive; quitting while
native work is active asks to stop and quit, and completion waits for actual child
termination. Internet-published exhibitions are separate from local shutdown.
The app delegate and button flows compile but require actual GUI qualification;
headless helper checks alone do not prove them.

At the initial lifecycle checkpoint, coverage included the native window, bounded pipe connection, persistent
presentation preferences and actual helper-owned preparation/start/stop. Native
button/quit GUI acceptance, backup/update/recovery parity, integrated artwork/exhibition editing, shared3D
viewport, publication, data migration, complete keyboard/VoiceOver interaction
and final signed delivery remained unfinished. The then-current create/preview/publish
destinations describe missing preconditions and perform no publication. This is
not a Docker-free release or a qualified replacement for the existing Manager.


## Native artwork management (partial authoring)

The Create destination now contains a personal workspace connection, optional
existing-account login, artist creation, artwork
search/pagination and metadata forms, using the existing CMS API. The selected native configuration supplies workspace identity. New personal
workspaces use a private helper-issued session, while optional legacy passwords
remain transient SecureField inputs. Sessions use a private ephemeral cookie jar.
Requests stay on the selected IPv4 loopback origin, refuse redirects, use the
existing CSRF token and bound tenant, limit JSON responses and never log secrets.
Authentication changes are serialized against in-flight requests.

The native editor preserves metadata/rights/provenance fields, sends the existing
revision on PATCH and retains edits on errors. Changing artwork/new-document
requires an explicit discard when dirty; quitting also warns about unsaved
artwork input. Mode/navigation changes share the same in-memory editor. A login
with a different account cannot reuse another account's dirty draft. Durable
unsaved-draft resume, Keychain session persistence and actual interaction
acceptance remain pending.

The actual isolated Swift/API integration creates a synthetic artist/artwork,
updates/reopens its metadata and rights, observes stale-revision rejection,
searches and logs out/in against bundled native PG/Node. This proves the Swift
HTTP boundary for that admin fixture, not native GUI acceptance, every RBAC role,
GLB import/upload/job recovery, server comparison UI, immutable approval,
shared3D/exhibition editing or OEX/publication completion. Those remain part of
the same first release scope.


## Personal workspace without a local CMS account

The default “이 Mac에서 시작하기” prepares a private, durable workspace bound to
its Mac UID, workspace/tenant and internal owner IDs. It creates no CMS password
credential and asks for no username or password. The Rust helper supplies random
per-run session/CSRF values to its owned Node child over bounded inherited stdin;
they are never status/journal fields, environment tokens or command arguments.
Only an explicit private helper request may return the active loopback session to
Swift. Foundation adopts it in an ephemeral cookie jar, then confirms the server
session's tenant/principal through the same authenticated API used elsewhere.
Origin, CSRF, RBAC and revision checks stay enabled. There is no anonymous HTTP
bootstrap or administrator route. Existing installations remain legacy data and
require an explicit connection/migration; they are not silently converted.

The native helper+Swift integration now demonstrates a new personal workspace,
authenticated authoring without `/auth/login`, metadata save, normal service
stop/restart, changed per-run token and same-owner data reopen. Native ownership
mismatch/legacy-file preservation and actual server origin/CSRF/legacy-account
refusal are separate passing checks. This is metadata-boundary evidence only.
Native GUI acceptance, actual GLB/textures/exhibition/OEX/static publication,
durable unsaved drafts, optional migration/backup compatibility and seamless
renewal after the current eight-hour server session are still pending.


## Native GLB file transfer and recovery

The native editor now offers an AppKit file panel and bounded, read-only GLB/PNG
snapshots (1–32 MiB). It refuses nonregular files and final symlinks, detects
source changes during reading, and holds immutable bytes only in Swift memory.
Before creating a server import, the private helper durably saves the exact
intent, hash, length, scale, rights and idempotency key. The record is bound to
the selected personal workspace and principal, revision checked, and contains
neither source paths nor session secrets. After relaunch the user reselects the
same bytes to resume an incomplete upload. An uncertain create response can be
recovered explicitly with the same request. No new request is silently invented.

Existing authenticated import APIs perform binary upload, validation, status,
retry/cancel and revision-bound artwork approval. A separate acknowledgement
confirms the saved metadata, asset and rights. The actual isolated Swift/helper/
PG/Node test imported a synthetic 1,516-byte GLB, recovered the same job after a
lost response and service restart, verified approved disk bytes, approved the
artwork, and reopened it. A malformed GLB failed validation, retried and cancelled
without replacing the earlier approved file. Unicode/space bundle paths are
covered. Source-file refusal and transfer-state ownership/corruption/symlink/CAS
checks passed separately. Run `ImportFileTests` for the bounded reader; the
explicit-fixture `GLBImportTests` requires an isolated payload/helper/profile.

Actual beginner GUI also selected the synthetic GLB through an owning-window
file sheet, saved its metadata/rights/provenance, completed validation and explicit
artwork approval, then reopened the same file/rights/import request after app
relaunch. Both GUI-owned service runs stopped normally with Node/PostgreSQL exit0.
The first standalone file-panel candidate was not accessible to the control tool;
that candidate/data were preserved and the panel was changed to a window sheet.

This supersedes the earlier GLB client-only gap in the tested slice. Textures, shared3D viewport,
exhibition editing, OEX/static publication, complete accessibility and operational
migration/backup/update acceptance remain pending. It does not qualify a release.


## Native exhibition documents and retained save requests

The exhibition destination now lists accessible Studio documents and edits title,
room name/dimensions and approved CMS artwork placement using SwiftUI controls.
The document retains opaque extension fields, original approved artwork transforms,
rights and inventory. Every added placement gets the same plain-text accessibility
alternative as the web editor. Platform's complete OES/Studio validators remain
in force; the initial missing-alternative failure was retained and fixed in the
native construction, without relaxing server admission.

One current valid draft, base ETag/saved edit version and exact pending save are
stored privately by the selected Mac workspace's Rust controller. The state is
principal/workspace bound, revision checked, owner checked, size bounded and
flushed before a POST/PUT; it contains no credential, source file path or asset
bytes. The inherited IPC frame bound is now3MiB (document store2.2MB), while
upload intent128KiB and JSON response2MiB bounds remain separate. Corrupt or linked
records are refused and preserved. A changed or unacknowledged request is never
silently assigned another ID. Known conflicts preserve the native input; a saved
version is read for comparison and applied only by explicit selection/confirmation.

Actual Swift/helper/PG/Node integration saved and recovered an unacknowledged
POST after relaunch, placed a real synthetic GLB, saved/reopened revision2,
refused stale If-Match and changed request payloads, and restored an unsent valid
local draft separately from the saved server version. Actual beginner GUI restored
that draft, changed title/room/position, saved revision3, compared and reopened it.
The GUI test's list row center only focused its Button row; the latest source uses
native List selection binding instead (compiled, not requalified in that candidate).
All owned native service runs stopped with Node/PostgreSQL exit0.

The current editor is partial: multiple-room/surface/lighting controls, shared3D
viewport/direct manipulation/undo, OEX/static publication, durable raw invalid
form input, full keyboard/VoiceOver/expert and migration/backup/update qualification
remain unfinished. Locally retaining valid input is an explicit button, not a
promise that every in-progress keystroke survives an unexpected exit. The server
list's query-admission tests passed; full cross-role/tenant HTTP acceptance remains
separate from the tested personal-admin flow. This is not release qualification.

The first native light inspector source adds list-based selection, existing
point/spot/directional/area inputs, room-local position, linear-RGB color,
renderer intensity, full beam angle in degrees and area size. General mode
disclosures expose the same position/color fields as Expert mode. Text stays in
the shared exhibition model when the inspector hides or the mode changes.
Preview requires the optional `light-inspector-v1` request and the existing
shared full validator; the old v1 frame selection and geometry action remain
closed. Exact reply/base/input/account binding precedes local journal Apply and
the existing Undo/Redo, server save and export paths. Opening unsupported data
refuses editing rather than replacing its values with defaults. Units follow
the existing producer; changing between different units uses its default input
of100 rather than a photometric conversion. Fixture shapes, direct3D movement,
aiming, Kelvin, calibrated illuminance and full lighting release acceptance are
separate. Source and synthetic unit coverage do not establish actual GUI,
save/reopen, OEX/Viewer/static consumer or distribution qualification.

The native bench producer uses the optional `bench-inspector-v1` bridge with
list-based selection independent of the strict v1 frame selection/action kinds.
Slab/backed benches expose room-local position, yaw in degrees, seat dimensions,
and the existing InstallationMaterial (sRGB hex and PBR factors). General mode
disclosures expose every Expert numeric field from the same model buffer. A new
bench can atomically create one fresh standard Presentation viewpoint even in an
empty exhibition. Editing a camera creates a fresh viewpoint and relinks the
bench; existing/shared viewpoints are preserved. Removal retains the camera.
Full shared validation and exact source/result/account/input binding precede
Preview, explicit Apply and one existing journal/history entry. Cancel discards
only this form's uncommitted preview. Save, reopen and outputs use the existing
exhibition paths; native code does not remap public/OEX identities or implement
seated physics independently. The shared Viewer owns explicit Sit/Return and
runtime clearance. Source/unit verification does not establish actual native
GUI, saved-output consumption, seated operation or release acceptance.

The native plaque producer references existing exhibition title/introduction,
room name or a same-room artwork placement. It stores no copied prose, fonts,
URIs or assets. List selection is independent of the strict v1 frame kinds.
One model buffer holds source, visibility, size, position and quaternion text;
General disclosures expose all Expert numeric fields and the same four facing
presets. Hidden records stay validated and retained. The optional
`plaque-inspector-v1` bridge uses full shared source/result validation and exact
binding/delta checks before transient Preview and explicit Apply. One existing
journal/history entry carries the change through normal save, reopen and
exports. Removal changes only the plaque namespace and the existing template
marker; original source text, artwork and unrelated rows/extensions are kept.
Native GUI and saved-output/host continuity remain separate qualification.


### Shared 3D viewport development slice (2026-10-23)

The SwiftUI editor embeds only Platform's dedicated shared GeometryPreview viewport,
with a nonpersistent WK cookie store and per-view private session. Navigation and
content rules pin the loopback origin, native viewport page, compiled assets and
scoped approved-artwork projection/preview paths. Nonce/sequence/owner checks reject
stale callbacks. Native proposals retain all nongeometry fields and persist through
the existing local document CAS; they do not publish or bypass save validation.

Actual development GUI has displayed one approved synthetic GLB with zero failed
assets. Initial module readiness is awaited before the first frame. Frames travel as JSON
text to avoid Foundation container identity affecting empty JS collections. Strict CSP is
preserved; Platform precompiles its existing AJV schemas instead of adding runtime
eval permission. This is partial evidence: native white-cube generation, undo/redo, light addition
and server save revision2 passed in actual GUI. The original Foundation object bridge
aliased empty collections (actual WK regression: [1,1,1,1]); JSON transport preserves
[1,0,0,0] and fixed the geometry rejection without relaxing schema admission. Read-only
preview loaded the approved GLB, but the closed cube occludes it from the current
outside camera in that candidate. The camera/transparency gap is superseded by
the following scoped development acceptance; full accessibility and recovery remain. It is not
release qualification or a substitute for OEX, publication, accessibility and recovery.


The dedicated viewport frame now includes a bounded camera request sequence and a
space-transparency flag (12 fields, packaged Manager/Platform pair). Repeated view
buttons issue actual camera actions; native transparency defaults on and only
changes rendering, preserving saved materials and candidate bytes. The dedicated
page memoizes its material lookup by document, so camera/selection frames retain
the renderer instead of resetting it. Existing web/Viewer consumers retain their
uncontrolled camera and transparency behavior.

Actual r21 beginner GUI passed top/isometric/front camera changes, a placement gizmo
drag reflected in native X and durable draft (2.0538089134331527m), white-cube6,
opaque/transparent switching, save revision2 and visible read-only front preview
of the displaced approved GLB. Loaded1/failed0. Expert mode persisted, but Computer
Use disconnected at selection and stayed unavailable after rebind/reset, so expert
rendering/reverse transition is unqualified. With the document saved/no pending
request, exact owned-app normal SIGTERM led to helper EOF and Node/PostgreSQL exit0.
At that checkpoint OEX/static publication, comprehensive selection/rotation/dimensions/keyboard/
VoiceOver/mode/relaunch UX, durable raw input/history/view preferences, session renewal,
and operational/distribution compatibility remain incomplete.


### Native OEX export development slice (2026-10-23)

The exhibition editor and preview now offer an owning-window NSSavePanel for OEX.
Uncommitted native input or an uncertain save disables export with save-first guidance.
The private Foundation client sends the existing authenticated, Origin/CSRF-protected
POST with the exact saved ETag, and never refreshes a conflicting ETag automatically.
The existing Platform service still verifies current approvals, export rights, source
bytes and complete OEX profile. No anonymous API, parallel archive format or web UI
wrapper is introduced.

Binary response admission rejects redirects, wrong MIME/encoding, declared or streamed
sizes over64MiB, changed session generation and truncated declared content. The client
checks ZIP transport magic only; Platform/public-contract validation remains authoritative
for the archive. Transfer/resource timeout is bounded120s; ordinary JSON request idle
limit stays15s and JSON response bound2MiB. User-selected output is written atomically on a
worker task after validation; canceling the native save sheet keeps the document.

Actual core export produced6816bytes from the existing native GLB/geometry fixture;
public OEX validation and exact1516byteGLB SHA passed, with CMS bindings removed and
geometry/placement preserved. Actual staleETag412, no-export-rights403, isolated-client401,
and missing-directory file-write refusal passed without changing the qualified source.
Loopback transport exercised wrong MIME, redirect and both declared/streamed64MiB bounds.

Actual r23 beginner GUI passed dirty-title gate, save revision2, native save-sheet cancel,
retry from preview, new5360byteOEX save, completed-path/Finder affordance and independent
public-contract/GLB-byte verification. Expert r22 observation crashed SkyComputerUseService
(Array.remove assertion), with expert preference/source retained; a fresh beginner profile
isolated the GUI test. Expert/full accessibility remains unqualified. All owned PG/Node
runs stopped normally. Output-location/hash feedback is memory-only; static publication,
full mode/relaunch/recovery and final release/distribution acceptance remain incomplete.


## Optional exhibition lighting settings (pending shared runtime)

The native `space-settings-v1` adapter edits the shared, exhibition-wide
`org.exhibitos.studio/space-lighting` namespace through the negotiated
`window.exhibitosViewport.spaceSettings` command. It is disabled when the shared
viewport does not advertise that capability. The existing `light-inspector-v1`
contract remains limited to its original light-row change.

General and Expert share the same model-owned raw input and baseline value;
Expert also exposes reflection directly. General can disclose the same reflection
field. Existing unset documents retain their previous rendering until an explicit
edit. Both values must be entered when creating an override or selected through an
explicit preset button. The shared D1 comparison qualified calm (0.15/0.2) and
bright (0.2/0.45) baseline/reflection mappings; these are display-response
conveniences, not photometry or human visual acceptance. Preset names are derived
from the same two values, never persisted. No legacy-to-new default mapping is
assumed. Unsetting submits `settings:null`.
Transient preview and cancel preserve the saved document, and the existing one-item
history restores the original namespace absence and template provenance.

This native source does not establish shared runtime capability, renderer changes,
GUI, OEX/static acceptance, mobile performance or visual quality. Those
boundaries require the matching shared implementation and scoped actual evidence.

### Optional furnishing catalog (source integration)

`furnishing-visuals-v1` adds one protected catalog entry, the CC0 canted display
pedestal, to an existing selected artwork placement. The native inspector keeps
position, angle and uniform scale as raw text shared by General and Expert modes.
Browsing does not edit the exhibition. An explicit shared `furnishingVisuals`
preview is checked for its exact request binding and permitted delta before Apply
creates one existing native history entry. Removing the visual keeps its legacy
conservative box; Undo restores the prior namespace, including absence.

Catalog metadata is checked against the immutable source/version/hash/rights.
`furnishingThumbnail` has its own catalog/hash identity and bounded 256px PNG;
artwork thumbnail and import authority are unchanged. Full GLB, room/world hull,
support, OEX/static inventory and rights admission remain with Platform. The
native adapter does not advertise support, ship a new renderer or generate asset
quality tiers. A runtime without the negotiated capability retains the legacy
editor. The selected visual locks legacy pedestal configuration until removal;
existing separate center/on-top actions use shared measured support. Chair/bench
visuals and automatic artwork movement are unsupported in this slice.

Source tests cover correspondence and raw input only. Matching qualified runtime
artifacts and native save/reopen/output/consumer actual evidence are required
before claiming this feature complete.
