# Linux independent qualification

This manual workflow checks the exact reviewed public Manager source d441a88593ea11297fc2007454a143d0b2875b98 on a standard ubuntu-24.04 runner. The workflow exists separately from the pending implementation PR so it can run without merging unqualified product changes. Checkout and Node setup actions are fixed to official immutable source commits; token persistence is disabled.

The job is allowed only for the public ExhibitOS/manager repository, has contents-read permissions, one concurrency group and a45-minute timeout. It uses no larger runner, hosted cache, artifact upload, package publication, deployment or paid external service. GitHub's official billing policy permits free standard-runner compute for public repositories; logs are the evidence, not uploaded build artifacts. Repo visibility must be checked before manual dispatch. If it becomes private, the job condition prevents execution.

The runner checks a conservative8GiB additional peak plus6GiB retained floor before dependency installation; insufficient capacity fails without relaxing the floor. Apt/Node/Rust downloads and build outputs stay in the disposable runner. Tests execute as the normal non-root runner user. The workflow installs only public locked dependencies and has no operations/Capture source or local credential/media dependency.

Checks: frontend typecheck/lint/unit/build/receipt tests; full Rust workspace tests and strict all-target Clippy; Linux native executable build without bundles/upload; actual headless Chromium preview tests. Ignored native tests remain ignored and must be recorded as unexecuted. These checks do not qualify full engine migration/activation/recovery, publication crash boundaries, public changed-schema admission, actual native GUI interaction, Windows, signing or physical Capture. A green build must not mark T08-02/T08-04 complete.

Changes to the source pin require a new source/rights/secret/build review and evidence. Do not accept arbitrary refs, add automatic triggers, caches, uploads or paid services as a workaround for local space. No existing user originals/backups are uploaded or deleted.
