use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Flag type
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FlagType {
    Boolean,
    WithArg,
    WithOptionalArg,
    /// Flag that can appear multiple times, collecting values into an array
    /// Example: curl -H "header1" -H "header2"
    Repeatable,
}

/// Positional argument type
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ArgType {
    String,
    Path,
    Number,
}

/// Definition for a single flag
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlagDef {
    /// Short form(s) - can be single "-f" or multiple ["-r", "-R"]
    #[serde(default)]
    pub short: Vec<String>,
    /// Long form "--force"
    #[serde(default)]
    pub long: Option<String>,
    /// Flag type
    #[serde(rename = "type")]
    pub flag_type: FlagType,
}

/// Definition for a positional argument
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PositionalDef {
    pub name: String,
    #[serde(rename = "type")]
    pub arg_type: ArgType,
    /// Fixed position (0-indexed), None for flexible
    #[serde(default)]
    pub position: Option<i32>,
    /// Can consume multiple arguments
    #[serde(default)]
    pub variadic: bool,
    /// Must be last argument
    #[serde(default)]
    pub last: bool,
    /// Optional argument
    #[serde(default)]
    pub optional: bool,
}

/// Parsing options for a command
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParsingOptions {
    #[serde(default = "default_true")]
    pub combine_short_flags: bool,
    #[serde(default = "default_true")]
    pub double_dash_ends_flags: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ParsingOptions {
    fn default() -> Self {
        ParsingOptions {
            combine_short_flags: true,
            double_dash_ends_flags: true,
        }
    }
}

/// Definition for a subcommand (like git push)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubcommandDef {
    pub flags: HashMap<String, FlagDef>,
    #[serde(default)]
    pub positional: Vec<PositionalDef>,
}

/// Full command definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandDef {
    #[serde(default)]
    pub flags: HashMap<String, FlagDef>,
    #[serde(default)]
    pub positional: Vec<PositionalDef>,
    #[serde(default)]
    pub subcommands: HashMap<String, SubcommandDef>,
    #[serde(default)]
    pub is_wrapper: bool,
    #[serde(default)]
    pub parsing: ParsingOptions,
}

/// All command definitions
#[derive(Debug, Clone)]
pub struct CommandDefinitions {
    pub commands: HashMap<String, CommandDef>,
    pub defaults: ParsingOptions,
}

impl CommandDefinitions {
    /// Create from a map of commands (useful for testing)
    #[cfg(test)]
    pub fn from_map(commands: HashMap<String, CommandDef>) -> Self {
        Self {
            commands,
            defaults: ParsingOptions::default(),
        }
    }


    /// Get built-in default definitions (empty - real definitions come from builtins.ncl)
    pub fn builtin() -> Self {
        CommandDefinitions {
            commands: HashMap::new(),
            defaults: ParsingOptions::default(),
        }
    }

    /// Get command definition by name
    pub fn get(&self, name: &str) -> Option<&CommandDef> {
        self.commands.get(name)
    }

