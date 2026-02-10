package claude.permissions

import rego.v1

is_python if input.binary_name == "python"

python_module := module if {
	is_python
	module := input.parsed_flags.module
}

is_python_module(name) if python_module == name

is_pytest if input.binary_name == "pytest"

is_pytest if {
	is_python_module("pytest")
}

is_tests_main if {
	is_python
	input.positional.file[0].raw == "tests/main.py"
}

is_json_tool if is_python_module("json.tool")

rules["pytest"] := {
	"decision": "allow",
	"reason": "Pytest allowed",
	"priority": 25,
} if {
	is_pytest
}

rules["tests_main"] := {
	"decision": "allow",
	"reason": "tests/main.py allowed",
	"priority": 25,
} if {
	is_tests_main
}

rules["json_tool"] := {
	"decision": "allow",
	"reason": "json.tool allowed",
	"priority": 25,
} if {
	is_json_tool
}

rules["safe_python_tools"] := {
	"decision": "allow",
	"reason": "Safe Python tool allowed",
	"priority": 25,
} if {
	input.command[0] in {"alembic", "mypy", "pylint", "black", "isort"}
}
