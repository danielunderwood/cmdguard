# Dynamic execution - matches @dynamic_exec patterns
# Use case: Code that executes strings as code

# eval - evaluate expression
result = eval("2 + 2")
x = eval(user_input)  # dangerous with untrusted input

# exec - execute statements
exec("x = 1")
exec("""
def hello():
    print('hello')
hello()
""")

# compile - compile source to code object
code = compile("print('hi')", "<string>", "exec")
exec(code)

# __import__ - dynamic import
mod = __import__("json")
