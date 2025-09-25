const DEFAULT_PORT = 8080;

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

function getConfig(env = process.env) {
  return {
    port: parsePort(env.PORT),
    tlsDisable: parseTlsDisable(env.TLS_DISABLE)
  };
}

module.exports = {
  DEFAULT_PORT,
  getConfig
};
