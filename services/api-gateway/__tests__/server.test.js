const express = require('express');
const http = require('node:http');
const request = require('supertest');
const { createApp } = require('../src/server');

function createDeviceRegistryStub() {
  const app = express();
  app.use(express.json());

  const devices = new Map([
    [
      'device-thermostat-1',
      {
        id: 'device-thermostat-1',
        roomId: 'room-1',
        capabilities: ['thermostat'],
        state: { temperature: 72 }
      }
    ]
  ]);

  app.get('/devices', (req, res) => {
    res.json({ devices: Array.from(devices.values()) });
  });

  app.get('/devices/:id', (req, res) => {
    const device = devices.get(req.params.id);
    if (!device) {
      res.status(404).json({ error: 'Device not found' });
      return;
    }
    res.json(device);
  });

  app.post('/devices', (req, res) => {
    const payload = req.body || {};
    if (!payload.id) {
      res.status(400).json({ error: 'Missing id' });
      return;
    }
    devices.set(payload.id, {
      id: payload.id,
      roomId: payload.roomId,
      capabilities: payload.capabilities || [],
      state: payload.state || {}
    });
    res.status(201).json(devices.get(payload.id));
  });

  app.put('/devices/:id', (req, res) => {
    const existing = devices.get(req.params.id);
    if (!existing) {
      res.status(404).json({ error: 'Device not found' });
      return;
    }
    const updates = req.body || {};
    const updated = {
      ...existing,
      ...updates,
      state: { ...existing.state, ...(updates.state || {}) }
    };
    devices.set(req.params.id, updated);
    res.json(updated);
  });

  app.delete('/devices/:id', (req, res) => {
    if (!devices.has(req.params.id)) {
      res.status(404).json({ error: 'Device not found' });
      return;
    }
    devices.delete(req.params.id);
    res.status(204).send();
  });

  app.post('/devices/:id/state', (req, res) => {
    const existing = devices.get(req.params.id);
    if (!existing) {
      res.status(404).json({ error: 'Device not found' });
      return;
    }
    existing.state = { ...existing.state, ...(req.body || {}) };
    devices.set(req.params.id, existing);
    res.json({ deviceId: existing.id, state: existing.state });
  });

  return { app, devices };
}

function createSceneServiceStub(devices) {
  const app = express();
  app.use(express.json());

  const scenes = new Map();

  function validateItems(items) {
    if (!Array.isArray(items)) {
      return 'items must be an array';
    }
    for (let index = 0; index < items.length; index += 1) {
      const item = items[index];
      if (!item || typeof item !== 'object') {
        return `items[${index}] must be an object`;
      }
      if (typeof item.deviceId !== 'string') {
        return `items[${index}].deviceId must be a string`;
      }
      if (!devices.has(item.deviceId)) {
        return `Device ${item.deviceId} not found`;
      }
      if (!item.desiredState || typeof item.desiredState !== 'object') {
        return `items[${index}].desiredState must be an object`;
      }
    }
    return null;
  }

  app.get('/scenes', (req, res) => {
    res.json({ scenes: Array.from(scenes.values()) });
  });

  app.get('/scenes/:id', (req, res) => {
    const scene = scenes.get(req.params.id);
    if (!scene) {
      res.status(404).json({ error: 'Scene not found' });
      return;
    }
    res.json(scene);
  });

  app.post('/scenes', (req, res) => {
    const payload = req.body || {};
    if (!payload.id || !payload.name) {
      res.status(400).json({ error: 'Invalid scene payload' });
      return;
    }
    const validationError = validateItems(payload.items || []);
    if (validationError) {
      res.status(400).json({ error: validationError });
      return;
    }
    const scene = {
      id: payload.id,
      name: payload.name,
      items: (payload.items || []).map((item) => ({
        deviceId: item.deviceId,
        desiredState: { ...item.desiredState }
      }))
    };
    scenes.set(scene.id, scene);
    res.status(201).json(scene);
  });

  app.put('/scenes/:id', (req, res) => {
    const existing = scenes.get(req.params.id);
    if (!existing) {
      res.status(404).json({ error: 'Scene not found' });
      return;
    }
    const updates = req.body || {};
    if (updates.items) {
      const validationError = validateItems(updates.items);
      if (validationError) {
        res.status(400).json({ error: validationError });
        return;
      }
      existing.items = updates.items.map((item) => ({
        deviceId: item.deviceId,
        desiredState: { ...item.desiredState }
      }));
    }
    if (updates.name) {
      existing.name = updates.name;
    }
    scenes.set(existing.id, existing);
    res.json(existing);
  });

  app.delete('/scenes/:id', (req, res) => {
    if (!scenes.has(req.params.id)) {
      res.status(404).json({ error: 'Scene not found' });
      return;
    }
    scenes.delete(req.params.id);
    res.status(204).send();
  });

  app.post('/scenes/:id/apply', (req, res) => {
    const scene = scenes.get(req.params.id);
    if (!scene) {
      res.status(404).json({ error: 'Scene not found' });
      return;
    }
    res.status(202).json({
      sceneId: scene.id,
      status: 'planned',
      steps: scene.items.map((item, index) => ({
        order: index + 1,
        deviceId: item.deviceId,
        desiredState: { ...item.desiredState }
      }))
    });
  });

  return { app, scenes };
}

