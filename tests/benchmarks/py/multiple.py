import time


def fib(n):
    return n if n <= 1 else fib(n - 1) + fib(n - 2)


for r in range(1, 6):
    start = time.perf_counter()
    result = fib(35)
    elapsed = (time.perf_counter() - start) * 1000.0
    print("--- Run ---")
    print(r)
    print("Result:")
    print(result)
    print("Time (ms):")
    print(elapsed)
