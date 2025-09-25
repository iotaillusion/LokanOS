const http = require('node:http');
const { createUpdaterApp } = require('./rest');
const { UpdaterStateMachine } = require('./state-machine');

const PORT = Number.parseInt(process.env.PORT || '4600', 10);
const HEALTH_FAIL_WINDOW = Number.parseInt(process.env.HEALTH_FAIL_WINDOW || '3', 10);

function startServer() {
  const machine = new UpdaterStateMachine({
    slotA: { version: process.env.SLOT_A_VERSION || '1.0.0' },
    slotB: { version: process.env.SLOT_B_VERSION || null },
    activeSlot: process.env.ACTIVE_SLOT || 'slotA',
    healthFailWindow: Number.isNaN(HEALTH_FAIL_WINDOW) ? 3 : HEALTH_FAIL_WINDOW
  });
  const { app } = createUpdaterApp({ machine });
  const server = http.createServer(app);
  server.listen(PORT, () => {
    // eslint-disable-next-line no-console
    console.log(`Updater service listening on port ${PORT}`);
  });
  return { server, machine };
}

if (require.main === module) {
  startServer();
}

module.exports = {
  startServer
};
