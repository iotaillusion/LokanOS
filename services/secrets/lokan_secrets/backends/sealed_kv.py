"""Placeholder implementation for a future sealed key-value backend."""
from __future__ import annotations

from .base import ConfigNotFoundError, SecretBackend, SecretNotFoundError


class SealedKVBackend(SecretBackend):
    """Placeholder backend that will eventually talk to the sealed KV store."""

    def get_secret(self, key: str) -> str:  # pragma: no cover - placeholder
        raise NotImplementedError(
            "The sealed-kv backend is not available yet."
        )

    def get_config(self, key: str) -> str:  # pragma: no cover - placeholder
        raise NotImplementedError(
            "The sealed-kv backend is not available yet."
        )
