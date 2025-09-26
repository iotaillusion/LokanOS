#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

function main() {
  const repoRoot = path.resolve(__dirname, '..');
  const bundlePath = path.join(repoRoot, 'openapi', '_bundle.json');
  if (!fs.existsSync(bundlePath)) {
    throw new Error(`OpenAPI bundle not found at ${bundlePath}`);
  }
  const raw = fs.readFileSync(bundlePath, 'utf8');
  const spec = JSON.parse(raw);

  const outputDir = path.join(repoRoot, 'sdks', 'typescript');
  fs.mkdirSync(outputDir, { recursive: true });

  const schemaGen = createSchemaGenerator(spec);
  const typesTs = schemaGen.generateTypesFile();
  fs.writeFileSync(path.join(outputDir, 'types.ts'), typesTs);

  const clientGen = createClientGenerator(spec, schemaGen.typeFromSchema);
  const clientResult = clientGen.generate();
  fs.writeFileSync(path.join(outputDir, 'client.ts'), clientResult.ts);
  fs.writeFileSync(path.join(outputDir, 'client.js'), clientResult.js);

  const typesJsPath = path.join(outputDir, 'types.js');
  if (!fs.existsSync(typesJsPath)) {
    fs.writeFileSync(
      typesJsPath,
      [
        '/**',
        ' * Runtime placeholder for generated TypeScript types.',
        ' * The SDK exposes ESM modules, so we ship an empty module to satisfy bundlers.',
        ' */',
        'export {};',
        ''
      ].join('\n')
    );
  }

  console.log('Generated TypeScript SDK in sdks/typescript');
}

function createSchemaGenerator(spec) {
  const schemas = (spec.components && spec.components.schemas) || {};

  function typeFromSchema(schema, indentLevel = 0) {
    if (!schema || typeof schema !== 'object') {
      return { type: 'unknown', imports: new Set() };
    }

    if (schema.$ref) {
      const refMatch = /^#\/components\/schemas\/(.+)$/.exec(schema.$ref);
      if (refMatch) {
        return { type: refMatch[1], imports: new Set([refMatch[1]]) };
      }
      return { type: 'unknown', imports: new Set() };
    }

    if (Array.isArray(schema.enum) && schema.enum.length > 0) {
      const enumValues = schema.enum.map((value) => JSON.stringify(value));
      let enumType = enumValues.join(' | ');
      if (schema.nullable) {
        enumType += ' | null';
      }
      return { type: enumType, imports: new Set() };
    }

    const resultImports = new Set();
    const nullable = Boolean(schema.nullable);
    const effectiveType = schema.type || inferTypeFromSchema(schema);

    if (effectiveType === 'object') {
      const properties = schema.properties || {};
      const required = new Set(schema.required || []);
      const lines = ['{'];
      const propertyNames = Object.keys(properties).sort((a, b) => a.localeCompare(b));
      for (const propName of propertyNames) {
        const propSchema = properties[propName];
        const propResult = typeFromSchema(propSchema, indentLevel + 1);
        mergeImports(resultImports, propResult.imports);
        const key = formatPropertyKey(propName);
        const optional = !required.has(propName);
        const propLines = formatMultilineType(propResult.type, indentLevel + 1);
        const optionalMark = optional ? '?' : '';
        lines.push(`${indent(indentLevel + 1)}${key}${optionalMark}: ${propLines};`);
      }
      if (schema.additionalProperties !== undefined) {
        const additional = schema.additionalProperties;
        let additionalType = 'unknown';
        if (additional && typeof additional === 'object') {
          const additionalResult = typeFromSchema(additional, indentLevel + 1);
          mergeImports(resultImports, additionalResult.imports);
          additionalType = additionalResult.type;
        } else if (additional === true) {
          additionalType = 'unknown';
        } else {
          additionalType = 'never';
        }
        lines.push(`${indent(indentLevel + 1)}[key: string]: ${additionalType};`);
      }
      lines.push(`${indent(indentLevel)}}`);
      let objectType = lines.join('\n');
      if (nullable) {
        objectType += ' | null';
      }
      return { type: objectType, imports: resultImports };
    }

    if (effectiveType === 'array') {
      const itemsResult = typeFromSchema(schema.items || {}, indentLevel + 1);
      mergeImports(resultImports, itemsResult.imports);
      const arrayType = wrapArrayType(itemsResult.type);
      let finalType = `${arrayType}[]`;
      if (nullable) {
        finalType += ' | null';
      }
      return { type: finalType, imports: resultImports };
    }

    let primitiveType = 'unknown';
    switch (effectiveType) {
      case 'string':
        primitiveType = 'string';
        break;
      case 'integer':
      case 'number':
        primitiveType = 'number';
        break;
      case 'boolean':
        primitiveType = 'boolean';
        break;
      case 'null':
        primitiveType = 'null';
        break;
      default:
        primitiveType = 'unknown';
    }

    if (nullable && primitiveType !== 'null') {
      primitiveType += ' | null';
    }
    return { type: primitiveType, imports: resultImports };
  }

  function generateTypesFile() {
    const header = [
      '/**',
      ' * This file is auto-generated by tools/oas2ts.ts.',
      ' * Do not edit this file directly.',
      ' */',
      ''
    ];

    const schemaEntries = Object.entries(schemas).sort((a, b) => a[0].localeCompare(b[0]));
    const body = [];

    for (const [name, schema] of schemaEntries) {
      const result = typeFromSchema(schema, 0);
      body.push(`export type ${name} = ${result.type};`);
    }

    return header.concat(body.join('\n\n')).join('\n') + '\n';
  }

  return { generateTypesFile, typeFromSchema };
}

