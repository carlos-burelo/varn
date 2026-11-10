total = 0
for i in range(10000000):
    x = {"a": i, "b": i + 1, "c": i + 2}
    total += x["a"]
print(total)
