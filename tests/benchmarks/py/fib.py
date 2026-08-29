import sys
sys.setrecursionlimit(10000)
from _harness import run


def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)


run(lambda: fib(35))
