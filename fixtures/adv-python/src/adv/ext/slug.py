"""In a namespace package (adv/ext has no __init__.py)."""


def slugify(title: str) -> str:
    return title.lower().replace(" ", "-")
