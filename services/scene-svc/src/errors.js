class SceneValidationError extends Error {
  constructor(message) {
    super(message);
    this.name = 'SceneValidationError';
  }
}

class SceneNotFoundError extends Error {
  constructor(message) {
    super(message);
    this.name = 'SceneNotFoundError';
  }
}

module.exports = {
  SceneValidationError,
  SceneNotFoundError
};
