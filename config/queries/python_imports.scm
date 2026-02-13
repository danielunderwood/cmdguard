; Python imports extraction query
; Extracts all imported module names for dependency validation

; Simple import: import foo
(import_statement
  name: (dotted_name) @import)

; Aliased import: import foo as bar
; Captures the original module name, not the alias
(import_statement
  name: (aliased_import
    name: (dotted_name) @import))

; From import: from foo import bar
; Captures the module being imported from
(import_from_statement
  module_name: (dotted_name) @import)
