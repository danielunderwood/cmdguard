# Subprocess operations - matches @subprocess_op and @dangerous_import patterns
# Use case: Code that spawns external processes

import os
import subprocess

# os module subprocess calls
os.system("ls -la")
os.popen("echo hello")

# subprocess module
subprocess.run(["git", "status"])
subprocess.call(["make", "build"])
subprocess.Popen(["python", "-m", "http.server"])

# os.exec variants
# os.execl("/bin/ls", "ls", "-l")
# os.execv("/bin/ls", ["ls", "-l"])
