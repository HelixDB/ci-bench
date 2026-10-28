"""Separators, registered by the plugins package's __init__.py."""

_SEPARATORS: dict[str, str] = {}


def register(name: str, separator: str) -> None:
    _SEPARATORS[name] = separator


def lookup(name: str) -> str:
    return _SEPARATORS[name]
