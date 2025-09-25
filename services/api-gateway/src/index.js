const { createApp } = require('./server');
const { getConfig } = require('./config');

function start() {
  const config = getConfig();
  const app = createApp();

  const server = app.listen(config.port, () => {
    const tlsMessage = config.tlsDisable ? ' with TLS disabled (development only)' : '';
    console.log(`API gateway listening on port ${config.port}${tlsMessage}`);
  });

  return server;
}

if (require.main === module) {
  start();
}

module.exports = {
  start
};