function createClientGenerator(spec, typeFromSchema) {
  function generate() {
    const operations = collectOperations(spec, typeFromSchema);
    const usedSchemaImports = new Set();
    const tsLines = [
      '/**',
      ' * This file is auto-generated by tools/oas2ts.ts.',
      ' * Thin client helpers for the LokanOS API bundle.',
      ' */',
      ''
    ];
    const jsLines = [
      '/**',
      ' * Runtime client generated from the LokanOS OpenAPI document.',
      ' * This module is intentionally dependency-free.',
      ' */',
      ''
    ];

    for (const op of operations) {
      for (const importName of op.imports) {
        usedSchemaImports.add(importName);
      }
    }

    const sortedImports = Array.from(usedSchemaImports).sort((a, b) => a.localeCompare(b));
    if (sortedImports.length > 0) {
      tsLines.push(`import type { ${sortedImports.join(', ')} } from './types.js';`);
      tsLines.push(`export type { ${sortedImports.join(', ')} } from './types.js';`);
      tsLines.push('');
    }

    tsLines.push(...generateRequestOptionsTs());
    tsLines.push('');
    tsLines.push(...generateHelpersTs());
    tsLines.push('');
    tsLines.push(...generateRequestFunctionTs());
    tsLines.push('');

    jsLines.push(...generateRequestOptionsJsComment());
    jsLines.push(...generateHelpersJs());
    jsLines.push('');
    jsLines.push(...generateRequestFunctionJs());
    jsLines.push('');

    for (const op of operations) {
      if (op.requestTypeDeclaration) {
        tsLines.push(op.requestTypeDeclaration);
        tsLines.push('');
      }
      tsLines.push(op.responseTypeDeclaration);
      tsLines.push('');
      tsLines.push(...op.tsFunctionLines);
      tsLines.push('');

      jsLines.push(...op.jsFunctionLines);
      jsLines.push('');
    }

    return { ts: tsLines.join('\n'), js: jsLines.join('\n') };
  }

  function generateRequestOptionsTs() {
    return [
      'export interface RequestOptions {',
      '  /** Base URL for the LokanOS API (e.g. https://api.example.com). */',
      '  baseUrl?: string;',
      '  /** Additional headers to apply to the request. */',
      '  headers?: Record<string, string>;',
      '  /** Optional body payload for the request. */',
      '  body?: unknown;',
      '  /** Custom fetch implementation, defaults to globalThis.fetch. */',
      '  fetch?: typeof fetch;',
      '}',
    ];
  }

  function generateRequestOptionsJsComment() {
    return [
      '/**',
      ' * @typedef {Object} RequestOptions',
      ' * @property {string} [baseUrl] Base URL for the LokanOS API.',
      ' * @property {Record<string, string>} [headers] Additional headers to send.',
      ' * @property {*} [body] Optional request body payload.',
      ' * @property {typeof fetch} [fetch] Custom fetch implementation.',
      ' */',
    ];
  }

  function generateHelpersTs() {
    return [
      "function joinUrl(baseUrl: string | undefined, apiPath: string): string {",
      "  if (!baseUrl) {",
      "    return apiPath;",
      "  }",
      "  if (/^https?:/i.test(apiPath)) {",
      "    return apiPath;",
      "  }",
      "  const normalizedBase = baseUrl.replace(/[/]+$/, '');",
      "  const normalizedPath = apiPath.startsWith('/') ? apiPath : `/${apiPath}`;",
      "  return `${normalizedBase}${normalizedPath}`;",
      "}",
      "",
      "function hasHeader(headers: Record<string, string>, name: string): boolean {",
      "  const lower = name.toLowerCase();",
      "  return Object.keys(headers).some((key) => key.toLowerCase() === lower);",
      "}",
      "",
      "function shouldSerializeJson(body: unknown): boolean {",
      "  if (body === null || body === undefined) {",
      "    return false;",
      "  }",
      "  if (typeof body === 'string') {",
      "    return false;",
      "  }",
      "  if (typeof ArrayBuffer !== 'undefined' && (body instanceof ArrayBuffer || ArrayBuffer.isView(body))) {",
      "    return false;",
      "  }",
      "  if (typeof Blob !== 'undefined' && body instanceof Blob) {",
      "    return false;",
      "  }",
      "  if (typeof FormData !== 'undefined' && body instanceof FormData) {",
      "    return false;",
      "  }",
      "  return typeof body === 'object';",
      "}",
    ];
  }

  function generateHelpersJs() {
    return [
      "function joinUrl(baseUrl, apiPath) {",
      "  if (!baseUrl) {",
      "    return apiPath;",
      "  }",
      "  if (/^https?:/i.test(apiPath)) {",
      "    return apiPath;",
      "  }",
      "  const normalizedBase = baseUrl.replace(/[/]+$/, '');",
      "  const normalizedPath = apiPath.startsWith('/') ? apiPath : `/${apiPath}`;",
      "  return `${normalizedBase}${normalizedPath}`;",
      "}",
      "",
      "function hasHeader(headers, name) {",
      "  const lower = name.toLowerCase();",
      "  return Object.keys(headers).some((key) => key.toLowerCase() === lower);",
      "}",
      "",
      "function shouldSerializeJson(body) {",
      "  if (body === null || body === undefined) {",
      "    return false;",
      "  }",
      "  if (typeof body === 'string') {",
      "    return false;",
      "  }",
      "  if (typeof ArrayBuffer !== 'undefined' && (body instanceof ArrayBuffer || ArrayBuffer.isView(body))) {",
      "    return false;",
      "  }",
      "  if (typeof Blob !== 'undefined' && body instanceof Blob) {",
      "    return false;",
      "  }",
      "  if (typeof FormData !== 'undefined' && body instanceof FormData) {",
      "    return false;",
      "  }",
      "  return typeof body === 'object';",
      "}",
    ];
  }

  function generateRequestFunctionTs() {
    return [
      "export async function request(path: string, method: string, options: RequestOptions = {}): Promise<Response> {",
      "  const { baseUrl, headers = {}, body, fetch: customFetch } = options;",
      "  const fetchImpl = customFetch || globalThis.fetch;",
      "  if (!fetchImpl) {",
      "    throw new Error('Fetch API is not available in this environment.');",
      "  }",
      "  const url = joinUrl(baseUrl, path);",
      "  const finalHeaders: Record<string, string> = { ...headers };",
      "  let finalBody: BodyInit | undefined;",
      "  if (body !== undefined) {",
      "    if (shouldSerializeJson(body)) {",
      "      if (!hasHeader(finalHeaders, 'content-type')) {",
      "        finalHeaders['Content-Type'] = 'application/json';",
      "      }",
      "      finalBody = JSON.stringify(body);",
      "    } else {",
      "      finalBody = body as BodyInit;",
      "    }",
      "  }",
      "  return fetchImpl(url, { method, headers: finalHeaders, body: finalBody });",
      "}",
    ];
  }

  function generateRequestFunctionJs() {
    return [
      "export async function request(path, method, options = {}) {",
      "  const { baseUrl, headers = {}, body, fetch: customFetch } = options;",
      "  const fetchImpl = customFetch || globalThis.fetch;",
      "  if (!fetchImpl) {",
      "    throw new Error('Fetch API is not available in this environment.');",
      "  }",
      "  const url = joinUrl(baseUrl, path);",
      "  const finalHeaders = { ...headers };",
      "  let finalBody;",
      "  if (body !== undefined) {",
      "    if (shouldSerializeJson(body)) {",
      "      if (!hasHeader(finalHeaders, 'content-type')) {",
      "        finalHeaders['Content-Type'] = 'application/json';",
      "      }",
      "      finalBody = JSON.stringify(body);",
      "    } else {",
      "      finalBody = body;",
      "    }",
      "  }",
      "  return fetchImpl(url, { method, headers: finalHeaders, body: finalBody });",
      "}",
    ];
  }

  return { generate };
}

