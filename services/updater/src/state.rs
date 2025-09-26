use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub const ALL: [Slot; 2] = [Slot::A, Slot::B];

    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SlotState {
    Inactive,
    Staged,
    Booting,
    Active,
    Bad,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotInfo {
    pub state: SlotState,
    pub artifact: Option<String>,
    pub generation: u64,
}

impl SlotInfo {
    fn new(state: SlotState) -> Self {
        Self {
            state,
            artifact: None,
            generation: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdaterState {
    pub generation: u64,
    pub active: Option<Slot>,
    pub previous_active: Option<Slot>,
    pub staging: Option<Slot>,
    pub last_failed: Option<Slot>,
    pub slots: BTreeMap<Slot, SlotInfo>,
}

impl Default for UpdaterState {
    fn default() -> Self {
        let mut slots = BTreeMap::new();
        slots.insert(Slot::A, SlotInfo::new(SlotState::Active));
        slots.insert(Slot::B, SlotInfo::new(SlotState::Inactive));
        Self {
            generation: 0,
            active: Some(Slot::A),
            previous_active: None,
            staging: None,
            last_failed: None,
            slots,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StageError {
    #[error("no slot available for staging")]
    NoAvailableSlot,
    #[error("slot is currently booting and cannot be restaged")]
    SlotBooting,
    #[error("target slot {0:?} is not available for staging")]
    TargetSlotUnavailable(Slot),
    #[error("staged slot {expected:?} does not match manifest target {requested:?}")]
    TargetSlotMismatch { expected: Slot, requested: Slot },
    #[error("failed to validate bundle: {0}")]
    InvalidBundle(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error("no artifact is staged for commit")]
    NothingStaged,
    #[error("staging slot is not ready for commit")]
    InvalidStageState,
}

#[derive(Debug, thiserror::Error)]
pub enum RollbackError {
    #[error("no previous active slot recorded for rollback")]
    NoPreviousActive,
    #[error("no failed slot recorded for rollback")]
    NoFailedSlot,
}

impl UpdaterState {
    pub fn stage(&mut self, artifact: String, target: Option<Slot>) -> Result<Slot, StageError> {
        if let Some(slot) = self.staging {
            if let Some(requested) = target {
                if requested != slot {
                    return Err(StageError::TargetSlotMismatch {
                        expected: slot,
                        requested,
                    });
                }
            }
            let info = self
                .slots
                .get_mut(&slot)
                .expect("staging slot must exist in state");
            if info.state == SlotState::Booting {
                return Err(StageError::SlotBooting);
            }

            if info.state == SlotState::Staged && info.artifact.as_deref() == Some(&artifact) {
                return Ok(slot);
            }

            info.state = SlotState::Staged;
            info.artifact = Some(artifact);
            self.generation += 1;
            info.generation = self.generation;
            return Ok(slot);
        }

        let candidate = match target {
            Some(slot) => {
                if !self.is_slot_available_for_stage(slot) {
                    return Err(StageError::TargetSlotUnavailable(slot));
                }
                slot
            }
            None => Slot::ALL
                .into_iter()
                .find(|slot| self.is_slot_available_for_stage(*slot))
                .ok_or(StageError::NoAvailableSlot)?,
        };

        let info = self
            .slots
            .get_mut(&candidate)
            .expect("candidate slot must exist in state");
        info.state = SlotState::Staged;
        info.artifact = Some(artifact);
        self.generation += 1;
        info.generation = self.generation;
        self.staging = Some(candidate);

        Ok(candidate)
    }

    fn is_slot_available_for_stage(&self, slot: Slot) -> bool {
        if self.active == Some(slot) && self.slots[&slot].state == SlotState::Active {
            return false;
        }

        matches!(
            self.slots[&slot].state,
            SlotState::Inactive | SlotState::Bad | SlotState::Staged
        )
    }

    pub fn begin_commit(&mut self) -> Result<Slot, CommitError> {
        let slot = self.staging.ok_or(CommitError::NothingStaged)?;
        let info = self
            .slots
            .get_mut(&slot)
            .expect("staging slot must exist in state");

        match info.state {
            SlotState::Staged => {
                info.state = SlotState::Booting;
                Ok(slot)
            }
            SlotState::Booting => Ok(slot),
            _ => Err(CommitError::InvalidStageState),
        }
    }

    pub fn finalize_commit(&mut self, slot: Slot) {
        let previous_active = self.active;
        if let Some(prev) = previous_active {
            if prev != slot {
                if let Some(info) = self.slots.get_mut(&prev) {
                    info.state = SlotState::Inactive;
                }
            }
        }

        if let Some(info) = self.slots.get_mut(&slot) {
            info.state = SlotState::Active;
        }

        self.previous_active = previous_active.filter(|prev| *prev != slot);
        self.active = Some(slot);
        self.staging = None;
        self.last_failed = None;
    }

    pub fn fail_commit(&mut self, slot: Slot) {
        if let Some(info) = self.slots.get_mut(&slot) {
            info.state = SlotState::Bad;
        }
        self.last_failed = Some(slot);
        self.staging = None;
    }

    pub fn mark_active_bad(&mut self) -> Option<Slot> {
        let active = self.active?;
        let info = self
            .slots
            .get_mut(&active)
            .expect("active slot must exist in state");

        if info.state != SlotState::Active {
            return None;
        }

        info.state = SlotState::Bad;
        self.active = None;
        self.last_failed = Some(active);
        Some(active)
    }

    pub fn rollback(&mut self) -> Result<Slot, RollbackError> {
        let previous_active = self
            .previous_active
            .ok_or(RollbackError::NoPreviousActive)?;
        let failed = self.last_failed.ok_or(RollbackError::NoFailedSlot)?;

        if let Some(info) = self.slots.get_mut(&previous_active) {
            info.state = SlotState::Active;
        }
        if let Some(info) = self.slots.get_mut(&failed) {
            if info.state == SlotState::Bad {
                info.state = SlotState::Inactive;
            }
        }

        self.active = Some(previous_active);
        self.previous_active = None;
        self.last_failed = None;
        self.staging = None;

        Ok(previous_active)
    }
}
