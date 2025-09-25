const request = require('supertest');
const { createApp } = require('../src/server');
const { topology } = require('../src/routes');

describe('api-gateway server', () => {
  let app;

  beforeAll(() => {
    app = createApp();
  });

  describe('GET /healthz', () => {
    it('returns a healthy status payload', async () => {
      const response = await request(app).get('/healthz');
      expect(response.status).toBe(200);
      expect(response.body).toEqual({ status: 'ok' });
    });
  });

  describe('GET /v1/topology', () => {
    it('returns deterministic topology data', async () => {
      const response = await request(app).get('/v1/topology');
      expect(response.status).toBe(200);
      expect(response.body).toEqual(topology);
    });
  });
});
