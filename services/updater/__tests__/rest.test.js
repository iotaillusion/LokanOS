const request = require('supertest');
const { createUpdaterApp } = require('../src/rest');
const { UpdaterStateMachine } = require('../src/state-machine');

describe('Updater REST API', () => {
  let machine;
  let app;

  beforeEach(() => {
    machine = new UpdaterStateMachine({
      slotA: { version: '1.0.0' },
      slotB: { version: null },
      activeSlot: 'slotA',
      healthFailWindow: 2
    });
    ({ app } = createUpdaterApp({ machine }));
  });

  it('reports update status', async () => {
    const response = await request(app).post('/updates/check');
    expect(response.status).toBe(200);
    expect(response.body.activeSlot).toBe('slotA');
    expect(response.body.activeVersion).toBe('1.0.0');
  });

  it('stages and activates an update via apply endpoint', async () => {
    const response = await request(app).post('/updates/apply').send({ version: '2.0.0' });
    expect(response.status).toBe(202);
    expect(response.body.status).toBe('staged');
    expect(response.body.state.activeSlot).toBe('slotB');
    expect(response.body.state.rollbackSlot).toBe('slotA');

    const status = await request(app).post('/updates/check');
    expect(status.body.trialSlot).toBe('slotB');
  });

  it('finalizes an update when apply is called with finalize flag', async () => {
    await request(app).post('/updates/apply').send({ version: '2.0.0' });
    const response = await request(app).post('/updates/apply').send({ finalize: true });
    expect(response.status).toBe(200);
    expect(response.body.status).toBe('committed');
    expect(response.body.state.rollbackSlot).toBeNull();
  });

  it('records unhealthy boots and triggers rollback through health endpoint', async () => {
    await request(app).post('/updates/apply').send({ version: '2.0.0' });
    const first = await request(app).post('/updates/health/unhealthy');
    expect(first.body.status).toBe('recorded');
    expect(first.body.state.unhealthyBoots).toBe(1);

    const second = await request(app).post('/updates/health/unhealthy');
    expect(second.body.status).toBe('rolled_back');
    expect(second.body.rolledBack).toBe(true);
    expect(second.body.state.activeSlot).toBe('slotA');
  });
});
