from pathlib import Path


def test_attente_ecrite_par_la_commande():
    assert Path("attente.txt").read_text().strip() == "fini"
