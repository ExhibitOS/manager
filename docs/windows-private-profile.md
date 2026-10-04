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

## Bound immutable records

`write_new_record` writes only a CREATE_NEW record after quota checking, holds an exclusive native lock, syncs its bytes, and retains the directory fence through a borrow. It never replaces or truncates existing generations. `read_record` reopens under that live directory fence with a shared lock; both paths omit FILE_SHARE_WRITE and FILE_SHARE_DELETE, refusing existing/new write handles and rename/delete during access. `BoundRecord` exposes checked bounded reads, retaining the original file identity and checking root/ancestors/DACL/singlelink before and after each read. Failed new writes may leave partial diagnostic records and are never automatically deleted. Read/write quota is at most16MiB.

Two additional native Windows tests cover exact close/reopen reads, concurrent shared readers, refusal of existing/new writers and delete attempts, duplicate generations and pre-creation quota refusal. These tests require actual Windows execution. This component is not yet connected to managed profile/journal/secret adapters. Namespace restart/provenance, atomic index publication, directory durability/power loss, trust history and whole recovery remain separate acceptance conditions; fsync of one file does not prove them.

## Common JSON adapter

Windows `read_json` and `write_json` now use this native guard. Private state/journal JSON reads require a protected current-user directory and record; invalid private ACLs are refused rather than automatically rewritten. Only public `bundle/manifest.json` input uses the separate owner/current-file-identity, reparse-safe and write-sharing-denied public reader; its ACL need not be a private protected DACL. Bundle authorization and signature/compatibility checks remain required by consumers.

A publication lock serializes writers. The writer validates prior private bytes/JSON, creates and syncs a protected new file with DELETE access, and calls SetFileInformationByHandle(FileRenameInfo) on that retained source handle. The directory and every ancestor remain pinned; source pathname reopening is avoided. An active private reader denies target deletion and causes replacement refusal while original bytes remain. Bad prior JSON or shared ACLs fail before replacement. Failed temporary files remain for diagnosis. Successful rename followed by validation/sync failure reports WINDOWS_PROFILE_PUBLICATION_UNCERTAIN; never assume that an uncertain result left the old pointer active or blindly retry.

This connects common JSON state/journal functions only. Secret/raw-file writers, private folder initialization, whole service/session anchors, namespace provenance across restart/replacement, history backup, selection/restore and power-loss durability still require integration and actual qualification. Current pinned Windows GUI mode is retained. Legacy unprotected data is not silently adopted or permission-repaired. Existing4MiB JSON limits are preserved, and individual file sync/handle rename does not prove directory/power-loss durability or multi-file recovery.

Native tests additionally cover core JSON create/read/replace and other-file preservation, active-reader replacement refusal, busy publication locks, unsafe ACL and malformed prior JSON preservation, invalid destination paths, and readable public bundle manifests without private-folder adoption. Native execution is required; cross compilation alone does not qualify the Win32 rename operation.

### Canonical drive-path regression

The first actual common-JSON Windows run at2fc6fe4 passed all six existing guards but failed all five new adapters with WINDOWS_PROFILE_IDENTITY_INVALID. Ancestor construction opened a bare canonical verbatim drive prefix before adding its explicit root separator. The guard now opens only the drive root and complete subsequent ancestors, retaining all identity/ACL/reparse checks and refusing bare-volume/drive-relative/network inputs. A native regression compares canonical and ordinary path root/ancestor identities and protected-directory reopening. The corrected12-test native run remains required; static compilation does not establish that the five JSON adapters now pass.

## Private adapter integration checkpoint

Windows installation directory create/inspect now use the protected native factory. Common immutable private writes connect runtime.env creation and selection-history generations to bound CREATE_NEW records; existing runtime.env is read through a bounded native record guard and never rotated or ACL-repaired. Private installation-backup source_bytes holds the supplied root/leaf directory and reads exact single-link records with ACL/identity and quota validation; empty files remain refused. General public source_bytes, operation/session locking, initial service-root creation, restart provenance, directory sync and full managed profile recovery are still separate unfinished work. This integration must not enable full managed Windows mode yet.

Actual4d4a3b1 Windows rerun:11PASS1FAIL, including all private JSON/canonical-path tests PASS. The remaining public manifest fails ACL validation. Windows default object owner can be a group SID from TokenOwner rather than TokenUser (Microsoft Owner of a New Object). Public input inspection now also accepts exactly the current token default owner, still requiring an explicit current-user full-control grant and refusing other nonprivileged writers. Private directories/records and their parents retain the original exact-current-user owner policy. The public test also adds an unrelated Everyone-write grant on its own synthetic file and requires refusal without changing bytes. Default-owner mismatch is the source-level failure hypothesis; native rerun is required to establish resolution. SID values and secrets are not logged.
