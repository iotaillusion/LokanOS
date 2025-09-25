class PresenceValidationError extends Error {
  constructor(message) {
    super(message);
    this.name = 'PresenceValidationError';
  }
}

class PresenceStore {
  constructor(options = {}) {
    const { clock } = options;
    this.clock = typeof clock === 'function' ? clock : () => new Date();
    this.records = new Map();
  }

  listPresence() {
    return Array.from(this.records.values());
  }

  getPresence(userId) {
    if (!userId || typeof userId !== 'string') {
      return null;
    }
    return this.records.get(userId) || null;
  }

  setPresence(userId, updates = {}) {
    if (!userId || typeof userId !== 'string' || userId.trim() === '') {
      throw new PresenceValidationError('userId is required');
    }

    const normalizedState = typeof updates.state === 'string' ? updates.state.trim().toLowerCase() : '';
    if (!normalizedState) {
      throw new PresenceValidationError('state is required');
    }
    if (normalizedState !== 'home' && normalizedState !== 'away') {
      throw new PresenceValidationError('state must be either "home" or "away"');
    }

    let timestamp;
    if (updates.lastSeenAt) {
      const provided = new Date(updates.lastSeenAt);
      if (Number.isNaN(provided.getTime())) {
        throw new PresenceValidationError('lastSeenAt must be a valid ISO-8601 timestamp');
      }
      timestamp = provided.toISOString();
    } else {
      const now = this.clock();
      const resolvedNow = now instanceof Date ? now : new Date(now);
      timestamp = resolvedNow.toISOString();
    }

    const record = {
      userId,
      state: normalizedState,
      lastSeenAt: timestamp
    };
    this.records.set(userId, record);
    return record;
  }
}

module.exports = {
  PresenceStore,
  PresenceValidationError
};
