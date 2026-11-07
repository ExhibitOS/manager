# Native static publication refresh

The macOS development helper provides a fixed offline operation for the exact retained publication approved for local refresh. It uses a reviewed runtime bundled with the helper. Source and target runtime admission require exact pins. The target runtime pin remains closed until the integrated package is independently verified.

Pass one JSON object, at most 64 KiB, to `exhibitos-native --refresh-static-publication` on standard input and close standard input. Use a new operation UUID and an absolute, canonical path for each directory. The output parent must be owned by the running user and exclude group and other write access. The destination itself must be absent.

```json
{
  "operationId": "<new UUID>",
  "sourceDir": "/absolute/path/to/retained-static-export",
  "sourceManifestSha256": "<reviewed source manifest SHA-256>",
  "indexSha256": "<reviewed publication index SHA-256>",
  "publicationId": "<reviewed publication UUID>",
  "revisionSha256": "<reviewed publication revision SHA-256>",
  "targetManifestSha256": "<reviewed bundled runtime manifest SHA-256>",
  "outputDir": "/absolute/path/to/new-local-output"
}
```

The helper validates and copies the retained source and packaged runtime from held directory handles. The fixed child receives the verified operation-owned copies and a separate writable stage. The source snapshot and runtime files are read-only to the child. This boundary protects against changes to the caller-supplied paths and concurrent writers to the selected output. Processes with arbitrary access to the same user account remain within the owner's trust boundary.

The helper independently verifies the staged file inventory and exact publication content, flushes files and directories, then promotes the candidate without replacing an existing destination. The successful JSON receipt includes the operation, publication, revision, source and runtime manifest digests, output manifest, index, file count, byte count and output path. The operation relies on the reviewed retained export rights and the encoded restrictions at execution. It has no live authority or revocation query.

If a commit receipt is interrupted, keep the same request and use `--refresh-static-publication-status` to verify the retained operation's commit record and exact destination identity and hashes. This check is read-only. An unrelated destination or a missing commit record returns an uncertain result. A second refresh cannot replace an existing destination. Cleanup or drain uncertainty may retain the private operation directory `.exhibitos-native-refresh-<operationId>` under the selected parent for owner inspection. The helper never takes a cleanup path from the child.

If stdout contains a valid commit receipt and stderr reports `NATIVE_STATIC_REFRESH_COMMITTED_CLEANUP_PENDING`, the destination was committed and only private-stage cleanup is pending. `NATIVE_STATIC_REFRESH_CLEANUP_PENDING` reports a failure before commit. Keep the receipt or recover using the same operation ID when the commit outcome is uncertain.
