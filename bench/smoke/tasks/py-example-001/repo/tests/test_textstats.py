from textstats import word_count


def test_word_count_single_spaces():
    assert word_count("un deux trois") == 3
