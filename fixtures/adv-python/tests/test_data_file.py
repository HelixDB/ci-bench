from pathlib import Path


def test_reads_a_data_file():
    rows = (Path(__file__).parent / "data" / "rates.csv").read_text().splitlines()
    assert rows[1:] == ["EUR,1.0", "USD,1.1"]
