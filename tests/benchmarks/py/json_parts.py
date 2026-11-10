import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path = [p for p in sys.path if os.path.abspath(p) != _here]
import json

items = []
for i in range(50000):
    items.append({"id": i, "name": f"Item_{i}", "active": (i % 2) == 0, "score": (i % 100) * 1.25})
text = json.dumps(items, separators=(chr(44), chr(58)))
print(f"Stringify done, len: {len(text)}")
parsed = json.loads(text)
print(f"Parse done, len: {len(parsed)}")
