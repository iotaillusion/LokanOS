'use strict';

const Ajv = require('ajv');
const { deepClone } = require('./utils');

const ajv = new Ajv({ allErrors: true, strict: true });

const triggerSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['type', 'event'],
  properties: {
    type: { enum: ['event'] },
    event: { type: 'string', minLength: 1 },
    source: { type: 'string', minLength: 1 },
    criteria: {
      type: 'object',
      additionalProperties: {
        anyOf: [
          { type: 'string' },
          { type: 'number' },
          { type: 'boolean' },
          { type: 'null' }
        ]
      }
    }
  }
};

const conditionSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['type', 'operator', 'path', 'value'],
  properties: {
    type: { const: 'comparison' },
    operator: { enum: ['eq', 'neq', 'gt', 'gte', 'lt', 'lte', 'contains', 'in'] },
    path: { type: 'string', minLength: 1 },
    value: {}
  }
};

const actionSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['type'],
  properties: {
    type: { type: 'string', minLength: 1 },
    target: { type: 'string', minLength: 1 },
    payload: { type: 'object', additionalProperties: true },
    parameters: { type: 'object', additionalProperties: true }
  }
};

const ruleSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['triggers', 'conditions', 'actions'],
  properties: {
    id: { type: 'string', minLength: 1 },
    name: { type: 'string', minLength: 1 },
    description: { type: 'string', minLength: 1 },
    triggers: {
      type: 'array',
      minItems: 1,
      items: triggerSchema
    },
    conditions: {
      type: 'array',
      items: conditionSchema
    },
    actions: {
      type: 'array',
      minItems: 1,
      items: actionSchema
    }
  }
};

const validateRule = ajv.compile(ruleSchema);

class RuleValidationError extends Error {
  constructor(message, errors) {
    super(message);
    this.name = 'RuleValidationError';
    this.errors = errors;
  }
}

function normalizeTrigger(trigger) {
  const normalized = { type: trigger.type };
  if (trigger.event) {
    normalized.event = trigger.event;
  }
  if (trigger.source) {
    normalized.source = trigger.source;
  }
  if (trigger.criteria) {
    normalized.criteria = deepClone(trigger.criteria);
  }
  return normalized;
}

function normalizeCondition(condition) {
  return {
    type: 'comparison',
    operator: condition.operator,
    path: condition.path,
    value: deepClone(condition.value)
  };
}

function normalizeAction(action) {
  const normalized = { type: action.type };
  if (action.target) {
    normalized.target = action.target;
  }
  if (action.payload) {
    normalized.payload = deepClone(action.payload);
  }
  if (action.parameters) {
    normalized.parameters = deepClone(action.parameters);
  }
  return normalized;
}

function parseRuleDefinition(rule) {
  if (!validateRule(rule)) {
    throw new RuleValidationError('Invalid rule definition', validateRule.errors);
  }
  return {
    id: rule.id || null,
    name: rule.name || null,
    description: rule.description || null,
    triggers: rule.triggers.map(normalizeTrigger),
    conditions: rule.conditions.map(normalizeCondition),
    actions: rule.actions.map(normalizeAction)
  };
}

module.exports = {
  parseRuleDefinition,
  RuleValidationError,
  ruleSchema
};
