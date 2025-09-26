/**
 * Runtime client generated from the LokanOS OpenAPI document.
 * This module is intentionally dependency-free.
 */

/**
 * @typedef {Object} RequestOptions
 * @property {string} [baseUrl] Base URL for the LokanOS API.
 * @property {Record<string, string>} [headers] Additional headers to send.
 * @property {*} [body] Optional request body payload.
 * @property {typeof fetch} [fetch] Custom fetch implementation.
 */
function joinUrl(baseUrl, apiPath) {
  if (!baseUrl) {
    return apiPath;
  }
  if (/^https?:/i.test(apiPath)) {
    return apiPath;
  }
  const normalizedBase = baseUrl.replace(/[/]+$/, '');
  const normalizedPath = apiPath.startsWith('/') ? apiPath : `/${apiPath}`;
  return `${normalizedBase}${normalizedPath}`;
}

function hasHeader(headers, name) {
  const lower = name.toLowerCase();
  return Object.keys(headers).some((key) => key.toLowerCase() === lower);
}

function shouldSerializeJson(body) {
  if (body === null || body === undefined) {
    return false;
  }
  if (typeof body === 'string') {
    return false;
  }
  if (typeof ArrayBuffer !== 'undefined' && (body instanceof ArrayBuffer || ArrayBuffer.isView(body))) {
    return false;
  }
  if (typeof Blob !== 'undefined' && body instanceof Blob) {
    return false;
  }
  if (typeof FormData !== 'undefined' && body instanceof FormData) {
    return false;
  }
  return typeof body === 'object';
}

export async function request(path, method, options = {}) {
  const { baseUrl, headers = {}, body, fetch: customFetch } = options;
  const fetchImpl = customFetch || globalThis.fetch;
  if (!fetchImpl) {
    throw new Error('Fetch API is not available in this environment.');
  }
  const url = joinUrl(baseUrl, path);
  const finalHeaders = { ...headers };
  let finalBody;
  if (body !== undefined) {
    if (shouldSerializeJson(body)) {
      if (!hasHeader(finalHeaders, 'content-type')) {
        finalHeaders['Content-Type'] = 'application/json';
      }
      finalBody = JSON.stringify(body);
    } else {
      finalBody = body;
    }
  }
  return fetchImpl(url, { method, headers: finalHeaders, body: finalBody });
}

export async function apiGatewayDiagnostics(options = {}) {
  const response = await request('/api-gateway/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function apiGatewayHealth(options = {}) {
  const response = await request('/api-gateway/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function apiGatewayMetrics(options = {}) {
  const response = await request('/api-gateway/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function apiGatewayListRoutes(options = {}) {
  const response = await request('/api-gateway/routes', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function auditLogDiagnostics(options = {}) {
  const response = await request('/audit-log/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function auditLogListEntries(options = {}) {
  const response = await request('/audit-log/entries', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function auditLogHealth(options = {}) {
  const response = await request('/audit-log/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function auditLogMetrics(options = {}) {
  const response = await request('/audit-log/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function deviceRegistryListDevices(options = {}) {
  const response = await request('/device-registry/devices', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function deviceRegistryDiagnostics(options = {}) {
  const response = await request('/device-registry/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function deviceRegistryHealth(options = {}) {
  const response = await request('/device-registry/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function deviceRegistryMetrics(options = {}) {
  const response = await request('/device-registry/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function energyServiceDiagnostics(options = {}) {
  const response = await request('/energy-svc/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function energyServiceGetReport(options = {}) {
  const response = await request('/energy-svc/energy-report', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function energyServiceHealth(options = {}) {
  const response = await request('/energy-svc/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function energyServiceMetrics(options = {}) {
  const response = await request('/energy-svc/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function presenceServiceDiagnostics(options = {}) {
  const response = await request('/presence-svc/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function presenceServiceHealth(options = {}) {
  const response = await request('/presence-svc/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function presenceServiceMetrics(options = {}) {
  const response = await request('/presence-svc/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function presenceServiceGetPresence(options = {}) {
  const response = await request('/presence-svc/presence', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function radioCoordinatorListChannels(options = {}) {
  const response = await request('/radio-coord/channels', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function radioCoordinatorDiagnostics(options = {}) {
  const response = await request('/radio-coord/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function radioCoordinatorHealth(options = {}) {
  const response = await request('/radio-coord/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function radioCoordinatorMetrics(options = {}) {
  const response = await request('/radio-coord/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function ruleEngineDiagnostics(options = {}) {
  const response = await request('/rule-engine/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function ruleEngineHealth(options = {}) {
  const response = await request('/rule-engine/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function ruleEngineMetrics(options = {}) {
  const response = await request('/rule-engine/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function ruleEngineListRules(options = {}) {
  const response = await request('/rule-engine/rules', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function sceneServiceDiagnostics(options = {}) {
  const response = await request('/scene-svc/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function sceneServiceHealth(options = {}) {
  const response = await request('/scene-svc/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function sceneServiceMetrics(options = {}) {
  const response = await request('/scene-svc/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function sceneServiceListScenes(options = {}) {
  const response = await request('/scene-svc/scenes', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function telemetryPipeDiagnostics(options = {}) {
  const response = await request('/telemetry-pipe/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function telemetryPipeHealth(options = {}) {
  const response = await request('/telemetry-pipe/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function telemetryPipeIngest(body, options = {}) {
  const response = await request('/telemetry-pipe/ingest', 'POST', { ...options, body });
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return;
}

export async function telemetryPipeMetrics(options = {}) {
  const response = await request('/telemetry-pipe/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}

export async function updaterServiceAvailable(options = {}) {
  const response = await request('/updater/available', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function updaterServiceDiagnostics(options = {}) {
  const response = await request('/updater/diag', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function updaterServiceHealth(options = {}) {
  const response = await request('/updater/health', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.json();
}

export async function updaterServiceMetrics(options = {}) {
  const response = await request('/updater/metrics', 'GET', options);
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return await response.text();
}
