; Python dangerous patterns query
; Detects operations that make code unsafe for inspection mode
;
; Capture names determine the DangerKind:
;   @dangerous_import -> DangerousImport
;   @dynamic_exec     -> DynamicExecution
;   @file_op          -> FileOperation
;   @subprocess_op    -> SubprocessOperation
;   @network_op       -> NetworkOperation

; =============================================================================
; Dangerous imports
; =============================================================================

; import os, import subprocess, etc.
(import_statement
  name: (dotted_name) @dangerous_import
  (#match? @dangerous_import "^(os|subprocess|socket|shutil|tempfile|ctypes|multiprocessing)$"))

; import os as x
(import_statement
  name: (aliased_import
    name: (dotted_name) @dangerous_import
    (#match? @dangerous_import "^(os|subprocess|socket|shutil|tempfile|ctypes|multiprocessing)$")))

; from os import path, from subprocess import run, etc.
(import_from_statement
  module_name: (dotted_name) @dangerous_import
  (#match? @dangerous_import "^(os|subprocess|socket|shutil|tempfile|ctypes|multiprocessing)$"))

; =============================================================================
; Dynamic execution
; =============================================================================

; eval(), exec(), compile(), __import__(), execfile()
(call
  function: (identifier) @dynamic_exec
  (#match? @dynamic_exec "^(eval|exec|compile|__import__|execfile)$"))

; =============================================================================
; File operations
; =============================================================================

; open(), file()
(call
  function: (identifier) @file_op
  (#match? @file_op "^(open|file)$"))

; pathlib.Path(...).write_text(), .write_bytes(), .open()
; (call
;   function: (attribute
;     attribute: (identifier) @method)
;   (#match? @method "^(write_text|write_bytes|open|unlink|rmdir|mkdir)$"))

; =============================================================================
; Subprocess operations via os module
; =============================================================================

; os.system(), os.popen(), os.exec*(), os.spawn*()
(call
  function: (attribute
    object: (identifier) @_obj
    attribute: (identifier) @subprocess_op)
  (#eq? @_obj "os")
  (#match? @subprocess_op "^(system|popen|exec|execl|execle|execlp|execlpe|execv|execve|execvp|execvpe|spawnl|spawnle|spawnlp|spawnlpe|spawnv|spawnve|spawnvp|spawnvpe)$"))

; =============================================================================
; Subprocess module calls
; =============================================================================

; subprocess.run(), subprocess.call(), subprocess.Popen(), etc.
(call
  function: (attribute
    object: (identifier) @_obj
    attribute: (identifier) @subprocess_op)
  (#eq? @_obj "subprocess"))

; =============================================================================
; Network operations (for future use)
; =============================================================================

; socket.socket(), socket.create_connection(), etc.
; (call
;   function: (attribute
;     object: (identifier) @_obj
;     attribute: (identifier) @network_op)
;   (#eq? @_obj "socket"))
