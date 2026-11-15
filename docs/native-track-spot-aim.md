# Adjust an articulated track spotlight

In the light inspector, select a spot light and open **트랙 스포트 조준**. Choose a ceiling rail in the same room. Enter the mount in metres relative to that rail and choose an aiming method.

- **작품 기준** uses an artwork placement and an offset in that placement's local axes.
- **공간의 지점** uses a point in the light's room, in metres.
- **각도 직접 입력** uses swivel and pitch. The inspector displays degrees and the SDK transmits radians.

Use **조준 미리보기** to check the solved emitter and the existing viewport preview. When the preview is ready, choose **조명 적용**, then use the ordinary Save control. The shadow priority controls address the light's shadow settings separately from the aiming target. A rejected preview leaves the inputs available for correction. Hiding or changing an articulated visual requires an explicit mode exit, which this profile does not provide. The whole light can be removed through its existing preview and Apply action.

The SDK and CLI reuse the saved-head `light-object-prepare` and `light-object-apply` operations. They require the existing light-object proposal capability and the exact `nativeTrackSpotAimProfile: "track-spot-aim-v1"`. The catalogue identity is `museum-track-spot-articulated/v1`.

- Entry uses the existing `set-light-fixture` operation. Its `visual` contains `catalogId`, `catalogVersion`, `pose` and `trackAim`. The pose is position `[0, 0.15, 0.0755]`, yaw `0` and scale `1`.
- Updates use `set-light-object` with the existing light transform and `settings.trackAim`. The settings use `fixture: "none"`. The mount supersedes the legacy light attachment.
- The closed binding has `version: 1`, `profile: "track-spot-aim-v1"`, `mount: {railPlacementId, position, yaw}` and one `aim`. The alternatives are `{kind: "placement-center", placementId, offset}`, `{kind: "point", position}` and `{kind: "manual", yaw, pitch}`.

The server checks the known catalogue, rail and target and solves the emitter. Native binds the returned selected transform to the preview and verifies the rest of the candidate. An owner-scoped proposal rejection still uses error code `AUTHORING_PROPOSAL_INVALID`. Version-one CLI error replies may also contain `trackAim: {reason, remedy}` when the response matches a known producer diagnostic. Other error replies retain their existing shape.

Source contract provenance is Shared `aca66b2`, API proposal `f88461ef` and API diagnostic adapter `e346edf`. The source fixtures cover closed inputs, fixture entry, server-solved transforms, retained shadow settings, unrelated candidate changes, explicit mode exit and bounded diagnostic serialization. Product build and live use need their own qualification.
