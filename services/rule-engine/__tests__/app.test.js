'use strict';

const request = require('supertest');
const { createApp } = require('../src/app');

describe('rule-engine HTTP API', () => {
  const app = createApp();
  const baseRule = {
    id: 'rule-light-on',
    name: 'Turn on light',
    triggers: [{ type: 'event', event: 'motion.detected', source: 'sensor.entry' }],
    conditions: [{ type: 'comparison', operator: 'eq', path: 'sensors.motion', value: true }],
    actions: [{ type: 'device.command', target: 'light.entry', payload: { command: 'set_power', value: 'on' } }]
  };

  it('accepts a valid request and returns simulation results', async () => {
    const response = await request(app)
      .post('/v1/rules:test')
      .send({
        ruleId: 'rule-light-on',
        rule: baseRule,
        inputs: {
          trigger: { type: 'event', event: 'motion.detected', source: 'sensor.entry' },
          sensors: { motion: true }
        }
      });

    expect(response.status).toBe(200);
    expect(response.body).toMatchObject({
      ruleId: 'rule-light-on',
      status: 'passed'
    });
    expect(Array.isArray(response.body.actions)).toBe(true);
    expect(response.body.actions).toHaveLength(1);
  });

  it('rejects invalid payloads with a 400 response', async () => {
    const response = await request(app)
      .post('/v1/rules:test')
      .send({ ruleId: 'missing-rule', inputs: {} });

    expect(response.status).toBe(400);
    expect(response.body.error).toBe('Invalid rule test request');
  });
});
