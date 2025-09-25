class UpdaterError extends Error {
  constructor(message, code = 'UPDATER_ERROR') {
    super(message);
    this.name = 'UpdaterError';
    this.code = code;
  }
}

function normalizeSlot(slot) {
  if (slot === 'slotA' || slot === 'SlotA') {
    return 'slotA';
  }
  if (slot === 'slotB' || slot === 'SlotB') {
    return 'slotB';
  }
  throw new UpdaterError(`Unknown slot ${slot}`, 'INVALID_SLOT');
}

function otherSlot(slot) {
  return slot === 'slotA' ? 'slotB' : 'slotA';
}

function normalizeVersion(version) {
  if (typeof version !== 'string') {
    throw new UpdaterError('version must be a string', 'INVALID_VERSION');
  }
  const trimmed = version.trim();
  if (!trimmed) {
    throw new UpdaterError('version must be a non-empty string', 'INVALID_VERSION');
  }
  return trimmed;
}

class UpdaterStateMachine {
  constructor(options = {}) {
    this.healthFailWindow = Number.isInteger(options.healthFailWindow) && options.healthFailWindow > 0
      ? options.healthFailWindow
      : 3;

    const initialActive = options.activeSlot ? normalizeSlot(options.activeSlot) : 'slotA';
    this.slots = {
      slotA: { version: options.slotA?.version ?? null },
      slotB: { version: options.slotB?.version ?? null }
    };

    this.activeSlot = initialActive;
    this.pendingSlot = null;
    this.trialSlot = null;
    this.previousActiveSlot = null;
    this.bootCount = 0;
    this.unhealthyBoots = 0;
    this.lastRollback = null;
  }

  getInactiveSlot() {
    return otherSlot(this.activeSlot);
  }

  check() {
    const inactiveSlot = this.getInactiveSlot();
    return {
      activeSlot: this.activeSlot,
      activeVersion: this.slots[this.activeSlot].version,
      inactiveSlot,
      inactiveVersion: this.slots[inactiveSlot].version,
      stagedSlot: this.pendingSlot,
      stagedVersion: this.pendingSlot ? this.slots[this.pendingSlot].version : null,
      trialSlot: this.trialSlot,
      trialVersion: this.trialSlot ? this.slots[this.trialSlot].version : null,
      rollbackSlot: this.previousActiveSlot,
      rollbackVersion: this.previousActiveSlot ? this.slots[this.previousActiveSlot].version : null,
      bootCount: this.bootCount,
      unhealthyBoots: this.unhealthyBoots,
      healthFailWindow: this.healthFailWindow,
      lastRollback: this.lastRollback
    };
  }

  stage(version) {
    if (this.trialSlot) {
      throw new UpdaterError('cannot stage while an update is pending commitment', 'UPDATE_IN_PROGRESS');
    }
    if (this.pendingSlot) {
      throw new UpdaterError('an update is already staged', 'UPDATE_ALREADY_STAGED');
    }
    const normalizedVersion = normalizeVersion(version);
    const targetSlot = this.getInactiveSlot();
    this.slots[targetSlot].version = normalizedVersion;
    this.pendingSlot = targetSlot;
    this.bootCount = 0;
    this.unhealthyBoots = 0;
    this.lastRollback = null;
    return { status: 'staged', state: this.check() };
  }

  commit() {
    if (this.pendingSlot) {
      const newActive = this.pendingSlot;
      const previous = this.activeSlot;
      this.activeSlot = newActive;
      this.pendingSlot = null;
      this.trialSlot = newActive;
      this.previousActiveSlot = previous;
      this.bootCount = 0;
      this.unhealthyBoots = 0;
      return { status: 'activated', state: this.check() };
    }
    if (this.trialSlot) {
      this.previousActiveSlot = null;
      this.trialSlot = null;
      this.bootCount = 0;
      this.unhealthyBoots = 0;
      return { status: 'committed', state: this.check() };
    }
    return { status: 'noop', state: this.check() };
  }

  markUnhealthy() {
    this.bootCount += 1;
    let rolledBack = false;
    if (this.trialSlot) {
      this.unhealthyBoots += 1;
      if (this.unhealthyBoots >= this.healthFailWindow && this.previousActiveSlot) {
        const failedSlot = this.activeSlot;
        const fallbackSlot = this.previousActiveSlot;
        this.activeSlot = fallbackSlot;
        this.trialSlot = null;
        this.previousActiveSlot = null;
        this.pendingSlot = null;
        this.bootCount = 0;
        this.unhealthyBoots = 0;
        this.lastRollback = {
          fromSlot: failedSlot,
          toSlot: fallbackSlot,
          failedVersion: this.slots[failedSlot].version,
          restoredVersion: this.slots[fallbackSlot].version
        };
        rolledBack = true;
      }
    }
    return { status: rolledBack ? 'rolled_back' : 'recorded', rolledBack, state: this.check() };
  }
}

module.exports = {
  UpdaterStateMachine,
  UpdaterError
};
