'use strict';

const { parseRuleDefinition, RuleValidationError } = require('../src/lib/parser');

describe('parseRuleDefinition', () => {
  const validRule = {
    id: 'rule-1',
    name: 'Test Rule',
    description: 'Example rule for parsing tests',
    triggers: [
      { type: 'event', event: 'motion.detected', source: 'sensor.entry' }
    ],
    conditions: [
      { type: 'comparison', operator: 'eq', path: 'sensors.motion', value: true },
      { type: 'comparison', operator: 'lt', path: 'environment.lux', value: 60 }
    ],
    actions: [
      {
        type: 'device.command',
        target: 'light.living-room',
        payload: { command: 'set_power', value: 'on' }
      }
    ]
  };

  it('normalizes a valid rule definition', () => {
    const parsed = parseRuleDefinition(validRule);
    expect(parsed).toEqual({
      id: 'rule-1',
      name: 'Test Rule',
      description: 'Example rule for parsing tests',
      triggers: [
        { type: 'event', event: 'motion.detected', source: 'sensor.entry' }
      ],
      conditions: [
        { type: 'comparison', operator: 'eq', path: 'sensors.motion', value: true },
        { type: 'comparison', operator: 'lt', path: 'environment.lux', value: 60 }
      ],
      actions: [
        {
          type: 'device.command',
          target: 'light.living-room',
          payload: { command: 'set_power', value: 'on' }
        }
      ]
    });
    expect(parsed.actions[0]).not.toBe(validRule.actions[0]);
  });

  it('throws a RuleValidationError when definition is invalid', () => {
    const invalidRule = { ...validRule, triggers: [] };
    expect(() => parseRuleDefinition(invalidRule)).toThrow(RuleValidationError);
    try {
      parseRuleDefinition(invalidRule);
    } catch (error) {
      expect(error.errors).toBeDefined();
      expect(Array.isArray(error.errors)).toBe(true);
    }
  });
});
