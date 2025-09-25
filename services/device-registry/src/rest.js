const express = require('express');
const { DeviceRegistry } = require('./registry');

function createRestApp(registry = new DeviceRegistry()) {
  const app = express();
  app.use(express.json());

  app.get('/healthz', (req, res) => {
    res.json({ status: 'ok' });
  });

  app.get('/devices', (req, res) => {
    res.json({ devices: registry.listDevices() });
  });

  app.get('/devices/:id', (req, res) => {
    const device = registry.getDevice(req.params.id);
    if (!device) {
      res.status(404).json({ error: 'Device not found' });
      return;
    }
    res.json(device);
  });

  app.post('/devices', (req, res) => {
    try {
      const device = registry.createDevice(req.body);
      res.status(201).json(device);
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  app.put('/devices/:id', (req, res) => {
    try {
      const device = registry.updateDevice(req.params.id, req.body || {});
      res.json(device);
    } catch (error) {
      if (error.message.includes('not found')) {
        res.status(404).json({ error: error.message });
      } else {
        res.status(400).json({ error: error.message });
      }
    }
  });

  app.delete('/devices/:id', (req, res) => {
    const deleted = registry.deleteDevice(req.params.id);
    if (!deleted) {
      res.status(404).json({ error: 'Device not found' });
      return;
    }
    res.status(204).send();
  });

  app.post('/devices/:id/state', (req, res) => {
    try {
      const event = registry.updateDeviceState(req.params.id, req.body || {});
      res.json(event);
    } catch (error) {
      if (error.message.includes('not found')) {
        res.status(404).json({ error: error.message });
      } else {
        res.status(400).json({ error: error.message });
      }
    }
  });

  app.get('/devices/state/stream', (req, res) => {
    res.setHeader('Content-Type', 'text/event-stream');
    res.setHeader('Cache-Control', 'no-cache');
    res.setHeader('Connection', 'keep-alive');

    const sendEvent = (event) => {
      res.write(`event: device-state\n`);
      res.write(`data: ${JSON.stringify(event)}\n\n`);
    };

    const unsubscribe = registry.subscribeToStateUpdates(sendEvent);
    req.on('close', () => {
      unsubscribe();
    });
  });

  app.use((req, res) => {
    res.status(404).json({ error: 'Not Found' });
  });

  return { app, registry };
}

module.exports = {
  createRestApp
};
