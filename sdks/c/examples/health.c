#include "lokan.h"

#include <stdio.h>
#include <stdlib.h>

static const char *env_or_default(const char *name, const char *fallback) {
    const char *value = getenv(name);
    if (value && value[0] != '\0') {
        return value;
    }
    return fallback;
}

int main(void) {
    const char *base_url = env_or_default("LOKAN_SDK_BASE_URL", "https://localhost:9443/scene-svc");
    const char *client_cert = getenv("LOKAN_SDK_CLIENT_CERT");
    const char *client_key = getenv("LOKAN_SDK_CLIENT_KEY");
    const char *ca_cert = getenv("LOKAN_SDK_CA_CERT");

    if (!client_cert || !client_key || !ca_cert) {
        fprintf(stderr, "Missing TLS configuration. Set LOKAN_SDK_CLIENT_CERT, LOKAN_SDK_CLIENT_KEY, and LOKAN_SDK_CA_CERT.\n");
        return 1;
    }

    lokan_client_config_t config = {
        .base_url = base_url,
        .client_cert_path = client_cert,
        .client_key_path = client_key,
        .ca_cert_path = ca_cert,
        .timeout_ms = 5000,
    };

    lokan_client_t *client = NULL;
    lokan_result_t result = lokan_client_init(&client, &config);
    if (result != LOKAN_OK) {
        fprintf(stderr, "Failed to initialize client: %s\n", lokan_result_string(result));
        return 1;
    }

    char *status = NULL;
    result = lokan_get_health(client, &status);
    if (result != LOKAN_OK) {
        fprintf(stderr, "Health check failed: %s\n", lokan_result_string(result));
        lokan_client_cleanup(client);
        return 1;
    }

    printf("Scene service health: %s\n", status);
    lokan_string_free(status);
    lokan_client_cleanup(client);
    return 0;
}