function collectOperations(spec, typeFromSchema) {
  const operations = [];
  const paths = spec.paths || {};
  const sortedPaths = Object.keys(paths).sort((a, b) => a.localeCompare(b));

  for (const apiPath of sortedPaths) {
    const pathItem = paths[apiPath] || {};
    const methods = Object.keys(pathItem).sort((a, b) => a.localeCompare(b));
    for (const method of methods) {
      const operation = pathItem[method];
      if (!operation || typeof operation !== 'object') {
        continue;
      }
      const operationId = operation.operationId;
      if (!operationId) {
        continue;
      }
      const upperMethod = method.toUpperCase();
      const requestInfo = extractRequestInfo(operation, typeFromSchema);
      const responseInfo = extractResponseInfo(operation, typeFromSchema);
      const imports = new Set();
      if (requestInfo && requestInfo.imports) {
        mergeImports(imports, requestInfo.imports);
      }
      if (responseInfo.imports) {
        mergeImports(imports, responseInfo.imports);
      }

      const functionName = toCamel(operationId);
      const responseTypeName = `${operationId}Response`;
      const tsFunctionLines = buildOperationTs(operationId, functionName, apiPath, upperMethod, requestInfo, responseInfo, responseTypeName);
      const jsFunctionLines = buildOperationJs(functionName, apiPath, upperMethod, requestInfo, responseInfo);

      const responseTypeDeclaration = `export type ${responseTypeName} = ${responseInfo.type};`;
      const requestTypeDeclaration = requestInfo && requestInfo.type
        ? `export type ${operationId}Request = ${requestInfo.type};`
        : undefined;

      operations.push({
        operationId,
        imports,
        requestTypeDeclaration,
        responseTypeDeclaration,
        tsFunctionLines,
        jsFunctionLines,
      });
    }
  }

  return operations;
}

