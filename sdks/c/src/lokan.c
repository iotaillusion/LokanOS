#include "lokan.h"

#include <curl/curl.h>
#include <ctype.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

struct lokan_client {
    CURL *handle;
    char *base_url;
    char *client_cert_path;
    char *client_key_path;
    char *ca_cert_path;
    long timeout_ms;
};

static lokan_result_t lokan_init_global(void) {
    static int initialized = 0;
    if (!initialized) {
        CURLcode code = curl_global_init(CURL_GLOBAL_DEFAULT);
        if (code != CURLE_OK) {
            return LOKAN_ERROR_CURL;
        }
        initialized = 1;
    }
    return LOKAN_OK;
}

static char *lokan_strdup(const char *value) {
    if (!value) {
        return NULL;
    }
    size_t len = strlen(value);
    char *copy = (char *)malloc(len + 1);
    if (!copy) {
        return NULL;
    }
    memcpy(copy, value, len);
    copy[len] = '\0';
    return copy;
}

lokan_result_t lokan_client_init(lokan_client_t **out_client, const lokan_client_config_t *config) {
    if (!out_client || !config || !config->base_url) {
        return LOKAN_ERROR_INVALID_ARGUMENT;
    }

    lokan_result_t init_result = lokan_init_global();
    if (init_result != LOKAN_OK) {
        return init_result;
    }

    lokan_client_t *client = (lokan_client_t *)calloc(1, sizeof(lokan_client_t));
    if (!client) {
        return LOKAN_ERROR_ALLOCATION;
    }

    client->handle = curl_easy_init();
    if (!client->handle) {
        free(client);
        return LOKAN_ERROR_CURL;
    }

    client->base_url = lokan_strdup(config->base_url);
    client->client_cert_path = lokan_strdup(config->client_cert_path);
    client->client_key_path = lokan_strdup(config->client_key_path);
    client->ca_cert_path = lokan_strdup(config->ca_cert_path);
    client->timeout_ms = config->timeout_ms > 0 ? config->timeout_ms : 5000;

    if (!client->base_url) {
        lokan_client_cleanup(client);
        return LOKAN_ERROR_ALLOCATION;
    }

    *out_client = client;
    return LOKAN_OK;
}

void lokan_client_cleanup(lokan_client_t *client) {
    if (!client) {
        return;
    }
    if (client->handle) {
        curl_easy_cleanup(client->handle);
    }
    free(client->base_url);
    free(client->client_cert_path);
    free(client->client_key_path);
    free(client->ca_cert_path);
    free(client);
}

const char *lokan_result_string(lokan_result_t result) {
    switch (result) {
        case LOKAN_OK:
            return "ok";
        case LOKAN_ERROR_INVALID_ARGUMENT:
            return "invalid argument";
        case LOKAN_ERROR_ALLOCATION:
            return "allocation failed";
        case LOKAN_ERROR_CURL:
            return "curl error";
        case LOKAN_ERROR_HTTP:
            return "http error";
        case LOKAN_ERROR_PARSE:
            return "parse error";
        default:
            return "unknown error";
    }
}

struct lokan_memory {
    char *data;
    size_t size;
};

static size_t lokan_write_callback(void *contents, size_t size, size_t nmemb, void *userp) {
    size_t realsize = size * nmemb;
    struct lokan_memory *mem = (struct lokan_memory *)userp;
    char *ptr = (char *)realloc(mem->data, mem->size + realsize + 1);
    if (!ptr) {
        return 0;
    }
    mem->data = ptr;
    memcpy(&(mem->data[mem->size]), contents, realsize);
    mem->size += realsize;
    mem->data[mem->size] = '\0';
    return realsize;
}

static char *lokan_join_url(const char *base, const char *path) {
    size_t base_len = strlen(base);
    size_t path_len = strlen(path);
    int need_slash = 0;
    if (base_len > 0 && path_len > 0) {
        int base_has = base[base_len - 1] == '/';
        int path_has = path[0] == '/';
        if (base_has && path_has) {
            need_slash = -1;
        } else if (!base_has && !path_has) {
            need_slash = 1;
        }
    }

    size_t total = base_len + path_len + 1;
    if (need_slash == 1) {
        total += 1;
    } else if (need_slash == -1) {
        total -= 1;
    }

    char *result = (char *)malloc(total);
    if (!result) {
        return NULL;
    }

    if (need_slash == -1 && base_len > 0) {
        memcpy(result, base, base_len - 1);
        result[base_len - 1] = '\0';
    } else if (base_len > 0) {
        memcpy(result, base, base_len);
        result[base_len] = '\0';
    } else {
        result[0] = '\0';
    }

    if (need_slash == 1) {
        strcat(result, "/");
    }
    strcat(result, path);
    return result;
}

static void lokan_apply_tls_options(lokan_client_t *client) {
    if (!client || !client->handle) {
        return;
    }
    curl_easy_setopt(client->handle, CURLOPT_USE_SSL, CURLUSESSL_ALL);
    curl_easy_setopt(client->handle, CURLOPT_SSLVERSION, CURL_SSLVERSION_TLSv1_2);
    curl_easy_setopt(client->handle, CURLOPT_SSL_VERIFYPEER, 1L);
    curl_easy_setopt(client->handle, CURLOPT_SSL_VERIFYHOST, 2L);
    if (client->client_cert_path) {
        curl_easy_setopt(client->handle, CURLOPT_SSLCERT, client->client_cert_path);
    }
    if (client->client_key_path) {
        curl_easy_setopt(client->handle, CURLOPT_SSLKEY, client->client_key_path);
    }
    if (client->ca_cert_path) {
        curl_easy_setopt(client->handle, CURLOPT_CAINFO, client->ca_cert_path);
    }
}

