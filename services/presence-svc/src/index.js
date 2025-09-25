const http = require('node:http');
const { createPresenceApp } = require('./rest');
const { PresenceStore } = require('./store');

const PORT = Number.parseInt(process.env.PORT || '4500', 10);

function startServer() {
  const store = new PresenceStore();
  const { app } = createPresenceApp({ store });
  const server = http.createServer(app);
  server.listen(PORT, () => {
    // eslint-disable-next-line no-console
    console.log(`Presence service listening on port ${PORT}`);
  });
  return { server, store };
}

if (require.main === module) {
  startServer();
}

module.exports = {
  startServer
};
