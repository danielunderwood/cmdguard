# File operations - matches @file_op patterns
# Use case: Code that reads/writes files

# Built-in open
with open("data.txt", "r") as f:
    content = f.read()

with open("output.txt", "w") as f:
    f.write("hello")

# Reading with mode
f = open("config.json")
data = f.read()
f.close()

# Pathlib operations (if query extended to cover these)
from pathlib import Path

p = Path("myfile.txt")
# These would need additional query patterns:
# p.write_text("content")
# p.read_text()
# p.unlink()
