'use strict';

const express = require('express');
const Ajv = require('ajv');
const { parseRuleDefinition, RuleValidationError, ruleSchema } = require('./lib/parser');
const { simulate } = require('./lib/simulator');

const ajv = new Ajv({ allErrors: true, strict: true });

const ruleTestRequestSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['ruleId', 'rule', 'inputs'],
  properties: {
    ruleId: { type: 'string', minLength: 1 },
    rule: ruleSchema,
    inputs: { type: 'object', additionalProperties: true },
    context: { type: 'object', additionalProperties: true }
  }
};

const validateRuleTestRequest = ajv.compile(ruleTestRequestSchema);

function formatValidationErrors(errors) {
  if (!errors) {
    return [];
  }
  return errors.map((error) => ({
    message: error.message,
    instancePath: error.instancePath,
    schemaPath: error.schemaPath
  }));
}

function createApp() {
  const app = express();
  app.disable('x-powered-by');
  app.use(express.json({ strict: true }));

  app.post('/v1/rules:test', (req, res) => {
    const payload = req.body;
    if (!validateRuleTestRequest(payload)) {
      res.status(400).json({
        error: 'Invalid rule test request',
        details: formatValidationErrors(validateRuleTestRequest.errors)
      });
      return;
    }

    try {
      const parsedRule = parseRuleDefinition(payload.rule);
      const simulation = simulate(parsedRule, payload.inputs || {}, payload.context || {});
      const status = simulation.triggered && simulation.conditionsMet ? 'passed' : 'failed';
      res.json({
        ruleId: payload.ruleId,
        status,
        logs: simulation.logs,
        actions: simulation.actions,
        errors: []
      });
    } catch (error) {
      if (error instanceof RuleValidationError) {
        res.status(400).json({
          error: 'Invalid rule definition',
          details: formatValidationErrors(error.errors)
        });
        return;
      }
      // eslint-disable-next-line no-console
      console.error('rule-engine: unexpected error', error);
      res.status(500).json({ error: 'Failed to simulate rule' });
    }
  });

  app.use((err, req, res, next) => {
    if (err instanceof SyntaxError && 'body' in err) {
      res.status(400).json({ error: 'Invalid JSON payload' });
      return;
    }
    next(err);
  });

  return app;
}

module.exports = {
  createApp,
  ruleTestRequestSchema,
  formatValidationErrors
};
