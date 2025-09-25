const { SceneValidationError, SceneNotFoundError } = require('./errors');

function cloneScene(scene) {
  return {
    id: scene.id,
    name: scene.name,
    items: scene.items.map((item) => ({
      deviceId: item.deviceId,
      desiredState: { ...item.desiredState }
    }))
  };
}

class SceneStore {
  constructor(options = {}) {
    this.deviceClient = options.deviceClient;
    this.scenes = new Map();
  }

  async ensureDevicesExist(deviceIds) {
    if (this.deviceClient && typeof this.deviceClient.ensureDevicesExist === 'function') {
      await this.deviceClient.ensureDevicesExist(deviceIds);
    }
  }

  validateString(value, fieldName) {
    if (typeof value !== 'string' || value.trim().length === 0) {
      throw new SceneValidationError(`${fieldName} must be a non-empty string`);
    }
    return value.trim();
  }

  normalizeItems(items) {
    if (!Array.isArray(items)) {
      throw new SceneValidationError('items must be an array');
    }
    return items.map((item, index) => {
      if (!item || typeof item !== 'object') {
        throw new SceneValidationError(`items[${index}] must be an object`);
      }
      const deviceId = this.validateString(item.deviceId, `items[${index}].deviceId`);
      const desiredState = item.desiredState;
      if (!desiredState || typeof desiredState !== 'object' || Array.isArray(desiredState)) {
        throw new SceneValidationError(`items[${index}].desiredState must be an object`);
      }
      return {
        deviceId,
        desiredState: { ...desiredState }
      };
    });
  }

  async createScene(payload) {
    if (!payload || typeof payload !== 'object') {
      throw new SceneValidationError('Scene payload must be an object');
    }
    const id = this.validateString(payload.id, 'id');
    if (this.scenes.has(id)) {
      throw new SceneValidationError(`Scene ${id} already exists`);
    }
    const name = this.validateString(payload.name, 'name');
    const items = this.normalizeItems(payload.items ?? []);
    await this.ensureDevicesExist(items.map((item) => item.deviceId));
    const scene = { id, name, items };
    this.scenes.set(id, scene);
    return cloneScene(scene);
  }

  getScene(sceneId) {
    const id = this.validateString(sceneId, 'sceneId');
    const scene = this.scenes.get(id);
    if (!scene) {
      return null;
    }
    return cloneScene(scene);
  }

  listScenes() {
    return Array.from(this.scenes.values()).map((scene) => cloneScene(scene));
  }

  async updateScene(sceneId, updates) {
    const id = this.validateString(sceneId, 'sceneId');
    const existing = this.scenes.get(id);
    if (!existing) {
      throw new SceneNotFoundError(`Scene ${sceneId} not found`);
    }
    if (!updates || typeof updates !== 'object') {
      throw new SceneValidationError('Scene updates must be an object');
    }
    let name = existing.name;
    if (Object.prototype.hasOwnProperty.call(updates, 'name')) {
      name = this.validateString(updates.name, 'name');
    }
    let items = existing.items;
    if (Object.prototype.hasOwnProperty.call(updates, 'items')) {
      items = this.normalizeItems(updates.items);
      await this.ensureDevicesExist(items.map((item) => item.deviceId));
    }
    const updated = { id: existing.id, name, items };
    this.scenes.set(id, updated);
    return cloneScene(updated);
  }

  deleteScene(sceneId) {
    const id = this.validateString(sceneId, 'sceneId');
    return this.scenes.delete(id);
  }

  async applyScene(sceneId) {
    const id = this.validateString(sceneId, 'sceneId');
    const existing = this.scenes.get(id);
    if (!existing) {
      throw new SceneNotFoundError(`Scene ${sceneId} not found`);
    }
    await this.ensureDevicesExist(existing.items.map((item) => item.deviceId));
    return {
      sceneId: existing.id,
      status: 'planned',
      steps: existing.items.map((item, index) => ({
        order: index + 1,
        deviceId: item.deviceId,
        desiredState: { ...item.desiredState }
      }))
    };
  }
}

module.exports = {
  SceneStore
};
