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

describe('api-gateway server', () => {
  let registryServer;
  let app;
  let devices;

  beforeAll((done) => {
    const { app: registryApp, devices: registryDevices } = createDeviceRegistryStub();
    devices = registryDevices;
    registryServer = http.createServer(registryApp);
    registryServer.listen(0, () => {
      const { port } = registryServer.address();
      const deviceRegistryUrl = `http://127.0.0.1:${port}`;
      app = createApp({ config: { port: 0, tlsDisable: true, deviceRegistryUrl } });
      done();
    });
  });

  afterAll((done) => {
    registryServer.close(done);
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
});
