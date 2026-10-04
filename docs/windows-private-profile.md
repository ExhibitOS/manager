# Windows private profile foundation

This development component is part of T08-01/T08-02; it does not enable managed Windows profiles, backup/update, or installer acceptance. Current Windows GUI uses the existing pinned profile path independently.

`windows_private::PrivateDirectory::create` creates only a new directory under an existing current-user-owned parent. It never changes existing ACLs or adopts a directory. The new root and records receive a protected, current-token-user-only full-control DACL at creation. Parent DACLs may retain SYSTEM/Administrators grants; other effective write/delete/owner/ACL grants are refused. Unknown allow ACE forms are refused conservatively. Security descriptors/SID buffers are checked using native APIs with bounded parsing.

Every lexical ancestor is opened without FILE_SHARE_DELETE and retained before canonicalization. Reparse points, relative paths, network paths and parent traversal are refused; volume/file identities are rechecked on retained and freshly opened handles. Records must be single-link regular files with protected owner-only DACLs; ASCII names reject separators, ADS, trailing dots and Windows device names. Existing records are never overwritten. A failure may leave a fresh diagnostic directory/file; there is no automatic repair, permission loosening or cleanup of user data.

## Validation

On native Windows with pinned toolchains, run:

```powershell
cargo test -p exhibitos-lifecycle --lib windows_private --locked
cargo test -p exhibitos-lifecycle --lib process_window --locked
```

Tests create uniquely named synthetic directories under LOCALAPPDATA and remove only their own successful fixtures. They check owner-only creation, duplicate refusal, unsafe names, root/record rename refusal, hardlink detection, an explicitly added Everyone-read grant refusal and native exclusive locking across handles. ACL mutation is confined to a new synthetic test record; existing user permissions are untouched. Failure fixtures remain for diagnosis.

Windows target compilation validates types, not NTFS permissions, native API behavior or GUI acceptance. Actual native results must be recorded before connecting this foundation to managed profiles. Remaining integration includes all profile/journal/secret writes, reopen/provenance, atomic publication, anchor/session locks, encrypted archive paths, trust floors and authority-loss recovery; existing Unix-only fail-closed guards are retained. Existing Windows warnings in unrelated Unix adapters are recorded separately.

MIT/Apache-2.0 upstream notices for windows-sys0.61.2 and windows-link0.2.1 are in licenses/windows-private, extracted from Cargo.lock checksum-verified registry archives. Product builds remain independent of private operations/Capture repositories.
