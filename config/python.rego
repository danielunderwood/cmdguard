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

rules["pytest"] := allow("Pytest allowed") if {
	is_pytest
}

rules["tests_main"] := allow("tests/main.py allowed") if {
	is_tests_main
}

rules["json_tool"] := allow("json.tool allowed") if {
	is_json_tool
}

rules["safe_python_tools"] := allow("Safe Python tool allowed") if {
	input.command[0] in {"alembic", "mypy", "pylint", "black", "isort", "ruff"}
}

# ============================================================================
# Python inline code analysis (python -c)
# ============================================================================

# Helper to check if any pattern matches a capture name
has_pattern(capture_name) if {
	input.python_analysis.patterns[_].capture == capture_name
}

# Allow safe inspection code (no dangerous patterns)
rules["python_safe_inspection"] := allow_at("Python code is safe for inspection", 30) if {
	is_python
	input.python_analysis.is_inspection_safe
}

# Deny dynamic execution (eval, exec, compile)
rules["python_deny_dynamic_exec"] := deny_at("Python code contains dynamic execution (eval/exec)", 40) if {
	is_python
	has_pattern("dynamic_exec")
}

# Deny subprocess operations
rules["python_deny_subprocess"] := deny_at("Python code contains subprocess operations", 40) if {
	is_python
	has_pattern("subprocess_op")
}
