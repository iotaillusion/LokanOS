const express = require('express');
const { SceneStore } = require('./scenes');
const { DeviceRegistryClient, DEFAULT_DEVICE_REGISTRY_URL } = require('./device-registry');
const { SceneValidationError, SceneNotFoundError } = require('./errors');

const DEFAULT_PORT = 4300;

function getFetchImplementation(fetchImpl) {
  if (fetchImpl) {
    return fetchImpl;
  }
  if (typeof fetch === 'function') {
    return (...args) => fetch(...args);
  }
  return (...args) => import('node-fetch').then(({ default: fetchFn }) => fetchFn(...args));
}

function createSceneApp(options = {}) {
  const app = express();
  app.use(express.json());

  const fetchImpl = getFetchImplementation(options.fetchImpl);
  const deviceRegistryUrl = options.deviceRegistryUrl || DEFAULT_DEVICE_REGISTRY_URL;
  const store = options.store || new SceneStore({
    deviceClient: new DeviceRegistryClient(deviceRegistryUrl, { fetchImpl })
  });

  app.get('/healthz', (req, res) => {
    res.json({ status: 'ok' });
  });

  app.get('/scenes', (req, res) => {
    res.json({ scenes: store.listScenes() });
  });

  app.post('/scenes', async (req, res) => {
    try {
      const scene = await store.createScene(req.body || {});
      res.status(201).json(scene);
    } catch (error) {
      if (error instanceof SceneValidationError) {
        res.status(400).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Failed to create scene' });
      }
    }
  });

  app.get('/scenes/:id', (req, res) => {
    try {
      const scene = store.getScene(req.params.id);
      if (!scene) {
        res.status(404).json({ error: 'Scene not found' });
        return;
      }
      res.json(scene);
    } catch (error) {
      if (error instanceof SceneValidationError) {
        res.status(400).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Failed to fetch scene' });
      }
    }
  });

  app.put('/scenes/:id', async (req, res) => {
    try {
      const scene = await store.updateScene(req.params.id, req.body || {});
      res.json(scene);
    } catch (error) {
      if (error instanceof SceneNotFoundError) {
        res.status(404).json({ error: error.message });
      } else if (error instanceof SceneValidationError) {
        res.status(400).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Failed to update scene' });
      }
    }
  });

  app.delete('/scenes/:id', (req, res) => {
    try {
      const deleted = store.deleteScene(req.params.id);
      if (!deleted) {
        res.status(404).json({ error: 'Scene not found' });
        return;
      }
      res.status(204).send();
    } catch (error) {
      if (error instanceof SceneValidationError) {
        res.status(400).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Failed to delete scene' });
      }
    }
  });

  app.post('/scenes/:id/apply', async (req, res) => {
    try {
      const plan = await store.applyScene(req.params.id);
      res.status(202).json(plan);
    } catch (error) {
      if (error instanceof SceneNotFoundError) {
        res.status(404).json({ error: error.message });
      } else if (error instanceof SceneValidationError) {
        res.status(400).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Failed to apply scene' });
      }
    }
  });

  app.use((req, res) => {
    res.status(404).json({ error: 'Not Found' });
  });

  return { app, store };
}

function startServer(options = {}) {
  const port = options.port || DEFAULT_PORT;
  const { app } = createSceneApp(options);
  const server = app.listen(port, () => {
    // eslint-disable-next-line no-console
    console.log(`Scene service listening on port ${port}`);
  });
  return server;
}

module.exports = {
  createSceneApp,
  startServer,
  DEFAULT_PORT
};
