# Mixed patterns - multiple capture types for policy testing
# Use case: Real-world code with various operations

import os
import json
from subprocess import run

# Safe operations
config = json.loads('{"debug": true}')
print(f"Debug mode: {config.get('debug')}")

# File operation
with open("log.txt", "a") as f:
    f.write("Starting process\n")

# Subprocess operation
result = run(["git", "rev-parse", "HEAD"], capture_output=True)
commit = result.stdout.decode().strip()

# Another subprocess via os
os.system(f"echo 'Current commit: {commit}'")

# Dynamic execution (maybe from config)
if config.get("eval_enabled"):
    expr = config.get("expression", "1+1")
    eval(expr)
