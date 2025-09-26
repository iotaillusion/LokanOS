# API & SDK Overview

LokanOS surfaces its control plane through HTTPS JSON APIs fronted by the API
Gateway. Most teams integrate with one of the provided SDKs instead of managing
raw HTTP requests. The SDKs handle URL composition, TLS configuration, and error
parsing so applications can focus on business logic.

## TypeScript SDK quickstart

The TypeScript SDK lives in `sdks/typescript` and is published as a local package
for internal builds. To use it in a Node.js or browser project:

```bash
npm install file:../sdks/typescript
```

Create a thin client instance by passing the API base URL and any shared headers
(such as authentication tokens). The generator exposes one function per API
operation; responses already include strong typing via the exported interfaces.

```ts
import { apiGatewayHealth, telemetryPipeIngest } from '@lokanos/sdk';

const clientOptions = {
  baseUrl: 'https://api.lokan.example',
  headers: {
    Authorization: `Bearer ${process.env.LOKAN_TOKEN}`,
  },
};

async function main() {
  const status = await apiGatewayHealth(clientOptions);
  console.log('Gateway status:', status);

  await telemetryPipeIngest(
    {
      source: 'lab-sensor-42',
      payload: { temperature_c: 23.4 },
    },
    clientOptions,
  );
}

main().catch((err) => {
  console.error('Lokan call failed', err);
  process.exitCode = 1;
});
```

The SDK defaults to `globalThis.fetch`; supply a polyfill (e.g. `node-fetch`) in
older environments via the `fetch` option on each call.

## C SDK quickstart

Embedded clients can depend on the C SDK located in `sdks/c`. The library uses
libcurl under the hood and exposes a minimal surface for health checks and scene
application.

1. Add the SDK as a subdirectory in your CMake project:

   ```cmake
   add_subdirectory(${CMAKE_SOURCE_DIR}/sdks/c ${CMAKE_BINARY_DIR}/lokan-sdk)
   target_link_libraries(your_app PRIVATE lokan-sdk)
   ```

2. Initialize the client with the proper TLS credentials and perform calls:

   ```c
   #include <lokan.h>

   int main(void) {
       lokan_client_t *client = NULL;
       lokan_client_config_t cfg = {
           .base_url = "https://api.lokan.example",
           .client_cert_path = "/etc/lokan/device.crt",
           .client_key_path = "/etc/lokan/device.key",
           .ca_cert_path = "/etc/ssl/certs/ca-certificates.crt",
           .timeout_ms = 5000,
       };

       if (lokan_client_init(&client, &cfg) != LOKAN_OK) {
           fprintf(stderr, "Failed to init Lokan client\n");
           return 1;
       }

       char *health_json = NULL;
       if (lokan_get_health(client, &health_json) == LOKAN_OK) {
           printf("Health: %s\n", health_json);
       }
       lokan_string_free(health_json);

       const char *scene_payload = "{\\"brightness\\":75}";
       lokan_apply_scene(client, "scene-lab-demo", scene_payload);

       lokan_client_cleanup(client);
       return 0;
   }
   ```

Check the `sdks/c/examples/` directory for a complete buildable reference.
