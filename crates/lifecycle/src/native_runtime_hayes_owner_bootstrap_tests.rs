// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::native_runtime::{
    furnishing_lifetime::WriterLifetime, hayes::operation::HayesBootstrap,
    owner_channel::Connection,
};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    path::PathBuf,
    process::{Command, Stdio},
};

const GENERATION: &str = "11111111-1111-4111-8111-111111111111";
const MANIFEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn authority() -> LocalAuthority {
    LocalAuthority {
        format: 1,
        workspace_id: "22222222-2222-4222-8222-222222222222".into(),
        tenant_id: "33333333-3333-4333-8333-333333333333".into(),
        principal_id: "44444444-4444-4444-8444-444444444444".into(),
        owner_uid: 501,
        session_token: "test-session".into(),
        csrf_token: "test-csrf".into(),
    }
}

fn pin() -> HayesSessionPin {
    HayesSessionPin::new(GENERATION.parse().unwrap(), MANIFEST).unwrap()
}

#[test]
fn selected_bootstrap_is_closed_and_legacy_has_no_hayes_pin() {
    let auth = authority();
    let ordinary = Setup::new().unwrap();
    let legacy = ordinary
        .workspace_furnishing_bootstrap(&auth, GENERATION, FurnishingStart::Reopen)
        .unwrap();
    assert!(!String::from_utf8(legacy)
        .unwrap()
        .contains("hayesOwnerChannel"));

    let hayes = HayesSetup::new(pin()).unwrap();
    assert!(hayes
        .bootstrap(
            &auth,
            GENERATION,
            FurnishingStart::SelectedImport040,
            &ordinary.secret
        )
        .is_err());
    assert!(hayes
        .bootstrap(
            &auth,
            GENERATION,
            FurnishingStart::Reopen,
            &hayes.socket.secret
        )
        .is_err());
    assert!(hayes
        .bootstrap(
            &auth,
            &auth.workspace_id,
            FurnishingStart::Reopen,
            &ordinary.secret
        )
        .is_err());
    let selected = String::from_utf8(
        hayes
            .bootstrap(&auth, GENERATION, FurnishingStart::Reopen, &ordinary.secret)
            .unwrap(),
    )
    .unwrap();
    assert!(selected.starts_with("{\"workspace\":"));
    assert!(selected.contains(&format!(",\"ownerChannel\":{{\"version\":3,\"runtimeGeneration\":\"{GENERATION}\",\"secret\":\"{}\",\"mode\":\"workspace-furnishing-036-reopen-v1\"}},\"hayesOwnerChannel\":{{\"version\":1,\"runtimeGeneration\":\"{GENERATION}\",\"secret\":\"{}\",\"nativeOwnerId\":\"{}\",\"runtimeManifestSha256\":\"{MANIFEST}\"}}}}", ordinary.secret, hayes.socket.secret, hayes.pin.native_owner_id())));
}

#[test]
fn dedicated_ack_requires_exact_bytes_and_consumes_once() {
    for altered in [false, true] {
        let auth = authority();
        let mut setup = HayesSetup::new(pin()).unwrap();
        let fd: std::os::fd::OwnedFd = setup
            .socket
            .child
            .as_ref()
            .unwrap()
            .try_clone()
            .unwrap()
            .into();
        let mut peer = UnixStream::from(fd);
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let ack = format!(
            "{{\"version\":1,\"kind\":\"hayes-owner-hello-ack\",\"workspaceId\":\"{}\",\"tenantId\":\"{}\",\"principalId\":\"{}\",\"runtimeGeneration\":\"{GENERATION}\",\"nativeOwnerId\":\"{}\",\"runtimeManifestSha256\":\"{MANIFEST}\"}}",
            auth.workspace_id, auth.tenant_id, auth.principal_id, setup.pin.native_owner_id(),
        );
        setup.close_child();
        assert!(setup.child_fd().is_err());
        let child = std::thread::spawn(move || {
            let mut header = [0; 4];
            peer.read_exact(&mut header).unwrap();
            let mut hello = vec![0; u32::from_be_bytes(header) as usize];
            peer.read_exact(&mut hello).unwrap();
            let hello: serde_json::Value = serde_json::from_slice(&hello).unwrap();
            assert_eq!(hello["kind"], "hayes-owner-hello");
            let ack = if altered { format!(" {ack}") } else { ack };
            peer.write_all(&(ack.len() as u32).to_be_bytes()).unwrap();
            peer.write_all(ack.as_bytes()).unwrap();
        });
        let result = setup.handshake(&auth, GENERATION, &AtomicBool::new(false));
        child.join().unwrap();
        if altered {
            assert!(result.is_err());
        } else {
            let mut state = HayesBootstrap::Issued(result.unwrap());
            assert!(state.consume().is_ok());
            assert!(state.consume().is_err());
        }
    }
    assert!(HayesBootstrap::Disabled.consume().is_err());
}

