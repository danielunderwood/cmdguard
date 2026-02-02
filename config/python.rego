package claude.permissions

import rego.v1

is_python if input.command[0] == "python"

python_module := module if {
    is_python
    module := flag_value("-m")
}

is_python_module(name) if python_module == name

is_pytest if input.command[0] == "pytest"

is_pytest if {
    is_python_module("pytest")
}

is_tests_main if {
    is_python
    input.command[1] == "tests/main.py"
}

is_json_tool if is_python_module("json.tool")

rules["pytest"] := {
    "decision": "allow",
    "reason": "Pytest allowed",
    "priority": 25,
} if is_pytest

rules["tests_main"] := {
    "decision": "allow",
    "reason": "tests/main.py allowed",
    "priority": 25,
} if is_tests_main

rules["json_tool"] := {
    "decision": "allow",
    "reason": "json.tool allowed",
    "priority": 25,
} if is_json_tool

rules["safe_python_tools"] := {
    "decision": "allow",
    "reason": "Safe Python tool allowed",
    "priority": 25,
} if {
    input.command[0] in {"mypy", "pylint", "black", "isort"}
}
