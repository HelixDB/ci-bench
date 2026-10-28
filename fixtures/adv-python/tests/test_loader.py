from adv.loader import load


def test_loads_a_format_by_name():
    assert load("json").EXTENSION == ".json"
