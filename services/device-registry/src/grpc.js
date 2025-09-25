const path = require('node:path');
const grpc = require('@grpc/grpc-js');
const protoLoader = require('@grpc/proto-loader');
const { DeviceRegistry } = require('./registry');

const PROTO_PATH = path.join(__dirname, '..', 'proto', 'device_registry.proto');

function loadProto() {
  const packageDefinition = protoLoader.loadSync(PROTO_PATH, {
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
    includeDirs: [path.join(__dirname, '..', 'proto')]
  });
  return grpc.loadPackageDefinition(packageDefinition).lokanos.device_registry;
}

function toStruct(value) {
  return value;
}

function createGrpcServer(registry = new DeviceRegistry()) {
  const proto = loadProto();
  const server = new grpc.Server();

  server.addService(proto.DeviceRegistryService.service, {
    ListDevices: (call, callback) => {
      callback(null, { devices: registry.listDevices().map(serializeDevice) });
    },
    GetDevice: (call, callback) => {
      const device = registry.getDevice(call.request.value);
      if (!device) {
        callback({ code: grpc.status.NOT_FOUND, message: 'Device not found' });
        return;
      }
      callback(null, serializeDevice(device));
    },
    CreateDevice: (call, callback) => {
      try {
        const device = registry.createDevice(deserializeDevice(call.request));
        callback(null, serializeDevice(device));
      } catch (error) {
        callback({ code: grpc.status.INVALID_ARGUMENT, message: error.message });
      }
    },
    UpdateDevice: (call, callback) => {
      try {
        const device = registry.updateDevice(call.request.id, deserializeDevice(call.request));
        callback(null, serializeDevice(device));
      } catch (error) {
        const code = error.message.includes('not found') ? grpc.status.NOT_FOUND : grpc.status.INVALID_ARGUMENT;
        callback({ code, message: error.message });
      }
    },
    DeleteDevice: (call, callback) => {
      const deleted = registry.deleteDevice(call.request.value);
      if (!deleted) {
        callback({ code: grpc.status.NOT_FOUND, message: 'Device not found' });
        return;
      }
      callback(null, {});
    },
    UpdateDeviceState: (call, callback) => {
      try {
        const event = registry.updateDeviceState(call.request.device_id, call.request.state || {});
        callback(null, serializeStateEvent(event));
      } catch (error) {
        const code = error.message.includes('not found') ? grpc.status.NOT_FOUND : grpc.status.INVALID_ARGUMENT;
        callback({ code, message: error.message });
      }
    },
    SubscribeDeviceStates: (call) => {
      const sendEvent = (event) => {
        call.write(serializeStateEvent(event));
      };

      const unsubscribe = registry.subscribeToStateUpdates(sendEvent);
      call.on('cancelled', () => {
        unsubscribe();
      });
      call.on('end', () => {
        unsubscribe();
        call.end();
      });
    }
  });

  return { server, proto };
}

function serializeDevice(device) {
  return {
    id: device.id,
    room_id: device.roomId,
    capabilities: device.capabilities,
    state: toStruct(device.state)
  };
}

function deserializeDevice(message) {
  return {
    id: message.id,
    roomId: message.room_id,
    capabilities: Array.isArray(message.capabilities) ? message.capabilities : [],
    state: message.state || {}
  };
}

function serializeStateEvent(event) {
  return {
    device_id: event.deviceId,
    state: toStruct(event.state)
  };
}

module.exports = {
  createGrpcServer,
  loadProto
};
