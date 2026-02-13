# Safe inspection code - should match no patterns
# Use case: Claude inspecting library APIs, checking types, reading docs

import json
import pandas as pd
from collections import defaultdict
from typing import List, Dict

# Type inspection
print(type(pd.DataFrame))
print(pd.DataFrame.__doc__)
print(pd.DataFrame.__name__)

# Safe stdlib operations
data = json.dumps({"key": "value"})
parsed = json.loads(data)

# Collection operations
items = defaultdict(list)
items["a"].append(1)

# Type annotations
def process(items: List[Dict[str, int]]) -> int:
    return sum(d.get("count", 0) for d in items)