function extractRequestInfo(operation, typeFromSchema) {
  const requestBody = operation.requestBody;
  if (!requestBody) {
    return undefined;
  }
  const content = requestBody.content || {};
  const mediaTypes = Object.keys(content);
  if (mediaTypes.length === 0) {
    return undefined;
  }
  let schema;
  if (content['application/json']) {
    schema = content['application/json'].schema;
  } else {
    const firstType = mediaTypes[0];
    schema = content[firstType].schema;
  }
  if (!schema) {
    return undefined;
  }
  const result = typeFromSchema(schema, 0);
  return {
    type: result.type,
    imports: result.imports,
    required: Boolean(requestBody.required),
    mediaType: content['application/json'] ? 'json' : mediaTypes[0],
  };
}

function extractResponseInfo(operation, typeFromSchema) {
  const responses = operation.responses || {};
  const statusCodes = Object.keys(responses)
    .map((code) => ({ code, numeric: parseInt(code, 10) }))
    .filter(({ numeric }) => !Number.isNaN(numeric) && numeric >= 200 && numeric < 300)
    .sort((a, b) => a.numeric - b.numeric);
  let successResponse;
  if (statusCodes.length > 0) {
    successResponse = responses[statusCodes[0].code];
  }
  if (!successResponse) {
    return { type: 'Response', parser: 'response', imports: new Set() };
  }

  const content = successResponse.content || {};
  if (Object.keys(content).length === 0) {
    return { type: 'void', parser: 'void', imports: new Set() };
  }

  const jsonMedia = Object.keys(content).find((key) => /json/i.test(key));
  let schema;
  let parser = 'response';
  if (jsonMedia) {
    schema = content[jsonMedia].schema;
    parser = 'json';
  } else {
    const firstType = Object.keys(content)[0];
    schema = content[firstType].schema;
    if (/text\//i.test(firstType)) {
      parser = 'text';
    } else {
      parser = 'response';
    }
  }

  if (!schema) {
    if (parser === 'text') {
      return { type: 'string', parser, imports: new Set() };
    }
    if (parser === 'json') {
      return { type: 'unknown', parser, imports: new Set() };
    }
    return { type: 'Response', parser: 'response', imports: new Set() };
  }

  const result = typeFromSchema(schema, 0);
  return { type: result.type, parser, imports: result.imports };
}

