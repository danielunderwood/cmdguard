# Python inline code analysis configuration
#
# Defines patterns for detecting dangerous operations in Python code.
# Used by the python_analyzer to determine if code is safe for
# inspection mode (auto-allow) vs execution mode (needs sandbox).

{
  # Imports that indicate potentially dangerous code
  # These trigger execution mode instead of inspection mode
  dangerous_imports = [
    "os",
    "subprocess",
    "socket",
    "shutil",
    "tempfile",
    "ctypes",
    "multiprocessing",
  ],

  # Function calls that are dangerous regardless of imports
  dangerous_calls = [
    "eval",
    "exec",
    "compile",
    "__import__",
    "execfile",
    "input",  # Can be dangerous in some contexts
  ],

  # File operation functions
  file_operations = [
    "open",
    "file",
  ],

  # Network-related modules (for future use)
  network_modules = [
    "socket",
    "urllib",
    "http",
    "ftplib",
    "smtplib",
    "telnetlib",
  ],

  # Safe builtins that are always allowed in inspection mode
  safe_builtins = [
    "print",
    "type",
    "dir",
    "help",
    "repr",
    "str",
    "len",
    "isinstance",
    "issubclass",
    "hasattr",
    "getattr",
    "id",
    "hash",
    "vars",
    "sorted",
    "reversed",
    "enumerate",
    "zip",
    "map",
    "filter",
    "list",
    "dict",
    "set",
    "tuple",
    "frozenset",
    "bool",
    "int",
    "float",
    "complex",
    "bytes",
    "bytearray",
    "memoryview",
    "range",
    "slice",
    "object",
    "super",
    "property",
    "classmethod",
    "staticmethod",
  ],

  # Safe attribute access patterns (for inspection)
  safe_attributes = [
    "__doc__",
    "__name__",
    "__module__",
    "__qualname__",
    "__annotations__",
    "__class__",
    "__bases__",
    "__mro__",
  ],

  # Safe modules from stdlib that are always allowed
  safe_stdlib = [
    "json",
    "re",
    "math",
    "datetime",
    "collections",
    "itertools",
    "functools",
    "operator",
    "string",
    "textwrap",
    "unicodedata",
    "typing",
    "types",
    "inspect",
    "dis",
    "ast",
    "dataclasses",
    "enum",
    "abc",
    "copy",
    "pprint",
    "reprlib",
    "numbers",
    "decimal",
    "fractions",
    "random",
    "statistics",
    "pathlib",
  ],

  # Custom tree-sitter queries for advanced pattern matching
  # Each query should capture dangerous nodes with @dangerous
  custom_queries = [
    # Example: detect requests.post/put/delete (network writes)
    # {
    #   name = "requests_write",
    #   kind = "network",
    #   query = m%"
    #     (call
    #       function: (attribute
    #         object: (identifier) @obj
    #         attribute: (identifier) @method)
    #       (#eq? @obj "requests")
    #       (#match? @method "^(post|put|delete|patch)$"))
    #   "%
    # },
  ],
}
