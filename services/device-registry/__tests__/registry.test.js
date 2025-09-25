const request = require('supertest');
const { DeviceRegistry } = require('../src/registry');
const { DeviceId } = require('../src/types');
const { createRestApp } = require('../src/rest');

describe('DeviceRegistry', () => {
  let registry;

  beforeEach(() => {
    registry = new DeviceRegistry();
  });

  it('creates, reads, updates, and deletes devices', () => {
    const created = registry.createDevice({
      id: 'device-1',
      roomId: 'room-1',
      capabilities: ['light'],
      state: { power: 'off' }
    });

    expect(created).toMatchObject({
      id: 'device-1',
      roomId: 'room-1',
      capabilities: ['light'],
      state: { power: 'off' }
    });

    const fetched = registry.getDevice(new DeviceId('device-1'));
    expect(fetched).toEqual(created);

    const updated = registry.updateDevice('device-1', {
      roomId: 'room-2',
      capabilities: ['light', 'dimming'],
      state: { power: 'on' }
    });

    expect(updated).toMatchObject({
      id: 'device-1',
      roomId: 'room-2',
      capabilities: ['light', 'dimming'],
      state: { power: 'on' }
    });

    const deleted = registry.deleteDevice('device-1');
    expect(deleted).toBe(true);
    expect(registry.getDevice('device-1')).toBeNull();
  });

  it('notifies subscribers on state updates', () => {
    registry.createDevice({
      id: 'device-1',
      roomId: 'room-1',
      capabilities: ['light'],
      state: { power: 'off' }
    });

    const events = [];
    const unsubscribe = registry.subscribeToStateUpdates((event) => {
      events.push(event);
    });

    const event = registry.updateDeviceState('device-1', { power: 'on' });
    expect(event).toEqual({
      deviceId: 'device-1',
      state: { power: 'on' }
    });

    unsubscribe();

    expect(events).toEqual([
      {
        deviceId: 'device-1',
        state: { power: 'on' }
      }
    ]);
  });
});

describe('REST API', () => {
  it('performs CRUD operations over HTTP', async () => {
    const { app, registry } = createRestApp(new DeviceRegistry());
    const client = request(app);

    const createResponse = await client
      .post('/devices')
      .send({ id: 'device-1', roomId: 'room-1', capabilities: ['light'], state: { power: 'off' } });

    expect(createResponse.status).toBe(201);
    expect(createResponse.body).toMatchObject({ id: 'device-1', roomId: 'room-1' });

    const listResponse = await client.get('/devices');
    expect(listResponse.status).toBe(200);
    expect(listResponse.body.devices).toHaveLength(1);

    const updateResponse = await client
      .put('/devices/device-1')
      .send({ roomId: 'room-2', state: { power: 'on' } });
    expect(updateResponse.status).toBe(200);
    expect(updateResponse.body).toMatchObject({ roomId: 'room-2', state: { power: 'on' } });

    const stateResponse = await client
      .post('/devices/device-1/state')
      .send({ brightness: 50 });
    expect(stateResponse.status).toBe(200);
    expect(stateResponse.body).toMatchObject({ deviceId: 'device-1', state: { power: 'on', brightness: 50 } });

    const deleteResponse = await client.delete('/devices/device-1');
    expect(deleteResponse.status).toBe(204);
    expect(registry.getDevice('device-1')).toBeNull();
  });
});
