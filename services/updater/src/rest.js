const express = require('express');
const { UpdaterStateMachine, UpdaterError } = require('./state-machine');

function createUpdaterApp(options = {}) {
  const machine = options.machine instanceof UpdaterStateMachine ? options.machine : new UpdaterStateMachine(options.state || {});
  const app = express();
  app.use(express.json());

  app.post('/updates/check', (req, res) => {
    res.json(machine.check());
  });

  app.post('/updates/apply', (req, res) => {
    const payload = req.body || {};
    try {
      if (payload.finalize === true && !payload.version) {
        const result = machine.commit();
        res.json({ status: result.status, state: result.state });
        return;
      }

      if (!payload.version) {
        res.status(400).json({ error: 'version is required to apply an update' });
        return;
      }

      const stageResult = machine.stage(payload.version);
      const commitResult = machine.commit();
      const responseState = commitResult.status === 'activated' ? commitResult.state : stageResult.state;
      res.status(202).json({ status: 'staged', state: responseState });
    } catch (error) {
      if (error instanceof UpdaterError) {
        const status = error.code === 'INVALID_VERSION' ? 400 : 409;
        res.status(status).json({ error: error.message, code: error.code });
        return;
      }
      // eslint-disable-next-line no-console
      console.error('updater: failed to apply update', error);
      res.status(500).json({ error: 'failed to apply update' });
    }
  });

  app.post('/updates/commit', (req, res) => {
    const result = machine.commit();
    res.json({ status: result.status, state: result.state });
  });

  app.post('/updates/health/unhealthy', (req, res) => {
    const result = machine.markUnhealthy();
    res.json({ status: result.status, rolledBack: result.rolledBack, state: result.state });
  });

  return { app, machine };
}

module.exports = {
  createUpdaterApp
};
