const DEFAULT_PORT = 8080;
const DEFAULT_DEVICE_REGISTRY_URL = 'http://localhost:4100';

function parsePort(rawPort) {
  const port = Number.parseInt(rawPort, 10);
  if (Number.isNaN(port) || port <= 0) {
    return DEFAULT_PORT;
  }
  return port;
}

function parseTlsDisable(rawValue) {
  if (!rawValue) {
    return false;
  }
  const normalized = String(rawValue).trim().toLowerCase();
  return normalized === '1' || normalized === 'true' || normalized === 'yes';
}

function parseDeviceRegistryUrl(rawUrl) {
  if (!rawUrl) {
    return DEFAULT_DEVICE_REGISTRY_URL;
  }
  try {
    const parsed = new URL(rawUrl);
    return parsed.toString().replace(/\/$/, '');
  } catch (error) {
    return DEFAULT_DEVICE_REGISTRY_URL;
  }
}

function getConfig(env = process.env) {
  return {
    port: parsePort(env.PORT),
    tlsDisable: parseTlsDisable(env.TLS_DISABLE),
    deviceRegistryUrl: parseDeviceRegistryUrl(env.DEVICE_REGISTRY_URL)
  };
}

module.exports = {
  DEFAULT_PORT,
  DEFAULT_DEVICE_REGISTRY_URL,
  getConfig
};