function createRuleEngineStub() {
  const app = express();
  app.use(express.json());

  const requests = [];

  app.post('/v1/rules:test', (req, res) => {
    const payload = req.body || {};
    requests.push(payload);
    if (!payload.ruleId || !payload.rule) {
      res.status(400).json({ error: 'invalid payload' });
      return;
    }
    const actions = Array.isArray(payload.rule.actions) ? payload.rule.actions : [];
    res.json({
      ruleId: payload.ruleId,
      status: 'passed',
      logs: ['stubbed simulation'],
      actions,
      errors: []
    });
  });

  return { app, requests };
}

describe('api-gateway server', () => {
  let registryServer;
  let sceneServer;
  let ruleEngineServer;
  let app;
  let devices;
  let scenes;
  let ruleRequests;

  beforeAll((done) => {
    const { app: registryApp, devices: registryDevices } = createDeviceRegistryStub();
    devices = registryDevices;
    const { app: sceneApp, scenes: sceneStore } = createSceneServiceStub(devices);
    scenes = sceneStore;
    const { app: ruleApp, requests } = createRuleEngineStub();
    ruleRequests = requests;

    registryServer = http.createServer(registryApp);
    sceneServer = http.createServer(sceneApp);
    ruleEngineServer = http.createServer(ruleApp);

    let pending = 3;
    let registryPort;
    let scenePort;
    let ruleEnginePort;
    function handleReady() {
      pending -= 1;
      if (pending === 0) {
        const deviceRegistryUrl = `http://127.0.0.1:${registryPort}`;
        const sceneServiceUrl = `http://127.0.0.1:${scenePort}`;
        const ruleEngineUrl = `http://127.0.0.1:${ruleEnginePort}`;
        app = createApp({
          config: { port: 0, tlsDisable: true, deviceRegistryUrl, sceneServiceUrl, ruleEngineUrl }
        });
        done();
      }
    }

    registryServer.listen(0, () => {
      registryPort = registryServer.address().port;
      handleReady();
    });
    sceneServer.listen(0, () => {
      scenePort = sceneServer.address().port;
      handleReady();
    });
    ruleEngineServer.listen(0, () => {
      ruleEnginePort = ruleEngineServer.address().port;
      handleReady();
    });
  });

  afterAll((done) => {
    const servers = [registryServer, sceneServer, ruleEngineServer].filter(Boolean);
    let pending = servers.length;
    if (pending === 0) {
      done();
      return;
    }
    servers.forEach((server) => {
      server.close(() => {
        pending -= 1;
        if (pending === 0) {
          done();
        }
      });
    });
  });

  describe('GET /healthz', () => {
    it('returns a healthy status payload', async () => {
      const response = await request(app).get('/healthz');
      expect(response.status).toBe(200);
      expect(response.body).toEqual({ status: 'ok' });
    });
  });

  describe('device proxy routes', () => {
    it('proxies CRUD operations to the device registry service', async () => {
      const client = request(app);

      const listResponse = await client.get('/v1/devices');
      expect(listResponse.status).toBe(200);
      expect(listResponse.body.devices).toHaveLength(1);

      const createResponse = await client
        .post('/v1/devices')
        .send({ id: 'device-light-1', roomId: 'room-1', capabilities: ['light'], state: { power: 'off' } });
      expect(createResponse.status).toBe(201);
      expect(devices.has('device-light-1')).toBe(true);

      const updateResponse = await client
        .put('/v1/devices/device-light-1')
        .send({ roomId: 'room-2', state: { power: 'on' } });
      expect(updateResponse.status).toBe(200);
      expect(updateResponse.body).toMatchObject({ roomId: 'room-2', state: { power: 'on' } });

      const stateResponse = await client
        .post('/v1/devices/device-light-1/state')
        .send({ brightness: 80 });
      expect(stateResponse.status).toBe(200);
      expect(stateResponse.body).toMatchObject({ deviceId: 'device-light-1', state: { power: 'on', brightness: 80 } });

      const deleteResponse = await client.delete('/v1/devices/device-light-1');
      expect(deleteResponse.status).toBe(204);
      expect(devices.has('device-light-1')).toBe(false);
    });

    it('aggregates topology data from the device registry', async () => {
      const response = await request(app).get('/v1/topology');
      expect(response.status).toBe(200);
      expect(response.body.devices).toHaveLength(devices.size);
      expect(response.body.rooms).toEqual([
        {
          id: 'room-1',
          deviceIds: Array.from(devices.values()).filter((device) => device.roomId === 'room-1').map((device) => device.id)
        }
      ]);
    });
  });

  describe('scene proxy routes', () => {
    it('proxies CRUD operations to the scene service', async () => {
      const client = request(app);

      const createResponse = await client.post('/v1/scenes').send({
        id: 'scene-movie-night',
        name: 'Movie Night',
        items: [{ deviceId: 'device-thermostat-1', desiredState: { temperature: 68 } }]
      });
      expect(createResponse.status).toBe(201);
      expect(scenes.has('scene-movie-night')).toBe(true);

      const listResponse = await client.get('/v1/scenes');
      expect(listResponse.status).toBe(200);
      expect(listResponse.body.scenes).toHaveLength(1);

      const getResponse = await client.get('/v1/scenes/scene-movie-night');
      expect(getResponse.status).toBe(200);
      expect(getResponse.body).toMatchObject({ id: 'scene-movie-night', name: 'Movie Night' });

      const updateResponse = await client
        .put('/v1/scenes/scene-movie-night')
        .send({ name: 'Movie Time' });
      expect(updateResponse.status).toBe(200);
      expect(updateResponse.body).toMatchObject({ name: 'Movie Time' });

      const applyResponse = await client.post('/v1/scenes/scene-movie-night/apply');
      expect(applyResponse.status).toBe(202);
      expect(applyResponse.body).toMatchObject({ sceneId: 'scene-movie-night', status: 'planned' });

      const deleteResponse = await client.delete('/v1/scenes/scene-movie-night');
      expect(deleteResponse.status).toBe(204);
      expect(scenes.has('scene-movie-night')).toBe(false);
    });
  });

  describe('rule engine routes', () => {
    it('proxies rule simulation requests to the rule engine service', async () => {
      const payload = {
        ruleId: 'rule-123',
        rule: {
          triggers: [{ type: 'event', event: 'motion.detected' }],
          conditions: [{ type: 'comparison', operator: 'eq', path: 'sensors.motion', value: true }],
          actions: [{ type: 'notify', payload: { message: 'Motion detected' } }]
        },
        inputs: { trigger: { type: 'event', event: 'motion.detected' } }
      };

      const response = await request(app).post('/v1/rules:test').send(payload);

      expect(response.status).toBe(200);
      expect(response.body).toMatchObject({ ruleId: 'rule-123', status: 'passed' });
      expect(response.body.actions).toEqual(payload.rule.actions);
      expect(ruleRequests).toHaveLength(1);
      expect(ruleRequests[0]).toMatchObject(payload);
    });
  });
});
