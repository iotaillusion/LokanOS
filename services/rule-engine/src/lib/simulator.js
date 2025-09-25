'use strict';

const { deepClone, getValueByPath } = require('./utils');

function evaluateTrigger(trigger, inputs, logs) {
  switch (trigger.type) {
    case 'event': {
      const eventInput = inputs.trigger || null;
      if (!eventInput || typeof eventInput !== 'object') {
        logs.push(`Trigger ${trigger.event} did not match: missing trigger input.`);
        return false;
      }
      if (eventInput.type && eventInput.type !== 'event') {
        logs.push(`Trigger ${trigger.event} did not match: type mismatch.`);
        return false;
      }
      if (eventInput.event !== trigger.event) {
        logs.push(`Trigger ${trigger.event} did not match: received ${eventInput.event || 'unknown'}.`);
        return false;
      }
      if (trigger.source && eventInput.source && trigger.source !== eventInput.source) {
        logs.push(`Trigger ${trigger.event} did not match: expected source ${trigger.source}.`);
        return false;
      }
      logs.push(`Trigger ${trigger.event} matched.`);
      return true;
    }
    default:
      logs.push(`Unsupported trigger type: ${trigger.type}.`);
      return false;
  }
}

function evaluateTriggers(triggers, inputs, logs) {
  if (!Array.isArray(triggers) || triggers.length === 0) {
    logs.push('No triggers defined; defaulting to triggered state.');
    return true;
  }
  for (const trigger of triggers) {
    if (!evaluateTrigger(trigger, inputs, logs)) {
      return false;
    }
  }
  return true;
}

function evaluateComparison(operator, actual, expected) {
  switch (operator) {
    case 'eq':
      return actual === expected;
    case 'neq':
      return actual !== expected;
    case 'gt':
      return typeof actual === 'number' && typeof expected === 'number' && actual > expected;
    case 'gte':
      return typeof actual === 'number' && typeof expected === 'number' && actual >= expected;
    case 'lt':
      return typeof actual === 'number' && typeof expected === 'number' && actual < expected;
    case 'lte':
      return typeof actual === 'number' && typeof expected === 'number' && actual <= expected;
    case 'contains':
      if (Array.isArray(actual)) {
        return actual.some((value) => value === expected);
      }
      if (typeof actual === 'string') {
        return typeof expected === 'string' && actual.includes(expected);
      }
      return false;
    case 'in':
      if (Array.isArray(expected)) {
        return expected.some((value) => value === actual);
      }
      return false;
    default:
      return false;
  }
}

function evaluateConditions(conditions, inputs, logs) {
  if (!Array.isArray(conditions) || conditions.length === 0) {
    logs.push('No conditions defined; assuming rule is satisfied.');
    return true;
  }
  let allMet = true;
  for (const condition of conditions) {
    const actual = getValueByPath(inputs, condition.path);
    const result = evaluateComparison(condition.operator, actual, condition.value);
    logs.push(
      `Condition ${condition.path} ${condition.operator} ${JSON.stringify(condition.value)} => ${result}`
    );
    if (!result) {
      allMet = false;
    }
  }
  return allMet;
}

function cloneAction(action) {
  const clone = { type: action.type };
  if (action.target !== undefined) {
    clone.target = action.target;
  }
  if (action.payload !== undefined) {
    clone.payload = deepClone(action.payload);
  }
  if (action.parameters !== undefined) {
    clone.parameters = deepClone(action.parameters);
  }
  return clone;
}

function simulate(rule, inputs = {}, context = {}) {
  const logs = [];
  const effectiveInputs = { ...inputs, context: deepClone(context) };

  const triggered = evaluateTriggers(rule.triggers, effectiveInputs, logs);
  if (!triggered) {
    logs.push('Rule did not trigger; skipping actions.');
    return { actions: [], logs, triggered: false, conditionsMet: false };
  }

  const conditionsMet = evaluateConditions(rule.conditions, effectiveInputs, logs);
  if (!conditionsMet) {
    logs.push('Rule conditions not satisfied; no actions returned.');
    return { actions: [], logs, triggered: true, conditionsMet: false };
  }

  const actions = rule.actions.map(cloneAction);
  logs.push(`Returning ${actions.length} action(s).`);
  return { actions, logs, triggered: true, conditionsMet: true };
}

module.exports = {
  simulate
};
