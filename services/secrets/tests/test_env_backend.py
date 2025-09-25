
import pytest

import lokan_secrets
from lokan_secrets.backends import ConfigNotFoundError, EnvBackend, SecretNotFoundError


def setup_function() -> None:
    # Ensure the module re-evaluates backend selection for each test.
    lokan_secrets._reset_backend_for_testing()


def test_env_backend_returns_secret(monkeypatch):
    monkeypatch.setenv("LOKAN_SECRETS_BACKEND", "env")
    monkeypatch.setenv("SERVICE_API_TOKEN", "token-123")

    assert lokan_secrets.get_secret("SERVICE_API_TOKEN") == "token-123"


def test_env_backend_returns_config(monkeypatch):
    monkeypatch.setenv("LOKAN_SECRETS_BACKEND", "env")
    monkeypatch.setenv("SERVICE_URL", "https://example.test")

    assert lokan_secrets.get_config("SERVICE_URL") == "https://example.test"


def test_env_backend_missing_secret(monkeypatch):
    monkeypatch.setenv("LOKAN_SECRETS_BACKEND", "env")
    monkeypatch.delenv("MISSING_SECRET", raising=False)

    with pytest.raises(SecretNotFoundError):
        lokan_secrets.get_secret("MISSING_SECRET")


def test_env_backend_missing_config(monkeypatch):
    monkeypatch.setenv("LOKAN_SECRETS_BACKEND", "env")
    monkeypatch.delenv("MISSING_CONFIG", raising=False)

    with pytest.raises(ConfigNotFoundError):
        lokan_secrets.get_config("MISSING_CONFIG")


def test_env_backend_rejects_empty_values(monkeypatch):
    monkeypatch.setenv("LOKAN_SECRETS_BACKEND", "env")
    monkeypatch.setenv("EMPTY_SECRET", "")

    with pytest.raises(SecretNotFoundError):
        lokan_secrets.get_secret("EMPTY_SECRET")


def test_env_backend_direct_usage():
    backend = EnvBackend({"FOO": "bar", "BAZ": "qux"})

    assert backend.get_secret("FOO") == "bar"
    assert backend.get_config("BAZ") == "qux"

    with pytest.raises(SecretNotFoundError):
        backend.get_secret("MISSING")

    with pytest.raises(ConfigNotFoundError):
        backend.get_config("MISSING")
