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

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
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

    fn authority(&self) -> Authority {
        Authority {
            uid: effective_uid(),
            root: Physical::from(&fs::metadata(&self.0).unwrap()),
            lock: Physical::from(&fs::metadata(self.0.join(LOCK)).unwrap()),
        }
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
fn wrong_or_duplicate_ready_cannot_authorize_work() {
    let root = Fixture::new();
    let mut attempt = root.acquire().begin(binding()).unwrap();
    let launch = attempt.before_spawn(Role::Generate).unwrap();
    let nonce = launch.nonce();
    let replay = InertLaunch {
        worker: launch.worker,
        lock_fd: launch.lock_fd.try_clone().unwrap(),
    };
    let ready = format!("READY {nonce}\n");
    assert_eq!(
        attempt.register_ready(launch, ready.as_bytes()).unwrap(),
        format!("GO {nonce}\n")
    );
    assert_eq!(
        code(attempt.register_ready(replay, ready.as_bytes())),
        "HAYES_CUSTODY_UNAVAILABLE"
    );
    assert_eq!(
        code(attempt.before_spawn(Role::Generate)),
        "HAYES_WORKER_ALREADY_LAUNCHED"
    );
    assert_eq!(
        code(attempt.before_spawn(Role::Validate)),
        "HAYES_DRAIN_UNCONFIRMED"
    );
    assert_eq!(code(attempt.retire_unstarted()), "HAYES_DRAIN_UNCONFIRMED");

    let root = Fixture::new();
    let mut attempt = root.acquire().begin(binding()).unwrap();
    let launch = attempt.before_spawn(Role::Generate).unwrap();
    let registered = attempt.journal.path.join(Role::Generate.files().1);
    assert_eq!(
        code(attempt.register_ready(launch, b"READY foreign\n")),
        "HAYES_READY_INVALID"
    );
    assert!(!registered.exists());
    assert_eq!(code(attempt.retire_unstarted()), "HAYES_DRAIN_UNCONFIRMED");
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
    let launch = attempt.before_spawn(Role::Generate).unwrap();
    let nonce = launch.nonce();
    for index in [0, 1] {
        let fd = launch.lock_fd.as_raw_fd();
        let mut command = child_command("lock_holder_fixture", &path);
        command
            .env("HAYES_TEST_INDEX", index.to_string())
            .env("HAYES_TEST_NONCE", &nonce)
            .env("HAYES_TEST_STAGE", &attempt.stage.path)
            .env("HAYES_TEST_FD", fd.to_string())
            .env("HAYES_TEST_LOCK", launch.lock_identity().unwrap());
        unsafe {
            command.pre_exec(move || cvt(libc::fcntl(fd, libc::F_SETFD, 0)).map(|_| ()));
        }
        command.spawn().unwrap();
    }
    wait_for(|| path.join("ready-1").is_file());
    let ready = path.join("ready-0");
    wait_for(|| fs::read(&ready).is_ok_and(|frame| frame == format!("READY {nonce}\n").as_bytes()));
    let go = attempt
        .register_ready(launch, &fs::read(ready).unwrap())
        .unwrap();
    if env::var("HAYES_TEST_GO").unwrap() == "yes" {
        fs::write(path.join("go"), &go).unwrap();
        wait_for(|| attempt.stage.path.join("synthetic").is_file());
    }
    // Bypass Rust destructors to model loss of the owner before retirement.
    std::process::exit(42);
}

#[test]
#[ignore]
fn lock_holder_fixture() {
    let path = PathBuf::from(env::var_os("HAYES_TEST_ROOT").unwrap());
    let index = env::var("HAYES_TEST_INDEX").unwrap();
    let fd = env::var("HAYES_TEST_FD").unwrap().parse().unwrap();
    let lock = unsafe { File::from_raw_fd(fd) };
    let identity = private_file(&lock).unwrap();
    assert_eq!(
        format!("{}:{}", identity.dev, identity.ino),
        env::var("HAYES_TEST_LOCK").unwrap()
    );
    let nonce = env::var("HAYES_TEST_NONCE").unwrap();
    fs::write(
        path.join(format!("ready-{index}")),
        format!("READY {nonce}\n"),
    )
    .unwrap();
    wait_for(|| {
        if path.join(format!("release-{index}")).is_file() {
            return true;
        }
        if index == "0"
            && fs::read(path.join("go")).is_ok_and(|go| go == format!("GO {nonce}\n").as_bytes())
        {
            let output = PathBuf::from(env::var_os("HAYES_TEST_STAGE").unwrap()).join("synthetic");
            fs::write(output, b"pending custody").unwrap();
        }
        false
    });
    drop(lock);
    if index == "0" {
        fs::write(path.join("closed-0"), b"").unwrap();
    }
}
