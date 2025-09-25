'use strict';

const { parseRuleDefinition } = require('../src/lib/parser');
const { simulate } = require('../src/lib/simulator');

describe('simulate', () => {
  const definition = {
    id: 'rule-light-on',
    name: 'Turn on light when motion detected',
    triggers: [{ type: 'event', event: 'motion.detected', source: 'sensor.entry' }],
    conditions: [
      { type: 'comparison', operator: 'eq', path: 'sensors.motion', value: true },
      { type: 'comparison', operator: 'lt', path: 'environment.lux', value: 50 }
    ],
    actions: [
      {
        type: 'device.command',
        target: 'light.entry',
        payload: { command: 'set_power', value: 'on' }
      }
    ]
  };
  const parsedRule = parseRuleDefinition(definition);

  it('returns actions when triggers and conditions match', () => {
    const result = simulate(parsedRule, {
      trigger: { type: 'event', event: 'motion.detected', source: 'sensor.entry' },
      sensors: { motion: true },
      environment: { lux: 30 }
    });

    expect(result.triggered).toBe(true);
    expect(result.conditionsMet).toBe(true);
    expect(result.actions).toHaveLength(1);
    expect(result.actions[0]).toEqual({
      type: 'device.command',
      target: 'light.entry',
      payload: { command: 'set_power', value: 'on' }
    });
    expect(result.actions[0]).not.toBe(parsedRule.actions[0]);
    expect(Array.isArray(result.logs)).toBe(true);
    expect(result.logs.some((line) => line.includes('Returning 1 action'))).toBe(true);
  });

  it('returns no actions when triggers fail to match', () => {
    const result = simulate(parsedRule, {
      trigger: { type: 'event', event: 'motion.stopped', source: 'sensor.entry' },
      sensors: { motion: false },
      environment: { lux: 80 }
    });

    expect(result.triggered).toBe(false);
    expect(result.conditionsMet).toBe(false);
    expect(result.actions).toHaveLength(0);
    expect(result.logs.some((line) => line.includes('did not match'))).toBe(true);
  });

  it('returns no actions when conditions fail', () => {
    const result = simulate(parsedRule, {
      trigger: { type: 'event', event: 'motion.detected', source: 'sensor.entry' },
      sensors: { motion: true },
      environment: { lux: 80 }
    });

    expect(result.triggered).toBe(true);
    expect(result.conditionsMet).toBe(false);
    expect(result.actions).toHaveLength(0);
    expect(result.logs.some((line) => line.includes('not satisfied'))).toBe(true);
  });
});
