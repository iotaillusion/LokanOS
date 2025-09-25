"""Environment variable backed secrets provider."""
from __future__ import annotations

import os
from typing import Mapping, MutableMapping

from .base import ConfigNotFoundError, SecretBackend, SecretNotFoundError


class EnvBackend(SecretBackend):
    """Secrets backend that reads values from environment variables."""

    def __init__(self, env: Mapping[str, str] | MutableMapping[str, str] | None = None):
        # ``os.environ`` behaves like a mutable mapping, but typing considers it
        # only mutable. Accept both immutable and mutable mappings so tests can
        # provide a tailored mapping.
        self._env: Mapping[str, str] | MutableMapping[str, str] = env if env is not None else os.environ

    def _get_value(self, key: str, error_type: type[SecretNotFoundError] | type[ConfigNotFoundError]) -> str:
        try:
            value = self._env[key]
        except KeyError as exc:  # pragma: no cover - delegated exception path
            raise error_type(f"{key} is not set in the environment") from exc

        if value == "":
            raise error_type(f"{key} is set but empty in the environment")
        return value

    def get_secret(self, key: str) -> str:
        return self._get_value(key, SecretNotFoundError)

    def get_config(self, key: str) -> str:
        return self._get_value(key, ConfigNotFoundError)
