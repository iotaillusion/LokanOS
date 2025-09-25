const { startServer } = require('./rest');

const port = Number.parseInt(process.env.PORT || '', 10) || undefined;
const deviceRegistryUrl = process.env.DEVICE_REGISTRY_URL;

startServer({ port, deviceRegistryUrl });
