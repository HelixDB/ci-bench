"""A pytest plugin the root conftest loads by name."""

import pytest


@pytest.fixture
def greeting():
    return "hello"
