n = 5000000
f = [True] * (n + 1)
i = 2
while i * i <= n:
    if f[i]:
        j = i * i
        while j <= n:
            f[j] = False
            j += i
    i += 1
print(sum(1 for k in range(2, n + 1) if f[k]))
