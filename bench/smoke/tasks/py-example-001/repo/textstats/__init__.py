"""Petites statistiques sur du texte."""


def words(text):
    return text.split(' ')


def word_count(text):
    return len(words(text))


def mean_word_length(text):
    found = words(text)
    return sum(len(word) for word in found) / len(found) if found else 0.0
