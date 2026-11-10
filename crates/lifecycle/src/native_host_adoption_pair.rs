// SPDX-License-Identifier: Apache-2.0
//! A bounded operator session over two existing registered profiles. Both
//! profile leases stay owned until the session closes; only one runtime runs.
use super::*;
use crate::native_runtime::{
    AdoptionProfilePin, check_cold_adoption_profile, model_adoption::Input,
};

pub struct AdoptionPair {
    source: Host,
    target: Host,
    source_pin: AdoptionProfilePin,
    target_pin: AdoptionProfilePin,
    payload: Arc<Payload>,
    at_target: bool,
}

fn unavailable() -> LifecycleError {
    err("NATIVE_MODEL_ADOPTION_PROFILE_PAIR_UNAVAILABLE")
}

impl AdoptionPair {
    pub fn open(
        source_profile: PathBuf,
        source_id: &str,
        target_profile: PathBuf,
        target_id: &str,
        resources: PathBuf,
    ) -> Result<Self> {
        if !source_profile.is_absolute() || !target_profile.is_absolute()
            || source_profile.starts_with(&target_profile) || target_profile.starts_with(&source_profile)
            || !installations::uuid(source_id) || !installations::uuid(target_id) || source_id == target_id
        {
            return Err(unavailable());
        }
        let payload = Arc::new(Payload::open(&resources)?);
        if !payload.native_model_adoption_carried_target() || payload.native_model_adoption_root()?.is_none() {
            return Err(unavailable());
        }
        let open = |profile, id, carried| -> Result<(Host, AdoptionProfilePin)> {
            let controller = Arc::new(InstallationController::open_existing_managed(profile)?);
            let context = controller.context()?;
            if context.active_id != id {
                return Err(unavailable());
            }
            let pin = controller.with_current(&context.selection_token, |service|
                check_cold_adoption_profile(service, &payload, carried))?;
            let host = Host::new(controller, resources.clone());
            host.state.lock().map_err(|_| unavailable())?.payload = Some(payload.clone());
            Ok((host, pin))
        };
        let (source, source_pin) = open(source_profile, source_id, false)?;
        let (target, target_pin) = open(target_profile, target_id, true)?;
        if source_pin.owner.0 == target_pin.owner.0 {
            return Err(unavailable());
        }
        Ok(Self { source, target, source_pin, target_pin, payload, at_target: false })
    }

    pub fn handle(&mut self, request: Request) -> Result<Value> {
        match &request.command {
            UiCommand::NativeModelAdoption { selection_token, adoption } => match adoption.as_ref() {
                Input::SwitchTargetProfile { operation_id } if !self.at_target => {
                    return self.switch_target(selection_token, operation_id);
                }
                Input::StageSource { .. } | Input::ReleasePrecommit { .. } if !self.at_target => (),
                Input::PreviewTarget { .. } | Input::ConfirmTarget { .. }
                | Input::StatusTarget { .. } | Input::SettleTarget { .. } if self.at_target => (),
                Input::RetainedOperation => (),
                _ => return Err(unavailable()),
            },
            UiCommand::Context | UiCommand::NativeStart { .. } | UiCommand::NativeStop { .. }
            | UiCommand::NativeStatus { .. } | UiCommand::DocumentState { .. } => (),
            UiCommand::NativeAuthoringSession { .. } | UiCommand::SaveDocumentState { .. }
            | UiCommand::StaticExport { .. } | UiCommand::StaticExportStatus { .. }
            | UiCommand::StaticExportCancel { .. } if self.at_target => (),
            _ => return Err(unavailable()),
        }
        if self.at_target {
            self.target.handle(request)
        } else {
            self.source.handle(request)
        }
    }

    fn switch_target(&mut self, source_token: &str, operation_id: &str) -> Result<Value> {
        self.source.selected(source_token)?;
        let target = self.target.controller.context()?;
        if self.source.adoption.is_none() {
            return Err(unavailable());
        }
        self.source.detach_stopped()?;
        let source = self.source.controller.with_current(source_token, |service|
            check_cold_adoption_profile(service, &self.payload, false))?;
        let target_pin = self.target.controller.with_current(&target.selection_token, |service|
            check_cold_adoption_profile(service, &self.payload, true))?;
        if !source.same_owner_and_writer(&self.source_pin) || target_pin != self.target_pin {
            return Err(unavailable());
        }
        let document = self.target.controller.native_document(&target.selection_token)?;
        self.source.adoption.as_ref().ok_or_else(unavailable)?
            .check_profile_switch(operation_id, &source, &document)?;
        let context = serde_json::to_value(target).map_err(|_| unavailable())?;
        self.target.adoption = self.source.adoption.take();
        self.at_target = true;
        Ok(context)
    }

    pub fn shutdown(&mut self) {
        self.target.shutdown();
        self.source.shutdown();
    }
}