#[test]
#[ignore = "requires the exact Platform scripts and a local Node binary"]
fn platform_inherited_descriptors_interoperate() {
    let scripts = PathBuf::from(std::env::var_os("EXHIBITOS_TEST_HAYES_PLATFORM_SCRIPTS").unwrap());
    let node = std::env::var_os("EXHIBITOS_TEST_HAYES_NODE").unwrap();
    struct TestRoot(PathBuf);
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!("hayes-owner-fixture-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let root = TestRoot(fs::canonicalize(root).unwrap());
    let lifetime = WriterLifetime::acquire(&root.0).unwrap();
    let mut setup = Setup::new()
        .unwrap()
        .with_furnishing_writer(lifetime.duplicate_for_writer().unwrap())
        .with_hayes(pin())
        .unwrap();
    let auth = authority();
    let mut command = Command::new(node);
    command.current_dir(scripts).args(["--input-type=module", "-e", r#"
import { fstatSync } from 'node:fs';
import { readNativeWorkspaceChannel } from './native-workspace-channel.mjs';
import { unpackNativeOwnerInput, openNativeOwnerChannel } from './native-owner-channel.mjs';
import { openHayesOwnerChannel } from './native-hayes-owner-channel.mjs';

const { workspace, ownerChannel, hayesOwnerChannel } = unpackNativeOwnerInput(await readNativeWorkspaceChannel());
const ordinaryFd = process.env.EXHIBITOS_NATIVE_OWNER_FD;
const writerFd = process.env.EXHIBITOS_NATIVE_FURNISHING_WRITER_FD;
const hayesFd = process.env.EXHIBITOS_NATIVE_HAYES_OWNER_FD;
if (new Set([ordinaryFd, writerFd, hayesFd]).size !== 3 || !fstatSync(Number(writerFd)).isFile()) process.exit(1);
const [ordinary, hayes] = await Promise.all([
  openNativeOwnerChannel(ordinaryFd, workspace, ownerChannel),
  openHayesOwnerChannel(hayesFd, workspace, hayesOwnerChannel),
]);
await hayes.whenClosed();
await ordinary.send(Buffer.from('ordinary-after-hayes'));
await ordinary.close();
"#]).stdin(Stdio::piped());
    setup.configure(&mut command).unwrap();
    let mut input = setup
        .workspace_furnishing_bootstrap(&auth, GENERATION, FurnishingStart::Initialize)
        .unwrap();
    let mut child = command.spawn().unwrap();
    setup.close_child();
    let written = child.stdin.take().unwrap().write_all(&input);
    input.fill(0);
    written.unwrap();

    let dedicated = setup.take_hayes().unwrap();
    let cancellation = AtomicBool::new(false);
    let mut ordinary: Connection = setup.handshake(&auth, GENERATION, &cancellation).unwrap();
    let mut hayes = dedicated
        .handshake(&auth, GENERATION, &cancellation)
        .unwrap();
    assert_eq!(hayes.pin.host_manifest_sha256(), MANIFEST);
    hayes.channel.claim_hayes().unwrap();
    hayes.channel.invalidate();
    let reply = ordinary
        .read_frame(
            Instant::now() + Duration::from_secs(5),
            &cancellation,
            &mut || Ok(()),
        )
        .unwrap();
    assert_eq!(reply, b"ordinary-after-hayes");
    assert!(child.wait().unwrap().success());
}
