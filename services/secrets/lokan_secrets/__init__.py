"""LokanOS secrets and configuration access helpers."""
from __future__ import annotations

import os
from typing import Callable, Dict, Optional

from .backends import (
    ConfigNotFoundError,
    EnvBackend,
    SealedKVBackend,
    SecretBackend,
    SecretNotFoundError,
)

BackendFactory = Callable[[], SecretBackend]

_BACKEND_FACTORIES: Dict[str, BackendFactory] = {
    "env": EnvBackend,
    "sealed-kv": SealedKVBackend,
}

_backend_instance: Optional[SecretBackend] = None


def _get_backend_name() -> str:
    return os.getenv("LOKAN_SECRETS_BACKEND", "env")


def _initialize_backend(name: str | None = None) -> SecretBackend:
    """Create and cache a backend instance."""
    global _backend_instance
    backend_name = name or _get_backend_name()
    try:
        factory = _BACKEND_FACTORIES[backend_name]
    except KeyError as exc:  # pragma: no cover - defensive programming
        raise ValueError(f"Unsupported secrets backend '{backend_name}'") from exc

    _backend_instance = factory()
    return _backend_instance


def _get_backend() -> SecretBackend:
    global _backend_instance
    if _backend_instance is None:
        return _initialize_backend()
    return _backend_instance


def get_secret(key: str) -> str:
    """Return the secret identified by ``key`` from the active backend."""
    return _get_backend().get_secret(key)


def get_config(key: str) -> str:
    """Return the configuration value identified by ``key``."""
    return _get_backend().get_config(key)


def configure_backend(name: str) -> None:
    """Force the secrets module to use the backend identified by ``name``."""
    _initialize_backend(name)


def _reset_backend_for_testing() -> None:
    """Reset cached backend to force reinitialisation.

    This helper is intentionally private; production code should rely on the
    automatic backend selection behaviour.
    """

    global _backend_instance
    _backend_instance = None


__all__ = [
    "ConfigNotFoundError",
    "SecretNotFoundError",
    "get_config",
    "get_secret",
    "configure_backend",
]
