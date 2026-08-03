from math import floor, sqrt
from _harness import run


def bench_math(it):
    r = 1.0
    i = 0
    while i < it:
        r = abs(r - i)
        r = sqrt(r + 1.0)
        r = floor(r * 10.0) / 10.0
        i = i + 1
    return r


run(lambda: bench_math(500000))
