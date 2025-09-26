#ifndef LOKAN_SDK_H
#define LOKAN_SDK_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    LOKAN_OK = 0,
    LOKAN_ERROR_INVALID_ARGUMENT = 1,
    LOKAN_ERROR_ALLOCATION = 2,
    LOKAN_ERROR_CURL = 3,
    LOKAN_ERROR_HTTP = 4,
    LOKAN_ERROR_PARSE = 5
} lokan_result_t;

typedef struct {
    const char *base_url;
    const char *client_cert_path;
    const char *client_key_path;
    const char *ca_cert_path;
    long timeout_ms;
} lokan_client_config_t;

typedef struct lokan_client lokan_client_t;

lokan_result_t lokan_client_init(lokan_client_t **out_client, const lokan_client_config_t *config);
void lokan_client_cleanup(lokan_client_t *client);
const char *lokan_result_string(lokan_result_t result);

lokan_result_t lokan_get_health(lokan_client_t *client, char **out_status);

lokan_result_t lokan_apply_scene(lokan_client_t *client, const char *scene_id, const char *payload_json);

void lokan_string_free(char *value);

#ifdef __cplusplus
}
#endif

#endif /* LOKAN_SDK_H */
