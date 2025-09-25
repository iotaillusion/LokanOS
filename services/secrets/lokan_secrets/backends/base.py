"""Abstract interfaces and exceptions for secret backends."""
from __future__ import annotations

from abc import ABC, abstractmethod


class SecretError(KeyError):
    """Base exception for secret backend errors."""


class SecretNotFoundError(SecretError):
    """Raised when a requested secret is not available."""


class ConfigNotFoundError(SecretError):
    """Raised when a requested configuration value is not available."""


class SecretBackend(ABC):
    """Interface for secret backends."""

    @abstractmethod
    def get_secret(self, key: str) -> str:
        """Return the secret associated with ``key``.

        Implementations should raise :class:`SecretNotFoundError` when the key
        is unknown or does not have an associated value.
        """

    @abstractmethod
    def get_config(self, key: str) -> str:
        """Return the configuration value associated with ``key``.

        Implementations should raise :class:`ConfigNotFoundError` when the key
        is unknown or does not have an associated value.
        """