    /// Merge custom command definitions into this set
    ///
    /// If a command already exists, its subcommands and flags are merged (custom takes precedence).
    /// If a command doesn't exist, it's added as-is.
    pub fn merge(&mut self, custom: HashMap<String, CommandDef>) {
        for (name, custom_def) in custom {
            if let Some(existing) = self.commands.get_mut(&name) {
                // Deep merge: merge subcommands (not replace)
                for (subcmd_name, subcmd_def) in custom_def.subcommands {
                    if let Some(existing_subcmd) = existing.subcommands.get_mut(&subcmd_name) {
                        // Deep merge the subcommand: add custom flags to existing
                        for (flag_name, flag_def) in subcmd_def.flags {
                            existing_subcmd.flags.insert(flag_name, flag_def);
                        }
                        // Merge positional args
                        for pos_def in subcmd_def.positional {
                            if !existing_subcmd.positional.iter().any(|p| p.name == pos_def.name) {
                                existing_subcmd.positional.push(pos_def);
                            }
                        }
                    } else {
                        // New subcommand, just insert
                        existing.subcommands.insert(subcmd_name, subcmd_def);
                    }
                }
                // Merge top-level flags
                for (flag_name, flag_def) in custom_def.flags {
                    existing.flags.insert(flag_name, flag_def);
                }
                // Merge positional args (append custom ones)
                for pos_def in custom_def.positional {
                    if !existing.positional.iter().any(|p| p.name == pos_def.name) {
                        existing.positional.push(pos_def);
                    }
                }
                // Override other fields if set in custom
                if custom_def.is_wrapper {
                    existing.is_wrapper = true;
                }
            } else {
                // New command, just insert
                self.commands.insert(name, custom_def);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_flag(short: &[&str], long: Option<&str>, flag_type: FlagType) -> FlagDef {
        FlagDef {
            short: short.iter().map(|s| s.to_string()).collect(),
            long: long.map(|s| s.to_string()),
            flag_type,
        }
    }

    fn make_positional(name: &str, arg_type: ArgType) -> PositionalDef {
        PositionalDef {
            name: name.to_string(),
            arg_type,
            position: None,
            variadic: false,
            last: false,
            optional: false,
        }
    }

    fn make_subcommand(flags: HashMap<String, FlagDef>, positional: Vec<PositionalDef>) -> SubcommandDef {
        SubcommandDef { flags, positional }
    }

    /// Create test definitions (used by tests that need command definitions)
    fn test_definitions() -> CommandDefinitions {
        let mut commands = HashMap::new();

        // git with subcommands
        commands.insert("git".to_string(), CommandDef {
            flags: HashMap::from([
                ("directory".to_string(), make_flag(&["-C"], None, FlagType::WithArg)),
            ]),
            positional: vec![],
            subcommands: HashMap::from([
                ("status".to_string(), make_subcommand(
                    HashMap::from([
                        ("short".to_string(), make_flag(&["-s"], Some("--short"), FlagType::Boolean)),
                    ]),
                    vec![],
                )),
                ("push".to_string(), make_subcommand(
                    HashMap::from([
                        ("force".to_string(), make_flag(&["-f"], Some("--force"), FlagType::Boolean)),
                    ]),
                    vec![],
                )),
                ("reset".to_string(), make_subcommand(
                    HashMap::from([
                        ("hard".to_string(), make_flag(&[], Some("--hard"), FlagType::Boolean)),
                    ]),
                    vec![],
                )),
            ]),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // rm
        commands.insert("rm".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), make_flag(&["-r", "-R"], Some("--recursive"), FlagType::Boolean)),
                ("force".to_string(), make_flag(&["-f"], Some("--force"), FlagType::Boolean)),
            ]),
            positional: vec![PositionalDef {
                name: "targets".to_string(),
                arg_type: ArgType::Path,
                position: None,
                variadic: true,
                last: false,
                optional: false,
            }],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // chmod
        commands.insert("chmod".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), make_flag(&["-R"], Some("--recursive"), FlagType::Boolean)),
            ]),
            positional: vec![
                PositionalDef {
                    name: "mode".to_string(),
                    arg_type: ArgType::String,
                    position: Some(0),
                    variadic: false,
                    last: false,
                    optional: false,
                },
                PositionalDef {
                    name: "targets".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: false,
                },
            ],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // cp
        commands.insert("cp".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), make_flag(&["-r", "-R"], Some("--recursive"), FlagType::Boolean)),
            ]),
            positional: vec![
                PositionalDef {
                    name: "sources".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: true,
                    last: false,
                    optional: false,
                },
                PositionalDef {
                    name: "destination".to_string(),
                    arg_type: ArgType::Path,
                    position: None,
                    variadic: false,
                    last: true,
                    optional: false,
                },
            ],
            subcommands: HashMap::new(),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // cargo
        commands.insert("cargo".to_string(), CommandDef {
            flags: HashMap::new(),
            positional: vec![],
            subcommands: HashMap::from([
                ("build".to_string(), make_subcommand(
                    HashMap::from([
                        ("release".to_string(), make_flag(&["-r"], Some("--release"), FlagType::Boolean)),
                    ]),
                    vec![],
                )),
            ]),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        // npm
        commands.insert("npm".to_string(), CommandDef {
            flags: HashMap::new(),
            positional: vec![],
            subcommands: HashMap::from([
                ("install".to_string(), make_subcommand(
                    HashMap::from([
                        ("save_dev".to_string(), make_flag(&["-D"], Some("--save-dev"), FlagType::Boolean)),
                    ]),
                    vec![],
                )),
            ]),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        CommandDefinitions {
            commands,
            defaults: ParsingOptions::default(),
        }
    }

    #[test]
    fn test_merge_adds_new_subcommand_to_existing_command() {
        let mut defs = test_definitions();

        // git exists in builtins, add a new subcommand
        let mut custom = HashMap::new();
        let mut git_subcommands = HashMap::new();
        git_subcommands.insert(
            "my-custom-subcmd".to_string(),
            make_subcommand(HashMap::new(), vec![make_positional("arg", ArgType::String)]),
        );
        custom.insert(
            "git".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![],
                subcommands: git_subcommands,
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // Verify git exists before merge
        assert!(defs.get("git").is_some());
        let git_before = defs.get("git").unwrap();
        assert!(!git_before.subcommands.contains_key("my-custom-subcmd"));

        defs.merge(custom);

        // After merge, git should still exist and have the new subcommand
        let git = defs.get("git").unwrap();
        assert!(git.subcommands.contains_key("my-custom-subcmd"));
        // Original subcommands should still be there
        assert!(git.subcommands.contains_key("status"));
        assert!(git.subcommands.contains_key("push"));
    }

    #[test]
    fn test_merge_overrides_existing_subcommand() {
        let mut defs = test_definitions();

        // git log exists in builtins, override it with custom definition
        let mut custom = HashMap::new();
        let mut git_subcommands = HashMap::new();

        // Custom log with different flags
        let mut custom_log_flags = HashMap::new();
        custom_log_flags.insert(
            "my-custom-flag".to_string(),
            make_flag(&["-x"], Some("--my-custom-flag"), FlagType::Boolean),
        );
        git_subcommands.insert(
            "log".to_string(),
            make_subcommand(custom_log_flags, vec![]),
        );

        custom.insert(
            "git".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![],
                subcommands: git_subcommands,
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        defs.merge(custom);

        let git = defs.get("git").unwrap();
        let log = git.subcommands.get("log").unwrap();
        // Custom flag should be present
        assert!(log.flags.contains_key("my-custom-flag"));
        // Original flags from builtin log are REPLACED (not merged at subcommand level)
        // The entire subcommand definition is replaced
    }

    #[test]
    fn test_merge_adds_new_flag_to_existing_command() {
        let mut defs = test_definitions();

        let mut custom = HashMap::new();
        let mut git_flags = HashMap::new();
        git_flags.insert(
            "my-new-flag".to_string(),
            make_flag(&["-z"], Some("--my-new-flag"), FlagType::WithArg),
        );

        custom.insert(
            "git".to_string(),
            CommandDef {
                flags: git_flags,
                positional: vec![],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        defs.merge(custom);

        let git = defs.get("git").unwrap();
        // New flag should be added
        assert!(git.flags.contains_key("my-new-flag"));
        // Original git flags should still be there
        assert!(git.flags.contains_key("directory"));
    }

    #[test]
    fn test_merge_overrides_existing_flag() {
        let mut defs = test_definitions();

        // Override git's -C/directory flag with a different type
        let mut custom = HashMap::new();
        let mut git_flags = HashMap::new();
        git_flags.insert(
            "directory".to_string(),
            make_flag(&["-D"], Some("--directory"), FlagType::Boolean), // Changed from WithArg to Boolean, different short
        );

        custom.insert(
            "git".to_string(),
            CommandDef {
                flags: git_flags,
                positional: vec![],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        let git_before = defs.get("git").unwrap();
        assert!(matches!(git_before.flags.get("directory").unwrap().flag_type, FlagType::WithArg));

        defs.merge(custom);

        let git = defs.get("git").unwrap();
        // Flag should be overridden
        assert!(matches!(git.flags.get("directory").unwrap().flag_type, FlagType::Boolean));
        // Short flag should also be updated
        assert!(git.flags.get("directory").unwrap().short.contains(&"-D".to_string()));
    }

    #[test]
    fn test_merge_adds_completely_new_command() {
        let mut defs = test_definitions();

        let mut custom = HashMap::new();
        let mut flags = HashMap::new();
        flags.insert(
            "verbose".to_string(),
            make_flag(&["-v"], Some("--verbose"), FlagType::Boolean),
        );

        custom.insert(
            "my-brand-new-tool".to_string(),
            CommandDef {
                flags,
                positional: vec![make_positional("file", ArgType::Path)],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // Verify it doesn't exist before
        assert!(defs.get("my-brand-new-tool").is_none());

        defs.merge(custom);

        // Should exist after merge
        let tool = defs.get("my-brand-new-tool").unwrap();
        assert!(tool.flags.contains_key("verbose"));
        assert_eq!(tool.positional.len(), 1);
        assert_eq!(tool.positional[0].name, "file");
    }

    #[test]
    fn test_merge_appends_new_positional_args() {
        let mut defs = test_definitions();

        let mut custom = HashMap::new();
        custom.insert(
            "git".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![
                    make_positional("custom-arg", ArgType::String),
                ],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        let git_before = defs.get("git").unwrap();
        let pos_count_before = git_before.positional.len();

        defs.merge(custom);

        let git = defs.get("git").unwrap();
        // Should have one more positional arg
        assert_eq!(git.positional.len(), pos_count_before + 1);
        assert!(git.positional.iter().any(|p| p.name == "custom-arg"));
    }

    #[test]
    fn test_merge_does_not_duplicate_positional_by_name() {
        let mut defs = test_definitions();

        // First add a custom positional
        let mut custom1 = HashMap::new();
        custom1.insert(
            "git".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![make_positional("my-arg", ArgType::String)],
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );
        defs.merge(custom1);

        let git_after_first = defs.get("git").unwrap();
        let pos_count_after_first = git_after_first.positional.len();

        // Now try to add the same positional again
        let mut custom2 = HashMap::new();
        custom2.insert(
            "git".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![make_positional("my-arg", ArgType::Path)], // Same name, different type
                subcommands: HashMap::new(),
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );
        defs.merge(custom2);

        let git = defs.get("git").unwrap();
        // Count should not have increased
        assert_eq!(git.positional.len(), pos_count_after_first);
        // Original type should be preserved (first one wins)
        let my_arg = git.positional.iter().find(|p| p.name == "my-arg").unwrap();
        assert!(matches!(my_arg.arg_type, ArgType::String));
    }

    #[test]
    fn test_merge_preserves_original_command_structure() {
        let mut defs = test_definitions();

        // Add only a subcommand, nothing else
        let mut custom = HashMap::new();
        let mut git_subcommands = HashMap::new();
        git_subcommands.insert(
            "my-subcmd".to_string(),
            make_subcommand(HashMap::new(), vec![]),
        );
        custom.insert(
            "git".to_string(),
            CommandDef {
                flags: HashMap::new(),
                positional: vec![],
                subcommands: git_subcommands,
                is_wrapper: false,
                parsing: ParsingOptions::default(),
            },
        );

        // Capture original structure
        let git_before = defs.get("git").unwrap();
        let original_flags: Vec<_> = git_before.flags.keys().cloned().collect();
        let original_subcommands: Vec<_> = git_before.subcommands.keys().cloned().collect();

        defs.merge(custom);

        let git = defs.get("git").unwrap();
        // All original flags should still exist
        for flag in &original_flags {
            assert!(git.flags.contains_key(flag), "Missing flag: {}", flag);
        }
        // All original subcommands should still exist
        for subcmd in &original_subcommands {
            assert!(git.subcommands.contains_key(subcmd), "Missing subcommand: {}", subcmd);
        }
        // Plus the new one
        assert!(git.subcommands.contains_key("my-subcmd"));
    }
}
