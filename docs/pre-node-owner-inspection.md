# Pre-Node owner diagnostic validation

This debug-only developer path inspects an existing personal `local-runtime`
whose exact SQL45 runtime ended with a pre-Node PostgreSQL cancellation. It does
not recover the profile, admit an ordinary runtime, or qualify a subsequent
import. No actual affected-profile inspection is established by source tests.

`exhibitos-native --inspect-pre-node-owner-v1 <existing-profile> <qualified-runtime>`
reads a bounded JSON request on inherited stdin. The request must select
`identityMode: "cluster-owner-observation"` and supply `owner` (workspaceId,
tenantId, principalId, ownerUid), `pointerSha256`, `failedStateSha256`,
`predecessorStateSha256`, and the exact retained `predecessor` binding. Its digest
must match the predecessor's retained completion, consumed by the failed run.
No caller SQL, database name, credentials, expected OID, or automatic fallback is
accepted. Existing installation-directory profiles are outside this path.

Before starting PostgreSQL, the implementation verifies the specific runtime
source/producer/manifest tuple and existing owner configuration, acquires only
existing profile/operation/writer locks, pins directories and source files,
rejects pending lifecycle operations, requires the failed process log to be
empty, and checks absent live recorded processes, postmaster file and occupied
ports. It runs the verified `pg_controldata` executable against the pinned cluster
to establish the cluster system identifier independently of the SQL connection.

The owned PostgreSQL process and bounded tools inherit the existing lock file
descriptions. A fresh server nonce and credentials authenticate the fixed local
`exhibitos` endpoint. One fixed read-only repeatable-read transaction takes
nonblocking authentication and maintenance locks in order, checks cluster ID,
owner/UID, active administrator and exact SQL45 ledger, and reads the bounded
four-kind diagnostic projection. Hayes claims and a global committed generation
are not inspected. Global idle is an observation and confers no authority.

The returned database OID is explicitly **observed only**. The receipt has null
`expectedDatabaseOid`, comparison `not-performed-no-independent-prior`, and false
logical database identity qualification, recovery eligibility and admission
eligibility. It must not become an independently authenticated prior identity.
The existing strict prior-identity SQL generator and verifier remain separate;
a missing or mismatched strict identity never falls back to this mode.

`diagnosticComplete` is emitted only after query success, rollback, normal owned
PostgreSQL exit, process-group absence, free ports and unchanged source records.
A failed operation returns not-inspected/null diagnostic fields and no recovery
claim. Cleanup uses owned children and normal SIGTERM, never forced termination
or adopted PIDs. Reaped terminal children are never signalled again. Uncertain
cleanup retains custody and the failed source remains quarantined. No current
pointer, completion, claim, migration, artwork, selection or exhibition is written.

Focused checks cover SQL projection/authority rejection, failed-source variants,
existing-only locks, empty-log custody, bounded synthetic pipes, normal group
return and nonzero/reaped-child signalling refusal. They do not establish actual
PostgreSQL execution of the SQL, affected-profile inspection, recovery, GUI,
signed release, or original-import qualification. A separately qualified helper
and a scoped execution handoff are required before any actual diagnostic.
