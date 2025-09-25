const express = require('express');
const { PresenceStore, PresenceValidationError } = require('./store');

function createPresenceApp(options = {}) {
  const store = options.store instanceof PresenceStore ? options.store : options.store || new PresenceStore(options);
  const app = express();
  app.use(express.json());

  app.get('/presence', (req, res) => {
    res.json({ presences: store.listPresence() });
  });

  app.get('/presence/:userId', (req, res) => {
    const record = store.getPresence(req.params.userId);
    if (!record) {
      res.status(404).json({ error: `Presence for user ${req.params.userId} not found` });
      return;
    }
    res.json(record);
  });

  function handleSetPresence(req, res) {
    try {
      const record = store.setPresence(req.params.userId, req.body || {});
      res.json(record);
    } catch (error) {
      if (error instanceof PresenceValidationError) {
        res.status(400).json({ error: error.message });
        return;
      }
      // eslint-disable-next-line no-console
      console.error('Failed to update presence', error);
      res.status(500).json({ error: 'Failed to update presence' });
    }
  }

  app.post('/presence/:userId', handleSetPresence);
  app.put('/presence/:userId', handleSetPresence);

  return { app, store };
}

module.exports = {
  createPresenceApp
};
