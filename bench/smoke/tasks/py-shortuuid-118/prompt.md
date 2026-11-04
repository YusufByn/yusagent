`ShortUUID.encoded_length(num_bytes)` annonce parfois un caractère de plus que nécessaire.

Avec un alphabet de 64 caractères, `encoded_length(9)` renvoie 13, alors que 9 octets tiennent exactement en 12 caractères. Avec un alphabet de 8 caractères, `encoded_length(9)` renvoie 25 au lieu de 24. Le même écart apparaît pour d'autres tailles avec ces alphabets.

Attendu : le nombre minimal exact de caractères qui peut représenter `num_bytes` octets avec l'alphabet choisi, pour toutes les tailles de 0 à 16 octets avec des alphabets de 8 et de 64 caractères. Les résultats déjà exacts pour les autres alphabets ne changent pas.
