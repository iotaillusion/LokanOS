const request = require('supertest');
const { SceneStore } = require('../src/scenes');
const { SceneValidationError, SceneNotFoundError } = require('../src/errors');
const { createSceneApp } = require('../src/rest');

describe('SceneStore', () => {
  let deviceClient;
  let store;

  beforeEach(() => {
    deviceClient = {
      ensureDevicesExist: jest.fn().mockResolvedValue(undefined)
    };
    store = new SceneStore({ deviceClient });
  });

  it('creates, updates, applies, and deletes scenes', async () => {
    const created = await store.createScene({
      id: 'scene-morning',
      name: 'Morning Routine',
      items: [
        { deviceId: 'device-light-1', desiredState: { power: 'on' } },
        { deviceId: 'device-coffee', desiredState: { brew: true } }
      ]
    });

    expect(created).toMatchObject({ id: 'scene-morning', name: 'Morning Routine' });
    expect(deviceClient.ensureDevicesExist).toHaveBeenCalledWith(['device-light-1', 'device-coffee']);

    const fetched = store.getScene('scene-morning');
    expect(fetched).toEqual(created);

    const list = store.listScenes();
    expect(list).toEqual([created]);

    const updated = await store.updateScene('scene-morning', {
      name: 'Early Morning',
      items: [
        { deviceId: 'device-light-1', desiredState: { power: 'on', brightness: 50 } }
      ]
    });

    expect(updated).toMatchObject({
      id: 'scene-morning',
      name: 'Early Morning',
      items: [
        { deviceId: 'device-light-1', desiredState: { power: 'on', brightness: 50 } }
      ]
    });

    const plan = await store.applyScene('scene-morning');
    expect(plan).toEqual({
      sceneId: 'scene-morning',
      status: 'planned',
      steps: [
        {
          order: 1,
          deviceId: 'device-light-1',
          desiredState: { power: 'on', brightness: 50 }
        }
      ]
    });

    const deleted = store.deleteScene('scene-morning');
    expect(deleted).toBe(true);
    expect(store.getScene('scene-morning')).toBeNull();
  });

  it('rejects when referenced devices do not exist', async () => {
    deviceClient.ensureDevicesExist.mockRejectedValueOnce(
      new SceneValidationError('Device device-missing not found in registry')
    );

    await expect(
      store.createScene({
        id: 'scene-invalid',
        name: 'Invalid Scene',
        items: [{ deviceId: 'device-missing', desiredState: { power: 'on' } }]
      })
    ).rejects.toThrow(SceneValidationError);
  });

  it('throws not found when updating unknown scenes', async () => {
    await expect(store.updateScene('scene-unknown', { name: 'x' })).rejects.toThrow(SceneNotFoundError);
  });
});

describe('Scene REST API', () => {
  let deviceClient;
  let store;
  let app;

  beforeEach(() => {
    deviceClient = {
      ensureDevicesExist: jest.fn().mockResolvedValue(undefined)
    };
    store = new SceneStore({ deviceClient });
    app = createSceneApp({ store }).app;
  });

  it('performs CRUD operations over HTTP', async () => {
    const client = request(app);

    const createResponse = await client.post('/scenes').send({
      id: 'scene-evening',
      name: 'Evening Relax',
      items: [{ deviceId: 'device-light-1', desiredState: { power: 'on', brightness: 20 } }]
    });
    expect(createResponse.status).toBe(201);
    expect(createResponse.body).toMatchObject({ id: 'scene-evening', name: 'Evening Relax' });

    const listResponse = await client.get('/scenes');
    expect(listResponse.status).toBe(200);
    expect(listResponse.body.scenes).toHaveLength(1);

    const updateResponse = await client
      .put('/scenes/scene-evening')
      .send({ name: 'Evening Chill' });
    expect(updateResponse.status).toBe(200);
    expect(updateResponse.body).toMatchObject({ name: 'Evening Chill' });

    const applyResponse = await client.post('/scenes/scene-evening/apply');
    expect(applyResponse.status).toBe(202);
    expect(applyResponse.body).toMatchObject({ sceneId: 'scene-evening', status: 'planned' });

    const deleteResponse = await client.delete('/scenes/scene-evening');
    expect(deleteResponse.status).toBe(204);
  });

  it('returns validation errors when device validation fails', async () => {
    deviceClient.ensureDevicesExist.mockRejectedValueOnce(
      new SceneValidationError('Device device-light-2 not found in registry')
    );

    const response = await request(app).post('/scenes').send({
      id: 'scene-invalid',
      name: 'Invalid',
      items: [{ deviceId: 'device-light-2', desiredState: { power: 'on' } }]
    });

    expect(response.status).toBe(400);
    expect(response.body).toEqual({ error: 'Device device-light-2 not found in registry' });
  });

  it('returns 404 when scene not found', async () => {
    const response = await request(app).post('/scenes/scene-missing/apply');
    expect(response.status).toBe(404);
    expect(response.body).toEqual({ error: 'Scene scene-missing not found' });
  });
});
