"""Importing any plugin runs this first: it registers the separators."""

from adv import registry

registry.register("csv", ",")
