// SPDX-License-Identifier: Apache-2.0
//! Dedicated inherited Hayes bootstrap within an ordinary selected Session.

use super::{
    err, read_bounded, write_bounded, Connection, FreshOwnerBinding, LocalAuthority, Result, Setup, LIMIT,
};
use crate::{hash_valid, native_runtime::FurnishingStart};
use uuid::Uuid;
use serde::Serialize;
use std::{
    os::fd::AsRawFd,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

/// Native-selected identity issued before the API sends OPEN. The caller
/// must convey it through the existing authenticated private bootstrap.
pub(in super::super) struct HayesSessionPin {
    runtime_generation: Uuid,
    host_manifest_sha256: String,
    native_owner_id: Uuid,
}

impl HayesSessionPin {
    pub(in super::super) fn new(
        runtime_generation: Uuid,
        host_manifest_sha256: &str,
    ) -> Result<Self> {
        if runtime_generation.get_variant() != uuid::Variant::RFC4122
            || !(1..=8).contains(&runtime_generation.get_version_num())
            || !hash_valid(host_manifest_sha256)
        {
            return Err(unavailable());
        }
        Ok(Self {
            runtime_generation,
            host_manifest_sha256: host_manifest_sha256.into(),
            native_owner_id: Uuid::new_v4(),
        })
    }

    pub(in super::super) fn runtime_generation(&self) -> Uuid {
        self.runtime_generation
    }

    pub(in super::super) fn host_manifest_sha256(&self) -> &str {
        &self.host_manifest_sha256
    }

    pub(in super::super) fn native_owner_id(&self) -> Uuid {
        self.native_owner_id
    }
}

pub(in super::super) struct AuthenticatedHayes {
    #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
    pub(in super::super) pin: HayesSessionPin,
    pub(in super::super) channel: Connection,
}

pub(in super::super) struct HayesSetup {
    socket: Box<Setup>,
    pin: HayesSessionPin,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap<'a> {
    workspace: &'a LocalAuthority,
    owner_channel: FreshOwnerBinding<'a>,
    hayes_owner_channel: Pin<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Pin<'a> {
    version: u8,
    runtime_generation: &'a str,
    secret: &'a str,
    native_owner_id: uuid::Uuid,
    runtime_manifest_sha256: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Hello<'a> {
    version: u8,
    kind: &'static str,
    workspace_id: &'a str,
    tenant_id: &'a str,
    principal_id: &'a str,
    runtime_generation: &'a str,
    secret: &'a str,
    native_owner_id: uuid::Uuid,
    runtime_manifest_sha256: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Ack<'a> {
    version: u8,
    kind: &'static str,
    workspace_id: &'a str,
    tenant_id: &'a str,
    principal_id: &'a str,
    runtime_generation: &'a str,
    native_owner_id: uuid::Uuid,
    runtime_manifest_sha256: &'a str,
}

impl HayesSetup {
    pub(super) fn new(pin: HayesSessionPin) -> Result<Self> {
        Ok(Self {
            socket: Box::new(Setup::new()?),
            pin,
        })
    }

    pub(super) fn child_fd(&self) -> Result<i32> {
        self.socket
            .child
            .as_ref()
            .map(AsRawFd::as_raw_fd)
            .ok_or_else(unavailable)
    }

    pub(super) fn close_child(&mut self) {
        self.socket.close_child();
    }

    pub(super) fn bootstrap(
        &self,
        auth: &LocalAuthority,
        generation: &str,
        start: FurnishingStart,
        ordinary_secret: &str,
    ) -> Result<Vec<u8>> {
        if !matches!(start, FurnishingStart::Initialize | FurnishingStart::Reopen | FurnishingStart::Upgrade040To044 | FurnishingStart::Upgrade044To045)
            || self.pin.runtime_generation().to_string() != generation
            || ordinary_secret == self.socket.secret
        {
            return Err(unavailable());
        }
        let mut bytes = serde_json::to_vec(&Bootstrap {
            workspace: auth,
            owner_channel: FreshOwnerBinding {
                version: 3,
                runtime_generation: generation,
                secret: ordinary_secret,
                mode: start.owner_mode(),
            },
            hayes_owner_channel: Pin {
                version: 1,
                runtime_generation: generation,
                secret: &self.socket.secret,
                native_owner_id: self.pin.native_owner_id(),
                runtime_manifest_sha256: self.pin.host_manifest_sha256(),
            },
        })
        .map_err(|_| unavailable())?;
        if bytes.len() > 4096 {
            bytes.fill(0);
            return Err(unavailable());
        }
        Ok(bytes)
    }

    pub(in super::super) fn handshake(
        mut self,
        auth: &LocalAuthority,
        generation: &str,
        cancellation: &AtomicBool,
    ) -> Result<AuthenticatedHayes> {
        if self.pin.runtime_generation().to_string() != generation || self.socket.child.is_some() {
            return Err(unavailable());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut hello = serde_json::to_vec(&Hello {
            version: 1,
            kind: "hayes-owner-hello",
            workspace_id: &auth.workspace_id,
            tenant_id: &auth.tenant_id,
            principal_id: &auth.principal_id,
            runtime_generation: generation,
            secret: &self.socket.secret,
            native_owner_id: self.pin.native_owner_id(),
            runtime_manifest_sha256: self.pin.host_manifest_sha256(),
        })
        .map_err(|_| unavailable())?;
        let mut check = || Ok(());
        let sent = (|| {
            if hello.is_empty() || hello.len() > LIMIT {
                return Err(unavailable());
            }
            write_bounded(
                &mut self.socket.parent,
                &(hello.len() as u32).to_be_bytes(),
                deadline,
                cancellation,
                &mut check,
            )?;
            write_bounded(
                &mut self.socket.parent,
                &hello,
                deadline,
                cancellation,
                &mut check,
            )
        })();
        hello.fill(0);
        sent?;

        let mut prefix = [0; 4];
        read_bounded(
            &mut self.socket.parent,
            &mut prefix,
            deadline,
            cancellation,
            &mut check,
        )?;
        let size = u32::from_be_bytes(prefix) as usize;
        if !(1..=LIMIT).contains(&size) {
            return Err(unavailable());
        }
        let mut raw = vec![0; size];
        read_bounded(
            &mut self.socket.parent,
            &mut raw,
            deadline,
            cancellation,
            &mut check,
        )?;
        let expected = serde_json::to_vec(&Ack {
            version: 1,
            kind: "hayes-owner-hello-ack",
            workspace_id: &auth.workspace_id,
            tenant_id: &auth.tenant_id,
            principal_id: &auth.principal_id,
            runtime_generation: generation,
            native_owner_id: self.pin.native_owner_id(),
            runtime_manifest_sha256: self.pin.host_manifest_sha256(),
        })
        .map_err(|_| unavailable())?;
        if raw != expected {
            return Err(unavailable());
        }
        let channel = self.socket.authenticated_connection()?;
        Ok(AuthenticatedHayes {
            #[cfg(all(feature = "workspace-v5-owner-validation", debug_assertions))]
            pin: self.pin,
            channel,
        })
    }
}

fn unavailable() -> crate::LifecycleError {
    err("HAYES_OPERATION_UNAVAILABLE")
}

#[cfg(all(test, feature = "workspace-v5-owner-validation", debug_assertions))]
#[path = "native_runtime_hayes_owner_bootstrap_tests.rs"]
mod tests;
