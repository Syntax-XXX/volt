total = 0
for i in range(500000):
    s = f"Eintrag {i} von {i * 2}"
    total += len(s)
print(total)
