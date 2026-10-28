from adv.money import price


def test_price():
    assert price(1250) == "€12.50"
