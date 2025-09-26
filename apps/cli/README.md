# LokanOS CLI Demo

This directory contains a lightweight Node.js script demonstrating how to use the generated TypeScript SDK to drive a commissioning flow:

1. Commission a device by sending telemetry to the pipeline.
2. List currently registered devices.
3. Fetch configured scenes and dispatch an apply request for the first entry.

## Running the walkthrough

1. Ensure the SDK artifacts are up to date:

   ```sh
   npm run build --prefix ../../sdks/typescript
   ```

2. Export the LokanOS API base URL if different from the default `http://localhost:8080`:

   ```sh
   export LOKANOS_API_URL="https://api.example.com"
   ```

3. Execute the demo script (optionally pass a device identifier):

   ```sh
   npm run demo --prefix . demo-device-007
   ```

Each step logs its progress so you can correlate requests with backend traces or logs.
