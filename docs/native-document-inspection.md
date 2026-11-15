# Inspect an existing Native document

The developer automation executable supports one read without starting the
runtime or opening an authoring session:

```sh
exhibitos-automation --inspect-existing-document --helper "/absolute/path/to/exhibitos-native" --profile "/absolute/path/to/existing-profile"
```

Both paths must already exist and be canonical absolute paths. The helper opens
the existing managed profile with its normal exclusive profile-session fence.
It does not initialize or recover a profile. The command uses only `context`
and `document_state`, then closes the helper through EOF. It never requests
Native status, runtime start/stop, authentication, Save or a journal update.
A missing existing document is refused, even though the helper's ordinary
document reader can return an in-memory revision-zero default.

Success writes one JSON line containing owner IDs, Native revision, saved and
current edit versions, exhibition ID, ETag, dirty status and pending Save
request ID/origin/ETag. It excludes candidate bodies, selection tokens and
credentials. Live runtime ownership remains `unknown`; this inspection does not
probe orphan processes. Applied-edit attribution is also `unknown` unless the
document holds the exact opt-in `automation-applied-draft-v1` Pending shape. That
shape reports its retained apply request ID and origin only, without inventing
an operation name. A legacy pending Save ID does not establish which operation
applied an unsaved edit. No Save or recovery authority is granted by inspection.

Failure writes a code to stderr and returns a nonzero exit status. An active
profile owner is refused by the existing helper's `PROFILE_BUSY` guard. The
current bridge reports helper startup refusals as `NATIVE_HELPER_EXITED`, so this
command cannot distinguish that refusal from other startup failures. Close the
current profile owner and check the existing managed profile before retrying;
the command never steals ownership. Ordinary `--configuration` behavior and
its clean-start requirements remain unchanged.

Source and fake-helper tests do not qualify an installed helper or an actual
profile. Component build and supported existing-profile use require their own
evidence.
