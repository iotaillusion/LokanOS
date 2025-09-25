const { UpdaterStateMachine, UpdaterError } = require('../src/state-machine');

describe('UpdaterStateMachine', () => {
  it('stages and activates a new version on the inactive slot', () => {
    const machine = new UpdaterStateMachine({
      slotA: { version: '1.0.0' },
      slotB: { version: null },
      activeSlot: 'slotA',
      healthFailWindow: 2
    });

    const stageResult = machine.stage('2.0.0');
    expect(stageResult.status).toBe('staged');
    expect(stageResult.state.stagedSlot).toBe('slotB');
    expect(stageResult.state.stagedVersion).toBe('2.0.0');

    const commitResult = machine.commit();
    expect(commitResult.status).toBe('activated');
    expect(commitResult.state.activeSlot).toBe('slotB');
    expect(commitResult.state.activeVersion).toBe('2.0.0');
    expect(commitResult.state.rollbackSlot).toBe('slotA');
    expect(commitResult.state.trialSlot).toBe('slotB');

    const finalCommit = machine.commit();
    expect(finalCommit.status).toBe('committed');
    expect(finalCommit.state.rollbackSlot).toBeNull();
    expect(finalCommit.state.trialSlot).toBeNull();
  });

  it('prevents staging when an update is pending commitment', () => {
    const machine = new UpdaterStateMachine({
      slotA: { version: '1.0.0' },
      slotB: { version: null },
      activeSlot: 'slotA',
      healthFailWindow: 2
    });

    machine.stage('2.0.0');
    machine.commit();

    expect(() => machine.stage('3.0.0')).toThrow(UpdaterError);
    expect(() => machine.stage('3.0.0')).toThrow('cannot stage while an update is pending commitment');
  });

  it('rolls back automatically after repeated unhealthy boots', () => {
    const machine = new UpdaterStateMachine({
      slotA: { version: '1.0.0' },
      slotB: { version: null },
      activeSlot: 'slotA',
      healthFailWindow: 2
    });

    machine.stage('2.0.0');
    machine.commit();

    let status = machine.markUnhealthy();
    expect(status.rolledBack).toBe(false);
    expect(status.state.unhealthyBoots).toBe(1);
    expect(status.state.activeSlot).toBe('slotB');

    status = machine.markUnhealthy();
    expect(status.rolledBack).toBe(true);
    expect(status.state.activeSlot).toBe('slotA');
    expect(status.state.activeVersion).toBe('1.0.0');
    expect(status.state.lastRollback).toEqual({
      fromSlot: 'slotB',
      toSlot: 'slotA',
      failedVersion: '2.0.0',
      restoredVersion: '1.0.0'
    });

    const nextStage = machine.stage('3.0.0');
    expect(nextStage.state.stagedSlot).toBe('slotB');
    expect(nextStage.state.stagedVersion).toBe('3.0.0');
  });
});
