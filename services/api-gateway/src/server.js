const express = require('express');
const { registerRoutes } = require('./routes');

function createApp() {
  const app = express();
  app.use(express.json());

  app.get('/healthz', (req, res) => {
    res.json({ status: 'ok' });
  });

  registerRoutes(app);

  app.use((req, res) => {
    res.status(404).json({ error: 'Not Found' });
  });

  return app;
}

module.exports = {
  createApp
};
