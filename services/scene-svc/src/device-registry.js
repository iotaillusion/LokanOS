const { SceneValidationError } = require('./errors');

const DEFAULT_DEVICE_REGISTRY_URL = 'http://localhost:4100';

function getFetchImplementation(providedFetch) {
  if (providedFetch) {
    return providedFetch;
  }
  if (typeof fetch === 'function') {
    return (...args) => fetch(...args);
  }
  return (...args) => import('node-fetch').then(({ default: fetchFn }) => fetchFn(...args));
}

class DeviceRegistryClient {
  constructor(baseUrl = DEFAULT_DEVICE_REGISTRY_URL, options = {}) {
    this.baseUrl = String(baseUrl || DEFAULT_DEVICE_REGISTRY_URL).replace(/\/$/, '');
    this.fetch = getFetchImplementation(options.fetchImpl);
  }

  async fetchDevice(deviceId) {
    const url = `${this.baseUrl}/devices/${encodeURIComponent(deviceId)}`;
    const response = await this.fetch(url, { method: 'GET' });
    if (response.status === 404) {
      return null;
    }
    if (!response.ok) {
      throw new SceneValidationError(`Failed to query device registry (status ${response.status})`);
    }
    return response.json();
  }

  async ensureDevicesExist(deviceIds = []) {
    const uniqueIds = Array.from(new Set(deviceIds.filter((id) => typeof id === 'string' && id.trim().length > 0)));
    if (uniqueIds.length === 0) {
      return;
    }

    const missing = [];
    for (const deviceId of uniqueIds) {
      // eslint-disable-next-line no-await-in-loop
      const device = await this.fetchDevice(deviceId);
      if (!device) {
        missing.push(deviceId);
      }
    }

    if (missing.length > 0) {
      const [firstMissing, ...rest] = missing;
      const suffix = rest.length > 0 ? ` (and ${rest.length} more)` : '';
      throw new SceneValidationError(`Device ${firstMissing}${suffix} not found in registry`);
    }
  }
}

module.exports = {
  DeviceRegistryClient,
  DEFAULT_DEVICE_REGISTRY_URL
};
