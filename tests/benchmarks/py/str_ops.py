import time


def report(name, checksum, ms):
    print(f"{name} | checksum={checksum} | ms={ms}")


def concat_append():
    s = ""
    for _ in range(20000):
        s += "abc"
    return len(s)


def concat_parts():
    total = 0
    for i in range(200000):
        piece = "a" + str(i) + "b"
        total += len(piece)
    return total


def join():
    parts = ["item" + str(i) for i in range(100000)]
    return len(",".join(parts))


def search():
    haystack = "abcdefghij" * 1000 + "needle"
    total = 0
    for _ in range(2000):
        idx = haystack.find("needle")
        total += -1 if idx < 0 else idx
        if "efgh" in haystack:
            total += 1
    return total


def split():
    csv = ",".join(str(i) for i in range(10000))
    total = 0
    for _ in range(100):
        total += len(csv.split(","))
    return total


def slice_():
    base = "abcdefghijklmnopqrstuvwxyz" * 400
    total = 0
    for i in range(200000):
        start = i % 10000
        total += len(base[start:start + 10])
    return total


def case():
    base = "AbCdEfGhIj" * 1000
    total = 0
    for _ in range(500):
        total += len(base.upper())
        total += len(base.lower())
    return total


def replace_all():
    base = "the quick brown fox jumps over the lazy dog " * 200
    total = 0
    for _ in range(200):
        total += len(base.replace("o", "00"))
    return total


def prefix_suffix():
    items = ["item" + str(i) + "x" for i in range(100000)]
    count = 0
    for item in items:
        if item.startswith("item1"):
            count += 1
        if item.endswith("9x"):
            count += 1
    return count


def char_code():
    base = "abcdefghijklmnopqrstuvwxyz0123456789" * 2000
    total = 0
    for _ in range(20):
        for ch in base:
            total += ord(ch)
    return total


def equality():
    items = ["key" + str(i % 100) for i in range(1000)]
    count = 0
    for _ in range(200):
        for item in items:
            if item == "key50":
                count += 1
    return count


def int_to_str():
    total = 0
    for i in range(200000):
        total += len(str(i))
    return total


for name, fn in [
    ("concat_append", concat_append), ("concat_parts", concat_parts), ("join", join),
    ("search", search), ("split", split), ("slice", slice_), ("case", case),
    ("replace_all", replace_all), ("prefix_suffix", prefix_suffix),
    ("char_code", char_code), ("equality", equality), ("int_to_str", int_to_str),
]:
    start = time.perf_counter()
    checksum = fn()
    report(name, checksum, (time.perf_counter() - start) * 1000.0)
