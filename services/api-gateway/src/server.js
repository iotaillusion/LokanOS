const express = require('express');
const { registerRoutes } = require('./routes');
const { getConfig } = require('./config');

function createApp(options = {}) {
  const resolvedConfig = options.config || getConfig();
  const app = express();
  app.use(express.json());

  app.get('/healthz', (req, res) => {
    res.json({ status: 'ok' });
  });

  registerRoutes(app, {
    deviceRegistryUrl: resolvedConfig.deviceRegistryUrl
  });

  app.use((req, res) => {
    res.status(404).json({ error: 'Not Found' });
  });

  return app;
}

module.exports = {
  createApp
};
