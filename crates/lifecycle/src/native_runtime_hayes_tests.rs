// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::{
    env,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) struct Fixture(PathBuf);

impl Fixture {
    pub(super) fn new() -> Self {
        let path = env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("hayes-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path.join(LOCK))
            .unwrap();
        Self(path)
    }

    pub(super) fn authority(&self) -> Authority {
        Authority {
            uid: effective_uid(),
            root: Physical::from(&fs::metadata(&self.0).unwrap()),
            lock: Physical::from(&fs::metadata(self.0.join(LOCK)).unwrap()),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }

    fn acquire(&self) -> Lease {
        Lease::acquire(&self.0, self.authority()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn binding() -> Binding {
    Binding {
        attempt: Uuid::new_v4(),
        generation: Uuid::new_v4(),
        owner: Uuid::new_v4(),
        job: Uuid::new_v4(),
    }
}

fn code<T>(result: Result<T>) -> String {
    result.err().expect("expected failure").code
}

#[test]
fn requires_exact_preprovisioned_namespace() {
    let root = Fixture::new();
    let authority = root.authority();
    let mut wrong_owner = authority;
    wrong_owner.uid ^= 1;
    assert_eq!(
        code(Lease::acquire(&root.0, wrong_owner)),
        "HAYES_CUSTODY_UNAVAILABLE"
    );
    assert_eq!(
        code(Lease::acquire(&Fixture::new().0, authority)),
        "HAYES_CUSTODY_UNAVAILABLE"
    );

    fs::set_permissions(root.0.join(LOCK), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        code(Lease::acquire(&root.0, authority)),
        "HAYES_CUSTODY_UNAVAILABLE"
    );
    fs::rename(root.0.join(LOCK), root.0.join("old-lock")).unwrap();
    assert_eq!(
        code(Lease::acquire(&root.0, authority)),
        "HAYES_CUSTODY_UNAVAILABLE"
    );
    assert!(!root.0.join(LOCK).exists());
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(root.0.join(LOCK))
        .unwrap();
    assert_eq!(
        code(Lease::acquire(&root.0, authority)),
        "HAYES_CUSTODY_UNAVAILABLE"
    );
}

#[test]
fn writes_terminal_before_retiring_only_an_unstarted_empty_attempt() {
    let root = Fixture::new();
    let attempt = root.acquire().begin(binding()).unwrap();
    let terminal = attempt.journal.path.join("terminal.json");
    attempt.retire_unstarted().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(terminal).unwrap()).unwrap()
            ["outcome"],
        "aborted-before-launch"
    );
    assert!(!root.0.join(ACTIVE).exists());

    let mut attempt = root.acquire().begin(binding()).unwrap();
    assert_eq!(
        code(attempt.before_spawn(Role::Validate)),
        "HAYES_DRAIN_UNCONFIRMED"
    );
    let stage = attempt.stage.path.join("output");
    fs::write(&stage, b"preserve").unwrap();
    let terminal = attempt.journal.path.join("terminal.json");
    assert_eq!(code(attempt.retire_unstarted()), "HAYES_DRAIN_UNCONFIRMED");
    assert!(!terminal.exists());
    assert_eq!(fs::read(stage).unwrap(), b"preserve");
    assert_eq!(
        code(root.acquire().begin(binding())),
        "HAYES_RESERVATION_RETAINED"
    );
}

#[test]
fn exact_child_registers_once_and_receives_one_go() {
    let root = Fixture::new();
    let mut attempt = root.acquire().begin(binding()).unwrap();
    let mut worker = spawn_channel_fixture(&mut attempt, &root.0, "valid");
    assert_eq!(
        code(attempt.send_go(&mut worker)),
        "HAYES_WORKER_CHANNEL_UNAVAILABLE"
    );
    assert_eq!(
        code(attempt.before_spawn(Role::Generate)),
        "HAYES_WORKER_ALREADY_LAUNCHED"
    );
    assert_eq!(
        code(attempt.before_spawn(Role::Validate)),
        "HAYES_DRAIN_UNCONFIRMED"
    );

    attempt.register_ready(&mut worker).unwrap();
    let registered: Registration = attempt.journal.read(Role::Generate.files().1).unwrap();
    assert_eq!(registered.worker, worker.worker);
    assert_eq!(registered.child_pid, worker.diagnostic_pid());
    assert!(!root.0.join("child-go").exists());
    assert_eq!(
        code(attempt.register_ready(&mut worker)),
        "HAYES_WORKER_CHANNEL_UNAVAILABLE"
    );
    attempt.send_go(&mut worker).unwrap();
    assert_eq!(
        code(attempt.send_go(&mut worker)),
        "HAYES_WORKER_CHANNEL_UNAVAILABLE"
    );
    wait_for(|| root.0.join("child-go").is_file());
    fs::write(root.0.join("release-0"), b"").unwrap();
    wait_for(|| worker.try_wait().unwrap().is_some());
    assert!(worker.try_wait().unwrap().unwrap().success());
    assert_eq!(code(attempt.retire_unstarted()), "HAYES_DRAIN_UNCONFIRMED");
}

#[test]
fn wrong_nonce_extra_frames_or_cross_attempt_child_never_receive_go() {
    for mode in ["wrong-nonce", "duplicate", "foreign-attempt"] {
        let root = Fixture::new();
        let mut attempt = root.acquire().begin(binding()).unwrap();
        let mut worker = spawn_channel_fixture(&mut attempt, &root.0, mode);
        if mode == "foreign-attempt" {
            let foreign = Fixture::new();
            let mut foreign_attempt = foreign.acquire().begin(binding()).unwrap();
            assert_eq!(
                code(foreign_attempt.register_ready(&mut worker)),
                "HAYES_READY_INVALID"
            );
            assert_eq!(
                code(attempt.register_ready(&mut worker)),
                "HAYES_WORKER_CHANNEL_UNAVAILABLE"
            );
        } else {
            assert_eq!(
                code(attempt.register_ready(&mut worker)),
                "HAYES_READY_INVALID"
            );
        }
        wait_for(|| worker.try_wait().unwrap().is_some());
        assert!(worker.try_wait().unwrap().unwrap().success());
        assert!(!root.0.join("child-go").exists());
        assert!(!attempt.journal.path.join(Role::Generate.files().1).exists());
        assert_eq!(code(attempt.retire_unstarted()), "HAYES_DRAIN_UNCONFIRMED");
    }
}

#[test]
fn restart_preserves_partial_intent_and_rejects_physical_and_generation_changes() {
    let root = Fixture::new();
    let expected = binding();
    let mut attempt = root.acquire().begin(expected).unwrap();
    // Simulate interruption after possible spawn and before READY.
    drop(attempt.before_spawn(Role::Generate).unwrap());
    let stage = attempt.stage.path.clone();
    drop(attempt);
    let lease = root.acquire();
    assert_eq!(
        code(lease.check_interrupted(expected)),
        "HAYES_DRAIN_UNCONFIRMED"
    );
    assert_eq!(
        code(lease.check_interrupted(Binding {
            generation: Uuid::new_v4(),
            ..expected
        })),
        "HAYES_CUSTODY_UNAVAILABLE"
    );
    fs::rename(&stage, stage.with_extension("old")).unwrap();
    fs::DirBuilder::new().mode(0o700).create(stage).unwrap();
    assert_eq!(
        code(lease.check_interrupted(expected)),
        "HAYES_CUSTODY_UNAVAILABLE"
    );

    let root = Fixture::new();
    fs::write(root.0.join(ACTIVE), b"{\"torn").unwrap();
    fs::set_permissions(root.0.join(ACTIVE), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        code(root.acquire().check_interrupted(expected)),
        "HAYES_CUSTODY_UNAVAILABLE"
    );
    assert_eq!(
        code(root.acquire().begin(binding())),
        "HAYES_RESERVATION_RETAINED"
    );
}

fn child_command(name: &str, root: &Path) -> Command {
    let mut cmd = Command::new(env::current_exe().unwrap());
    cmd.args([
        "--ignored",
        "--exact",
        &format!("native_runtime::hayes::tests::{name}"),
    ])
    .env("HAYES_TEST_ROOT", root)
    .stdin(Stdio::null())
    .stdout(Stdio::null());
    cmd
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !condition() {
        assert!(Instant::now() < deadline, "Hayes fixture timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn crashed_parent_retains_lock_until_both_exec_holders_close_then_retains_output() {
    for send_go in [false, true] {
        let root = Fixture::new();
        let mut parent = child_command("crash_parent_fixture", &root.0)
            .env("HAYES_TEST_GO", if send_go { "yes" } else { "no" })
            .spawn()
            .unwrap();
        let mut status = None;
        wait_for(|| {
            status = parent.try_wait().unwrap();
            status.is_some()
        });
        assert_eq!(status.unwrap().code(), Some(42));
        assert_eq!(
            code(Lease::acquire(&root.0, root.authority())),
            "HAYES_RESERVATION_BUSY"
        );

        fs::write(root.0.join("release-0"), b"").unwrap();
        wait_for(|| root.0.join("closed-0").is_file());
        assert_eq!(
            code(Lease::acquire(&root.0, root.authority())),
            "HAYES_RESERVATION_BUSY"
        );
        fs::write(root.0.join("release-1"), b"").unwrap();

        let mut successor = None;
        wait_for(|| {
            match Lease::acquire(&root.0, root.authority()) {
                Ok(lease) => successor = Some(lease),
                Err(error) => assert_eq!(error.code, "HAYES_RESERVATION_BUSY"),
            }
            successor.is_some()
        });
        let lease = successor.unwrap();
        let intent: Intent = lease.root.read(ACTIVE).unwrap();
        assert_eq!(
            code(lease.check_interrupted(intent.binding)),
            "HAYES_DRAIN_UNCONFIRMED"
        );
        let journal = root.0.join(intent.binding.attempt.to_string());
        assert!(journal.join(Role::Generate.files().1).is_file());
        let stage_output = journal.join("stage/synthetic");
        if send_go {
            assert_eq!(fs::read(&stage_output).unwrap(), b"pending custody");
        } else {
            assert!(!stage_output.exists());
        }
        assert!(!journal.join("terminal.json").exists());
        assert_eq!(code(lease.begin(binding())), "HAYES_RESERVATION_RETAINED");
    }
}

// Parent and holders are actual execs in the queue's process group. The second
// holder models a descendant with the same open description. Fixture signals
// are private files so the holders can outlive their synthetic parent.
#[test]
#[ignore]
fn crash_parent_fixture() {
    let path = PathBuf::from(env::var_os("HAYES_TEST_ROOT").unwrap());
    let authority = Authority {
        uid: effective_uid(),
        root: Physical::from(&fs::metadata(&path).unwrap()),
        lock: Physical::from(&fs::metadata(path.join(LOCK)).unwrap()),
    };
    let mut attempt = Lease::acquire(&path, authority)
        .unwrap()
        .begin(binding())
        .unwrap();
    let holder_fd = attempt.lease.lock.try_clone().unwrap();
    let fd = holder_fd.as_raw_fd();
    let mut holder = child_command("lock_holder_fixture", &path);
    holder.env("HAYES_TEST_FD", fd.to_string());
    unsafe {
        holder.pre_exec(move || cvt(libc::fcntl(fd, libc::F_SETFD, 0)).map(|_| ()));
    }
    holder.spawn().unwrap();
    let mut worker = spawn_channel_fixture(&mut attempt, &path, "crash");
    wait_for(|| path.join("ready-1").is_file());
    attempt.register_ready(&mut worker).unwrap();
    if env::var("HAYES_TEST_GO").unwrap() == "yes" {
        attempt.send_go(&mut worker).unwrap();
        wait_for(|| attempt.stage.path.join("synthetic").is_file());
    }
    // Bypass Rust destructors to model loss of the owner before retirement.
    std::process::exit(42);
}

fn spawn_channel_fixture(attempt: &mut Attempt, path: &Path, mode: &str) -> SpawnedWorker {
    use std::ffi::OsStr;
    let exe = env::current_exe().unwrap();
    let permit = attempt.before_spawn(Role::Generate).unwrap();
    // The shell only maps test-only argv to environment before execing libtest.
    // The permit appends the same identity and nonce arguments as the Node ABI.
    permit.spawn_fixture(Path::new("/bin/sh"), &[
        OsStr::new("-c"),
        OsStr::new("HAYES_TEST_ROOT=$2 HAYES_TEST_STAGE=$3 HAYES_TEST_MODE=$4 HAYES_TEST_LOCK=$5 HAYES_TEST_NONCE=$6 exec \"$1\" --ignored --exact native_runtime::hayes::tests::channel_child_fixture"),
        OsStr::new("hayes-test"),
        exe.as_os_str(),
        path.as_os_str(),
        attempt.stage.path.as_os_str(),
        OsStr::new(mode),
    ]).unwrap()
}

#[test]
#[ignore]
fn channel_child_fixture() {
    use std::os::unix::net::UnixStream;
    let path = PathBuf::from(env::var_os("HAYES_TEST_ROOT").unwrap());
    let mode = env::var("HAYES_TEST_MODE").unwrap();
    let lock = unsafe { File::from_raw_fd(4) };
    let identity = private_file(&lock).unwrap();
    assert_eq!(
        format!("{}:{}", identity.dev, identity.ino),
        env::var("HAYES_TEST_LOCK").unwrap()
    );
    let nonce = env::var("HAYES_TEST_NONCE").unwrap();
    let mut owner = unsafe { UnixStream::from_raw_fd(3) };
    owner
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let frame = match mode.as_str() {
        "wrong-nonce" => "READY 00000000000000000000000000000000\n".to_owned(),
        "duplicate" => format!("READY {nonce}\nREADY {nonce}\n"),
        _ => format!("READY {nonce}\n"),
    };
    if mode == "foreign-attempt" {
        // Parent burns the channel without accepting a frame from this child.
        assert_eq!(owner.read(&mut [0]).unwrap(), 0);
        return;
    }
    owner.write_all(frame.as_bytes()).unwrap();
    let expected = format!("GO {nonce}\n");
    let mut received = vec![0; expected.len()];
    match owner.read_exact(&mut received) {
        Ok(()) => {
            assert!(mode == "valid" || mode == "crash");
            assert_eq!(received, expected.as_bytes());
            let output = PathBuf::from(env::var_os("HAYES_TEST_STAGE").unwrap()).join("synthetic");
            fs::write(output, b"pending custody").unwrap();
            fs::write(path.join("child-go"), b"").unwrap();
        }
        Err(error) => {
            // Some kernels reset the socket when the parent closes on extra
            // unread bytes. Either closure must leave the child without GO.
            assert!(matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
            ));
            assert_ne!(mode, "valid");
        }
    }
    if mode == "valid" || mode == "crash" {
        wait_for(|| path.join("release-0").is_file());
    }
    drop(lock);
    fs::write(path.join("closed-0"), b"").unwrap();
}

#[test]
#[ignore]
fn lock_holder_fixture() {
    let path = PathBuf::from(env::var_os("HAYES_TEST_ROOT").unwrap());
    let fd = env::var("HAYES_TEST_FD").unwrap().parse().unwrap();
    let lock = unsafe { File::from_raw_fd(fd) };
    private_file(&lock).unwrap();
    fs::write(path.join("ready-1"), b"").unwrap();
    wait_for(|| path.join("release-1").is_file());
}
