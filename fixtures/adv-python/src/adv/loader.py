"""Loads a format module by name at run time."""

import importlib

FORMATS = {"json": "adv.formats.jsonfmt"}


def load(name: str):
    return importlib.import_module(FORMATS[name])
