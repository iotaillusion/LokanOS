const { EventEmitter } = require('events');
const { DeviceId, RoomId, Capability, DeviceState } = require('./types');

class DeviceRegistry {
  constructor(initialDevices = []) {
    this.devices = new Map();
    this.emitter = new EventEmitter();
    this.emitter.setMaxListeners(0);

    initialDevices.forEach((device) => {
      this.createDevice(device);
    });
  }

  normalizeDevicePayload(payload) {
    if (!payload || typeof payload !== 'object') {
      throw new TypeError('Device payload must be an object');
    }

    const id = payload.id instanceof DeviceId ? payload.id.toString() : new DeviceId(payload.id).toString();
    const roomId = payload.roomId instanceof RoomId ? payload.roomId.toString() : new RoomId(payload.roomId).toString();
    const capabilities = Array.isArray(payload.capabilities)
      ? payload.capabilities.map((cap) => (cap instanceof Capability ? cap.toString() : new Capability(cap).toString()))
      : [];
    const state = payload.state instanceof DeviceState ? payload.state.toJSON() : new DeviceState(payload.state || {}).toJSON();

    return {
      id,
      roomId,
      capabilities,
      state
    };
  }

  listDevices() {
    return Array.from(this.devices.values()).map((device) => ({ ...device, state: { ...device.state } }));
  }

  getDevice(deviceId) {
    const id = deviceId instanceof DeviceId ? deviceId.toString() : new DeviceId(deviceId).toString();
    const device = this.devices.get(id);
    if (!device) {
      return null;
    }
    return { ...device, state: { ...device.state } };
  }

  createDevice(payload) {
    const normalized = this.normalizeDevicePayload(payload);
    if (this.devices.has(normalized.id)) {
      throw new Error(`Device ${normalized.id} already exists`);
    }
    this.devices.set(normalized.id, normalized);
    return { ...normalized, state: { ...normalized.state } };
  }

  updateDevice(deviceId, updates) {
    const existing = this.getDevice(deviceId);
    if (!existing) {
      throw new Error(`Device ${deviceId} not found`);
    }
    const merged = this.normalizeDevicePayload({
      id: existing.id,
      roomId: updates.roomId ?? existing.roomId,
      capabilities: updates.capabilities ?? existing.capabilities,
      state: updates.state ? { ...existing.state, ...updates.state } : existing.state
    });
    this.devices.set(merged.id, merged);
    return { ...merged, state: { ...merged.state } };
  }

  deleteDevice(deviceId) {
    const id = deviceId instanceof DeviceId ? deviceId.toString() : new DeviceId(deviceId).toString();
    const existed = this.devices.delete(id);
    return existed;
  }

  updateDeviceState(deviceId, stateUpdate) {
    const existing = this.getDevice(deviceId);
    if (!existing) {
      throw new Error(`Device ${deviceId} not found`);
    }
    const currentState = new DeviceState(existing.state);
    const mergedState = currentState.merge(stateUpdate);
    const updated = { ...existing, state: mergedState };
    this.devices.set(existing.id, { ...existing, state: mergedState });

    const event = {
      deviceId: existing.id,
      state: { ...mergedState }
    };
    this.emitter.emit('state', event);
    return event;
  }

  subscribeToStateUpdates(listener) {
    this.emitter.on('state', listener);
    return () => this.emitter.off('state', listener);
  }
}

module.exports = {
  DeviceRegistry
};
