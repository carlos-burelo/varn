import json

items = []
for i in range(50000):
    items.append({"id": i, "name": f"Item_{i}", "active": (i % 2) == 0, "score": (i % 100) * 1.25})
text = json.dumps(items)
print(f"Stringify done, len: {len(text)}")
parsed = json.loads(text)
print(f"Parse done, len: {len(parsed)}")
