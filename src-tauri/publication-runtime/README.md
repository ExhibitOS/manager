# Public exhibition runtime

The generated directory is intentionally ignored. Prepare it with the repository's
`scripts/prepare-publication-runtime.mjs` using a clean public Platform checkout and
the existing static Viewer build. See `docs/publication-runtime.md`.
This resource contains public exporter code, locked dependencies, licenses and Viewer
assets only. It must never contain a profile, runtime.env, credentials or user data.
