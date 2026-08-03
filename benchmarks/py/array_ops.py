from _harness import run


def compute():
    arr = []
    size = 500000
    i = 0
    while i < size:
        arr.append(i)
        i = i + 1
    s = 0
    j = 0
    while j < size:
        s = s + arr[j]
        j = j + 1
    k = 0
    while k < size:
        arr[k] = k * 2
        k = k + 1
    return s


run(compute)
