import adv.plugins.csv
from adv import registry


def test_the_package_init_registers_the_separator():
    assert adv.plugins.csv.split("a,b", registry.lookup("csv")) == ["a", "b"]
