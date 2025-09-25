'use strict';

function deepClone(value) {
  if (value === undefined) {
    return undefined;
  }
  return JSON.parse(JSON.stringify(value));
}

function getValueByPath(source, path) {
  if (!source || typeof source !== 'object') {
    return undefined;
  }
  if (typeof path !== 'string' || path.length === 0) {
    return undefined;
  }
  const segments = path.split('.');
  let current = source;
  for (const segment of segments) {
    if (current === null || typeof current !== 'object') {
      return undefined;
    }
    if (Object.prototype.hasOwnProperty.call(current, segment)) {
      current = current[segment];
    } else {
      return undefined;
    }
  }
  return current;
}

module.exports = {
  deepClone,
  getValueByPath
};
