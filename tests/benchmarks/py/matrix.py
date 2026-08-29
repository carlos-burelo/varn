from _harness import run


def compute():
    n = 150
    size = n * n
    a = []
    b = []
    c = []
    for i in range(size):
        a.append((i % 100) + 1)
        b.append(((i * 3) % 100) + 1)
        c.append(0)
    for row in range(n):
        for col in range(n):
            s = 0
            for k in range(n):
                s = s + a[row * n + k] * b[k * n + col]
            c[row * n + col] = s
    return c[0] + c[size - 1]


run(compute)