static lokan_result_t lokan_perform_request(
    lokan_client_t *client,
    const char *path,
    const char *method,
    const char *body,
    size_t body_len,
    char **out_body,
    long *out_status) {
    if (!client || !client->handle || !path || !method) {
        return LOKAN_ERROR_INVALID_ARGUMENT;
    }

    curl_easy_reset(client->handle);
    lokan_apply_tls_options(client);
    curl_easy_setopt(client->handle, CURLOPT_TIMEOUT_MS, client->timeout_ms);
    curl_easy_setopt(client->handle, CURLOPT_USERAGENT, "lokan-c-sdk/0.1");

    char *url = lokan_join_url(client->base_url, path);
    if (!url) {
        return LOKAN_ERROR_ALLOCATION;
    }
    curl_easy_setopt(client->handle, CURLOPT_URL, url);

    struct lokan_memory memory = {0};
    curl_easy_setopt(client->handle, CURLOPT_WRITEFUNCTION, lokan_write_callback);
    curl_easy_setopt(client->handle, CURLOPT_WRITEDATA, (void *)&memory);

    struct curl_slist *headers = NULL;

    if (strcmp(method, "GET") == 0) {
        curl_easy_setopt(client->handle, CURLOPT_HTTPGET, 1L);
    } else {
        curl_easy_setopt(client->handle, CURLOPT_CUSTOMREQUEST, method);
    }

    if (body && body_len > 0) {
        curl_easy_setopt(client->handle, CURLOPT_POSTFIELDS, body);
        curl_easy_setopt(client->handle, CURLOPT_POSTFIELDSIZE, (long)body_len);
        headers = curl_slist_append(headers, "Content-Type: application/json");
    }

    headers = curl_slist_append(headers, "Accept: application/json");
    curl_easy_setopt(client->handle, CURLOPT_HTTPHEADER, headers);

    CURLcode res = curl_easy_perform(client->handle);
    curl_slist_free_all(headers);
    free(url);

    if (res != CURLE_OK) {
        free(memory.data);
        return LOKAN_ERROR_CURL;
    }

    long status_code = 0;
    curl_easy_getinfo(client->handle, CURLINFO_RESPONSE_CODE, &status_code);
    if (out_status) {
        *out_status = status_code;
    }

    if (status_code >= 400) {
        free(memory.data);
        return LOKAN_ERROR_HTTP;
    }

    if (out_body) {
        if (!memory.data) {
            memory.data = lokan_strdup("");
            if (!memory.data) {
                return LOKAN_ERROR_ALLOCATION;
            }
        }
        *out_body = memory.data;
    } else {
        free(memory.data);
    }

    return LOKAN_OK;
}

static char *lokan_trim_quotes(char *value) {
    if (!value) {
        return NULL;
    }
    size_t len = strlen(value);
    while (len > 0 && isspace((unsigned char)value[len - 1])) {
        value[--len] = '\0';
    }
    while (*value && isspace((unsigned char)*value)) {
        value++;
    }
    if (*value == '"') {
        value++;
        char *end = strchr(value, '"');
        if (end) {
            *end = '\0';
        }
    }
    return value;
}

lokan_result_t lokan_get_health(lokan_client_t *client, char **out_status) {
    if (!client || !out_status) {
        return LOKAN_ERROR_INVALID_ARGUMENT;
    }

    char *response = NULL;
    lokan_result_t result = lokan_perform_request(client, "/health", "GET", NULL, 0, &response, NULL);
    if (result != LOKAN_OK) {
        return result;
    }

    const char *needle = "\"status\"";
    char *found = strstr(response, needle);
    if (!found) {
        free(response);
        return LOKAN_ERROR_PARSE;
    }
    found += strlen(needle);
    found = strchr(found, ':');
    if (!found) {
        free(response);
        return LOKAN_ERROR_PARSE;
    }
    found++;
    char *status_value = lokan_trim_quotes(found);
    if (!status_value || *status_value == '\0') {
        free(response);
        return LOKAN_ERROR_PARSE;
    }

    char *status_copy = lokan_strdup(status_value);
    free(response);
    if (!status_copy) {
        return LOKAN_ERROR_ALLOCATION;
    }

    *out_status = status_copy;
    return LOKAN_OK;
}

lokan_result_t lokan_apply_scene(lokan_client_t *client, const char *scene_id, const char *payload_json) {
    if (!client || !scene_id) {
        return LOKAN_ERROR_INVALID_ARGUMENT;
    }

    const char *template_body = "{\"sceneId\":\"%s\"}";
    char *body = NULL;
    size_t body_len = 0;

    if (payload_json) {
        body_len = strlen(payload_json);
        body = lokan_strdup(payload_json);
        if (!body) {
            return LOKAN_ERROR_ALLOCATION;
        }
    } else {
        body_len = strlen(template_body) + strlen(scene_id) - 2; /* %s replaced */
        body = (char *)malloc(body_len + 1);
        if (!body) {
            return LOKAN_ERROR_ALLOCATION;
        }
        int written = snprintf(body, body_len + 1, template_body, scene_id);
        if (written < 0 || (size_t)written != body_len) {
            free(body);
            return LOKAN_ERROR_ALLOCATION;
        }
    }

    lokan_result_t result = lokan_perform_request(client, "/scenes/apply", "POST", body, body_len, NULL, NULL);
    free(body);
    return result;
}

void lokan_string_free(char *value) {
    free(value);
}