function buildOperationTs(operationId, functionName, apiPath, method, requestInfo, responseInfo, responseTypeName) {
  const lines = [];
  const optionsParam = 'options: RequestOptions = {}';
  if (requestInfo && requestInfo.type) {
    const requestParam = `body${requestInfo.required ? '' : '?'}: ${operationId}Request`;
    lines.push(`export async function ${functionName}(${requestParam}, ${optionsParam}): Promise<${responseTypeName}> {`);
    lines.push(`  const response = await request('${apiPath}', '${method}', { ...options, body });`);
  } else {
    lines.push(`export async function ${functionName}(${optionsParam}): Promise<${responseTypeName}> {`);
    lines.push(`  const response = await request('${apiPath}', '${method}', options);`);
  }
  lines.push('  if (!response.ok) {');
  lines.push('    throw new Error(`Request failed with status ${response.status}`);');
  lines.push('  }');

  if (responseInfo.parser === 'json') {
    lines.push('  const data = await response.json();');
    lines.push(`  return data as ${responseTypeName};`);
  } else if (responseInfo.parser === 'text') {
    lines.push('  const data = await response.text();');
    lines.push(`  return data as ${responseTypeName};`);
  } else if (responseInfo.parser === 'void') {
    lines.push(`  return undefined as ${responseTypeName};`);
  } else {
    lines.push(`  return response as ${responseTypeName};`);
  }
  lines.push('}');
  return lines;
}

function buildOperationJs(functionName, apiPath, method, requestInfo, responseInfo) {
  const lines = [];
  if (requestInfo && requestInfo.type) {
    const bodyParam = requestInfo.required ? 'body' : 'body = undefined';
    lines.push(`export async function ${functionName}(${bodyParam}, options = {}) {`);
    lines.push(`  const response = await request('${apiPath}', '${method}', { ...options, body });`);
  } else {
    lines.push(`export async function ${functionName}(options = {}) {`);
    lines.push(`  const response = await request('${apiPath}', '${method}', options);`);
  }
  lines.push('  if (!response.ok) {');
  lines.push('    throw new Error(`Request failed with status ${response.status}`);');
  lines.push('  }');

  if (responseInfo.parser === 'json') {
    lines.push('  return await response.json();');
  } else if (responseInfo.parser === 'text') {
    lines.push('  return await response.text();');
  } else if (responseInfo.parser === 'void') {
    lines.push('  return;');
  } else {
    lines.push('  return response;');
  }
  lines.push('}');
  return lines;
}

function inferTypeFromSchema(schema) {
  if (schema.properties || schema.additionalProperties) {
    return 'object';
  }
  if (schema.items) {
    return 'array';
  }
  return undefined;
}

function toCamel(name) {
  if (!name) {
    return name;
  }
  return name.charAt(0).toLowerCase() + name.slice(1);
}

function indent(level) {
  return '  '.repeat(level);
}

function mergeImports(target, source) {
  if (!source) {
    return;
  }
  for (const value of source) {
    target.add(value);
  }
}

function formatPropertyKey(key) {
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
    return key;
  }
  return JSON.stringify(key);
}

function formatMultilineType(type, _indentLevel) {
  return type;
}

function wrapArrayType(type) {
  if (/\|/.test(type) || /&/.test(type)) {
    return `(${type})`;
  }
  return type;
}

main();
