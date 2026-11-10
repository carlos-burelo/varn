import time

n = 150
size = n * n
a = [(i % 100) + 1 for i in range(size)]
b = [((i * 3) % 100) + 1 for i in range(size)]
c = [0] * size

start = time.perf_counter()
for row in range(n):
    for col in range(n):
        total = 0
        for k in range(n):
            total += a[row * n + k] * b[k * n + col]
        c[row * n + col] = total
end = time.perf_counter()

print("c[0] =", c[0])
print("c[last] =", c[size - 1])
print("Elapsed time (ms):", (end - start) * 1000.0)
