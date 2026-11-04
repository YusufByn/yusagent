from textstats import mean_word_length, word_count


def test_word_count_repeated_spaces():
    assert word_count("un  deux") == 2


def test_word_count_newlines():
    assert word_count("un\ndeux\ntrois") == 3


def test_mean_word_length():
    assert mean_word_length("ab cdef") == 3.0
