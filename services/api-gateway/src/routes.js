const DEFAULT_OPTIONS = {
  deviceRegistryUrl: 'http://localhost:4100',
  sceneServiceUrl: 'http://localhost:4300',
  ruleEngineUrl: 'http://localhost:4400',
  presenceServiceUrl: 'http://localhost:4500',
  updaterServiceUrl: 'http://localhost:4600'
};

const fetchImpl = typeof fetch === 'function'
  ? (...args) => fetch(...args)
  : (...args) => import('node-fetch').then(({ default: fetchFn }) => fetchFn(...args));

async function proxyRequest(path, options) {
  const response = await fetchImpl(path, options);
  const contentType = response.headers.get('content-type') || '';
  const body = contentType.includes('application/json') ? await response.json() : await response.text();
  if (!response.ok) {
    const error = new Error(`Upstream service responded with ${response.status}`);
    error.status = response.status;
    error.body = body;
    throw error;
  }
  return { status: response.status, body };
}

function registerRoutes(app, options = {}) {
  const mergedOptions = { ...DEFAULT_OPTIONS, ...options };
  const deviceBaseUrl = mergedOptions.deviceRegistryUrl.replace(/\/$/, '');
  const sceneBaseUrl = mergedOptions.sceneServiceUrl.replace(/\/$/, '');
  const ruleEngineBaseUrl = mergedOptions.ruleEngineUrl.replace(/\/$/, '');
  const presenceBaseUrl = mergedOptions.presenceServiceUrl.replace(/\/$/, '');
  const updaterBaseUrl = mergedOptions.updaterServiceUrl.replace(/\/$/, '');

  app.get('/v1/topology', async (req, res) => {
    try {
      const { body: data } = await proxyRequest(`${deviceBaseUrl}/devices`, { method: 'GET' });
      const devices = data.devices || [];
      const roomsMap = new Map();
      devices.forEach((device) => {
        const roomId = device.roomId || device.room_id;
        if (!roomId) {
          return;
        }
        if (!roomsMap.has(roomId)) {
          roomsMap.set(roomId, { id: roomId, deviceIds: [] });
        }
        roomsMap.get(roomId).deviceIds.push(device.id);
      });

      res.json({
        rooms: Array.from(roomsMap.values()),
        devices,
        scenes: []
      });
    } catch (error) {
      res.status(502).json({ error: 'Failed to load topology from device registry' });
    }
  });

  app.get('/v1/devices', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${deviceBaseUrl}/devices`, { method: 'GET' });
      res.json(body);
    } catch (error) {
      res.status(error.status || 502).json({ error: 'Failed to fetch devices' });
    }
  });

  app.get('/v1/devices/:id', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${deviceBaseUrl}/devices/${req.params.id}`, { method: 'GET' });
      res.json(body);
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Device not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to fetch device' });
      }
    }
  });

  app.post('/v1/devices', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${deviceBaseUrl}/devices`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.status(201).json(body);
    } catch (error) {
      res.status(error.status || 502).json({ error: 'Failed to create device', details: error.body?.error });
    }
  });

  app.put('/v1/devices/:id', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${deviceBaseUrl}/devices/${req.params.id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.json(body);
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Device not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to update device', details: error.body?.error });
      }
    }
  });

  app.delete('/v1/devices/:id', async (req, res) => {
    try {
      await proxyRequest(`${deviceBaseUrl}/devices/${req.params.id}`, { method: 'DELETE' });
      res.status(204).send();
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Device not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to delete device' });
      }
    }
  });

  app.post('/v1/devices/:id/state', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${deviceBaseUrl}/devices/${req.params.id}/state`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.json(body);
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Device not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to update device state', details: error.body?.error });
      }
    }
  });

  app.post('/v1/devices/:id/commands', (req, res) => {
    const { id } = req.params;
    res.status(202).json({
      commandId: `cmd-${id}`,
      status: 'queued'
    });
  });

  app.get('/v1/scenes', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${sceneBaseUrl}/scenes`, { method: 'GET' });
      res.json(body);
    } catch (error) {
      res.status(error.status || 502).json({ error: 'Failed to fetch scenes' });
    }
  });

  app.get('/v1/scenes/:id', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${sceneBaseUrl}/scenes/${req.params.id}`, { method: 'GET' });
      res.json(body);
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Scene not found' });
      } else if (error.status === 400) {
        res.status(400).json({ error: 'Invalid scene identifier' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to fetch scene' });
      }
    }
  });

  app.post('/v1/scenes', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${sceneBaseUrl}/scenes`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.status(201).json(body);
    } catch (error) {
      res.status(error.status || 502).json({ error: 'Failed to create scene', details: error.body?.error });
    }
  });

  app.put('/v1/scenes/:id', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${sceneBaseUrl}/scenes/${req.params.id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.json(body);
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Scene not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to update scene', details: error.body?.error });
      }
    }
  });

  app.delete('/v1/scenes/:id', async (req, res) => {
    try {
      await proxyRequest(`${sceneBaseUrl}/scenes/${req.params.id}`, { method: 'DELETE' });
      res.status(204).send();
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Scene not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to delete scene' });
      }
    }
  });

  app.post('/v1/scenes/:id/apply', async (req, res) => {
    try {
      const { status, body } = await proxyRequest(`${sceneBaseUrl}/scenes/${req.params.id}/apply`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.status(status).json(body);
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Scene not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to apply scene', details: error.body?.error });
      }
    }
  });

  app.post('/v1/rules:test', async (req, res) => {
    try {
      const { status, body } = await proxyRequest(`${ruleEngineBaseUrl}/v1/rules:test`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.status(status).json(body);
    } catch (error) {
      if (error.status === 400) {
        res.status(400).json({ error: 'Invalid rule test request', details: error.body?.details });
      } else {
        res
          .status(error.status || 502)
          .json({ error: 'Failed to execute rule test', details: error.body?.error });
      }
    }
  });

  app.get('/v1/presence', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${presenceBaseUrl}/presence`, { method: 'GET' });
      res.json(body);
    } catch (error) {
      res.status(error.status || 502).json({ error: 'Failed to fetch presence data' });
    }
  });

  app.get('/v1/presence/:userId', async (req, res) => {
    try {
      const { body } = await proxyRequest(`${presenceBaseUrl}/presence/${req.params.userId}`, { method: 'GET' });
      res.json(body);
    } catch (error) {
      if (error.status === 404) {
        res.status(404).json({ error: 'Presence not found' });
      } else {
        res.status(error.status || 502).json({ error: 'Failed to fetch presence record' });
      }
    }
  });

  app.post('/v1/updates/check', async (req, res) => {
    try {
      const { status, body } = await proxyRequest(`${updaterBaseUrl}/updates/check`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.status(status).json(body);
    } catch (error) {
      res.status(error.status || 502).json({ error: 'Failed to check for updates', details: error.body?.error });
    }
  });

  app.post('/v1/updates/apply', async (req, res) => {
    try {
      const { status, body } = await proxyRequest(`${updaterBaseUrl}/updates/apply`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req.body || {})
      });
      res.status(status).json(body);
    } catch (error) {
      res.status(error.status || 502).json({ error: 'Failed to apply update', details: error.body?.error });
    }
  });
}

module.exports = {
  registerRoutes
};
