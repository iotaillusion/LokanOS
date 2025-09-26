import {
  telemetryPipeIngest,
  deviceRegistryListDevices,
  sceneServiceListScenes,
} from '../../sdks/typescript/client.js';

const baseUrl = process.env.LOKANOS_API_URL ?? 'http://localhost:8080';
const clientOptions = { baseUrl };

async function commissionDevice(deviceId) {
  console.log(`Commissioning device ${deviceId}...`);
  await telemetryPipeIngest(
    {
      source: 'cli-demo',
      payload: {
        type: 'commission',
        deviceId,
        issuedAt: new Date().toISOString(),
      },
    },
    clientOptions,
  );
  console.log('Commission request submitted to the telemetry pipeline.');
}

async function listDevices() {
  console.log('Retrieving registered devices...');
  const response = await deviceRegistryListDevices(clientOptions);
  const devices = Array.isArray(response?.devices) ? response.devices : [];
  if (devices.length === 0) {
    console.log('No registered devices were returned.');
  } else {
    devices.forEach((device, index) => {
      console.log(`${index + 1}. ${JSON.stringify(device)}`);
    });
  }
  return devices;
}

async function applyScene(sceneId) {
  console.log(`Applying scene ${sceneId}...`);
  await telemetryPipeIngest(
    {
      source: 'cli-demo',
      payload: {
        type: 'scene.apply',
        sceneId,
        requestedAt: new Date().toISOString(),
      },
    },
    clientOptions,
  );
  console.log('Scene application event dispatched via telemetry pipeline.');
}

async function main() {
  const deviceId = process.argv[2] ?? 'demo-device-001';

  console.log('--- LokanOS CLI commissioning walkthrough ---');
  await commissionDevice(deviceId);

  const devices = await listDevices();

  console.log('Fetching available scenes...');
  const scenesResponse = await sceneServiceListScenes(clientOptions);
  const scenes = Array.isArray(scenesResponse?.scenes) ? scenesResponse.scenes : [];
  if (scenes.length === 0) {
    console.log('No scenes configured; skipping apply step.');
    return;
  }

  const chosenScene = scenes[0];
  const sceneIdentifier = chosenScene.id ?? chosenScene.slug ?? 'scene-1';
  console.log(`Applying scene ${sceneIdentifier}.`);
  await applyScene(sceneIdentifier);

  console.log('Workflow complete. Use service metrics or logs to verify the requests.');
}

main().catch((error) => {
  console.error('CLI demo failed:', error);
  process.exit(1);
});
