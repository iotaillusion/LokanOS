class DeviceId {
  constructor(value) {
    if (typeof value !== 'string' || value.trim().length === 0) {
      throw new TypeError('DeviceId must be a non-empty string');
    }
    this.value = value;
  }

  toString() {
    return this.value;
  }
}

class RoomId {
  constructor(value) {
    if (typeof value !== 'string' || value.trim().length === 0) {
      throw new TypeError('RoomId must be a non-empty string');
    }
    this.value = value;
  }

  toString() {
    return this.value;
  }
}

class Capability {
  constructor(value) {
    if (typeof value !== 'string' || value.trim().length === 0) {
      throw new TypeError('Capability must be a non-empty string');
    }
    this.value = value;
  }

  toString() {
    return this.value;
  }
}

class DeviceState {
  constructor(initialState = {}) {
    if (!DeviceState.isValid(initialState)) {
      throw new TypeError('DeviceState must be a plain object');
    }
    this.value = { ...initialState };
  }

  static isValid(state) {
    return state && typeof state === 'object' && !Array.isArray(state);
  }

  merge(update) {
    if (!DeviceState.isValid(update)) {
      throw new TypeError('DeviceState updates must be plain objects');
    }
    this.value = { ...this.value, ...update };
    return this.value;
  }

  toJSON() {
    return { ...this.value };
  }
}

module.exports = {
  DeviceId,
  RoomId,
  Capability,
  DeviceState
};
