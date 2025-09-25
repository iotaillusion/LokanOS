const request = require('supertest');
const { PresenceStore, PresenceValidationError } = require('../src/store');
const { createPresenceApp } = require('../src/rest');

describe('PresenceStore', () => {
  it('creates and retrieves presence entries with generated timestamps', () => {
    const clock = jest.fn(() => new Date('2024-06-01T10:00:00.000Z'));
    const store = new PresenceStore({ clock });

    const record = store.setPresence('user-alice', { state: 'home' });
    expect(record).toEqual({
      userId: 'user-alice',
      state: 'home',
      lastSeenAt: '2024-06-01T10:00:00.000Z'
    });

    const fetched = store.getPresence('user-alice');
    expect(fetched).toEqual(record);
    expect(store.listPresence()).toEqual([record]);
  });

  it('uses provided timestamps when supplied', () => {
    const store = new PresenceStore();
    const record = store.setPresence('user-bob', {
      state: 'away',
      lastSeenAt: '2024-06-01T11:30:00Z'
    });
    expect(record.lastSeenAt).toBe('2024-06-01T11:30:00.000Z');
  });

  it('rejects invalid updates', () => {
    const store = new PresenceStore();
    expect(() => store.setPresence('', { state: 'home' })).toThrow(PresenceValidationError);
    expect(() => store.setPresence('user-carol', { state: 'invalid' })).toThrow('state must be either "home" or "away"');
    expect(() => store.setPresence('user-dan', { state: 'home', lastSeenAt: 'not-a-date' })).toThrow(
      'lastSeenAt must be a valid ISO-8601 timestamp'
    );
  });
});

describe('Presence REST API', () => {
  let store;
  let app;

  beforeEach(() => {
    store = new PresenceStore({ clock: () => new Date('2024-06-01T12:00:00.000Z') });
    ({ app } = createPresenceApp({ store }));
  });

  it('supports creating and reading presence records', async () => {
    const client = request(app);

    const createResponse = await client.put('/presence/user-alice').send({ state: 'home' });
    expect(createResponse.status).toBe(200);
    expect(createResponse.body).toEqual({
      userId: 'user-alice',
      state: 'home',
      lastSeenAt: '2024-06-01T12:00:00.000Z'
    });

    const getResponse = await client.get('/presence/user-alice');
    expect(getResponse.status).toBe(200);
    expect(getResponse.body).toEqual(createResponse.body);

    const listResponse = await client.get('/presence');
    expect(listResponse.status).toBe(200);
    expect(listResponse.body).toEqual({ presences: [createResponse.body] });
  });

  it('validates payloads', async () => {
    const response = await request(app).put('/presence/user-alice').send({ state: 'invalid' });
    expect(response.status).toBe(400);
    expect(response.body).toEqual({ error: 'state must be either "home" or "away"' });
  });

  it('returns 404 for unknown users', async () => {
    const response = await request(app).get('/presence/user-missing');
    expect(response.status).toBe(404);
    expect(response.body).toEqual({ error: 'Presence for user user-missing not found' });
  });
});
