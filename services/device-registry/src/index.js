const http = require('node:http');
const grpc = require('@grpc/grpc-js');
const { createRestApp } = require('./rest');
const { createGrpcServer } = require('./grpc');
const { DeviceRegistry } = require('./registry');

const REST_PORT = Number.parseInt(process.env.REST_PORT || '4100', 10);
const GRPC_PORT = Number.parseInt(process.env.GRPC_PORT || '50051', 10);

function startServers() {
  const registry = new DeviceRegistry([
    {
      id: 'device-thermostat-1',
      roomId: 'room-1',
      capabilities: ['thermostat', 'temperature'],
      state: { temperature: 72 }
    },
    {
      id: 'device-light-1',
      roomId: 'room-1',
      capabilities: ['light', 'dimming'],
      state: { power: 'on', brightness: 80 }
    },
    {
      id: 'device-light-2',
      roomId: 'room-2',
      capabilities: ['light'],
      state: { power: 'off' }
    }
  ]);

  const { app } = createRestApp(registry);
  const restServer = http.createServer(app);
  restServer.listen(REST_PORT, () => {
    console.log(`Device registry REST API listening on port ${REST_PORT}`);
  });

  const { server: grpcServer } = createGrpcServer(registry);
  grpcServer.bindAsync(`0.0.0.0:${GRPC_PORT}`, grpc.ServerCredentials.createInsecure(), (err) => {
    if (err) {
      console.error('Failed to start gRPC server:', err);
      process.exit(1);
    }
    grpcServer.start();
    console.log(`Device registry gRPC API listening on port ${GRPC_PORT}`);
  });

  return { restServer, grpcServer, registry };
}

if (require.main === module) {
  startServers();
}

module.exports = {
  startServers
};
