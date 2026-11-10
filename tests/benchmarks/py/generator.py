import time


def pingpong():
    x = 0
    while True:
        x += 1
        yield x


def range_gen(start, end):
    for i in range(start, end):
        yield i


def fib_gen():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b


def nested_inner(n):
    yield n * 10
    yield n * 20


def nested_outer():
    yield 1
    inner = nested_inner(5)
    yield next(inner) + 100
    yield next(inner) + 100


def make_counter(step):
    base = step * 1000

    def counter():
        i = 0
        while True:
            yield base + i
            i += 1

    return counter


def guarded():
    yield 7
    yield 8


t0 = time.perf_counter()
pp = pingpong()
last = next(pp)
for _ in range(100000):
    last = next(pp)
print(f"pingpong checksum={last} ms={(time.perf_counter() - t0) * 1000}")

t0 = time.perf_counter()
map_sum = filt_sum = 0
for v in range_gen(0, 100000):
    map_sum += v * 2
    if v % 2 == 0:
        filt_sum += v
print(f"mapfilter map_sum={map_sum} filt_sum={filt_sum} ms={(time.perf_counter() - t0) * 1000}")

t0 = time.perf_counter()
fg = fib_gen()
fib_carry = 0
for _ in range(80):
    fib_carry = next(fg)
print(f"fibcarry checksum={fib_carry} ms={(time.perf_counter() - t0) * 1000}")

t0 = time.perf_counter()
nest_sum = sum(nested_outer())
print(f"nested checksum={nest_sum} ms={(time.perf_counter() - t0) * 1000}")

t0 = time.perf_counter()
counter = make_counter(3)
c1, c2 = counter(), counter()
cap_sum = next(c1) + next(c1) + next(c2)
print(f"upvalue checksum={cap_sum} ms={(time.perf_counter() - t0) * 1000}")

t0 = time.perf_counter()
try:
    gg = guarded()
    try_sum = next(gg) + next(gg)
except StopIteration:
    try_sum = -1
print(f"tryoutside checksum={try_sum} ms={(time.perf_counter() - t0) * 1000}")
