const topology = {
  rooms: [
    {
      id: 'room-1',
      name: 'Living Room',
      floor: '1',
      deviceIds: ['device-thermostat-1', 'device-light-1']
    },
    {
      id: 'room-2',
      name: 'Bedroom',
      floor: '1',
      deviceIds: ['device-light-2']
    }
  ],
  devices: [
    {
      id: 'device-thermostat-1',
      name: 'Thermostat',
      type: 'thermostat',
      roomId: 'room-1',
      state: { temperature: 72 },
      metadata: { manufacturer: 'Acme' }
    },
    {
      id: 'device-light-1',
      name: 'Ceiling Light',
      type: 'light',
      roomId: 'room-1',
      state: { power: 'on', brightness: 80 },
      metadata: { manufacturer: 'BrightLite' }
    },
    {
      id: 'device-light-2',
      name: 'Bedside Lamp',
      type: 'light',
      roomId: 'room-2',
      state: { power: 'off' },
      metadata: { manufacturer: 'BrightLite' }
    }
  ],
  scenes: [
    {
      id: 'scene-movie-night',
      name: 'Movie Night',
      description: 'Dim lights and set comfortable temperature',
      deviceStates: [
        {
          deviceId: 'device-light-1',
          state: { power: 'on', brightness: 30 }
        },
        {
          deviceId: 'device-thermostat-1',
          state: { temperature: 70 }
        }
      ]
    }
  ]
};

function registerRoutes(app) {
  app.get('/v1/topology', (req, res) => {
    res.json(topology);
  });

  app.post('/v1/devices/:id/commands', (req, res) => {
    const { id } = req.params;
    res.status(202).json({
      commandId: `cmd-${id}`,
      status: 'queued'
    });
  });

  app.post('/v1/scenes/:id/apply', (req, res) => {
    const { id } = req.params;
    res.status(202).json({
      sceneId: id,
      status: 'queued'
    });
  });

  app.post('/v1/rules:test', (req, res) => {
    const { ruleId = 'rule-test' } = req.body || {};
    res.json({
      ruleId,
      status: 'passed',
      logs: ['Rule evaluated successfully with mock data.'],
      actions: [
        {
          type: 'notify',
          payload: {
            channel: 'operations',
            message: 'Rule triggered notification.'
          }
        }
      ],
      errors: []
    });
  });
}

module.exports = {
  registerRoutes,
  topology
};
