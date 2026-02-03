use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Flag type
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FlagType {
    Boolean,
    WithArg,
    WithOptionalArg,
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

fn default_true() -> bool { true }

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
    /// Get built-in default definitions
    pub fn builtin() -> Self {
        let mut commands = HashMap::new();

        // rm
        commands.insert("rm".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), FlagDef {
                    short: vec!["-r".to_string(), "-R".to_string()],
                    long: Some("--recursive".to_string()),
                    flag_type: FlagType::Boolean,
                }),
                ("force".to_string(), FlagDef {
                    short: vec!["-f".to_string()],
                    long: Some("--force".to_string()),
                    flag_type: FlagType::Boolean,
                }),
                ("no_preserve_root".to_string(), FlagDef {
                    short: vec![],
                    long: Some("--no-preserve-root".to_string()),
                    flag_type: FlagType::Boolean,
                }),
            ]),
            positional: vec![
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

        // chmod
        commands.insert("chmod".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), FlagDef {
                    short: vec!["-R".to_string()],
                    long: Some("--recursive".to_string()),
                    flag_type: FlagType::Boolean,
                }),
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
                ("recursive".to_string(), FlagDef {
                    short: vec!["-r".to_string(), "-R".to_string()],
                    long: Some("--recursive".to_string()),
                    flag_type: FlagType::Boolean,
                }),
                ("force".to_string(), FlagDef {
                    short: vec!["-f".to_string()],
                    long: Some("--force".to_string()),
                    flag_type: FlagType::Boolean,
                }),
                ("no_clobber".to_string(), FlagDef {
                    short: vec!["-n".to_string()],
                    long: Some("--no-clobber".to_string()),
                    flag_type: FlagType::Boolean,
                }),
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

        // mv
        commands.insert("mv".to_string(), CommandDef {
            flags: HashMap::from([
                ("force".to_string(), FlagDef {
                    short: vec!["-f".to_string()],
                    long: Some("--force".to_string()),
                    flag_type: FlagType::Boolean,
                }),
                ("no_clobber".to_string(), FlagDef {
                    short: vec!["-n".to_string()],
                    long: Some("--no-clobber".to_string()),
                    flag_type: FlagType::Boolean,
                }),
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

        // chown
        commands.insert("chown".to_string(), CommandDef {
            flags: HashMap::from([
                ("recursive".to_string(), FlagDef {
                    short: vec!["-R".to_string()],
                    long: Some("--recursive".to_string()),
                    flag_type: FlagType::Boolean,
                }),
            ]),
            positional: vec![
                PositionalDef {
                    name: "owner".to_string(),
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

        // sudo
        commands.insert("sudo".to_string(), CommandDef {
            flags: HashMap::from([
                ("user".to_string(), FlagDef {
                    short: vec!["-u".to_string()],
                    long: Some("--user".to_string()),
                    flag_type: FlagType::WithArg,
                }),
                ("group".to_string(), FlagDef {
                    short: vec!["-g".to_string()],
                    long: Some("--group".to_string()),
                    flag_type: FlagType::WithArg,
                }),
                ("stdin".to_string(), FlagDef {
                    short: vec!["-S".to_string()],
                    long: Some("--stdin".to_string()),
                    flag_type: FlagType::Boolean,
                }),
                ("login".to_string(), FlagDef {
                    short: vec!["-i".to_string()],
                    long: Some("--login".to_string()),
                    flag_type: FlagType::Boolean,
                }),
                ("preserve_env".to_string(), FlagDef {
                    short: vec!["-E".to_string()],
                    long: Some("--preserve-env".to_string()),
                    flag_type: FlagType::Boolean,
                }),
            ]),
            positional: vec![],
            subcommands: HashMap::new(),
            is_wrapper: true,
            parsing: ParsingOptions::default(),
        });

        // git with subcommands
        commands.insert("git".to_string(), CommandDef {
            flags: HashMap::from([
                ("directory".to_string(), FlagDef {
                    short: vec!["-C".to_string()],
                    long: None,
                    flag_type: FlagType::WithArg,
                }),
            ]),
            positional: vec![],
            subcommands: HashMap::from([
                ("status".to_string(), SubcommandDef {
                    flags: HashMap::from([
                        ("short".to_string(), FlagDef {
                            short: vec!["-s".to_string()],
                            long: Some("--short".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                        ("branch".to_string(), FlagDef {
                            short: vec!["-b".to_string()],
                            long: Some("--branch".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                    ]),
                    positional: vec![],
                }),
                ("push".to_string(), SubcommandDef {
                    flags: HashMap::from([
                        ("force".to_string(), FlagDef {
                            short: vec!["-f".to_string()],
                            long: Some("--force".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                        ("force_with_lease".to_string(), FlagDef {
                            short: vec![],
                            long: Some("--force-with-lease".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                        ("delete".to_string(), FlagDef {
                            short: vec!["-d".to_string()],
                            long: Some("--delete".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                        ("set_upstream".to_string(), FlagDef {
                            short: vec!["-u".to_string()],
                            long: Some("--set-upstream".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                    ]),
                    positional: vec![],
                }),
                ("reset".to_string(), SubcommandDef {
                    flags: HashMap::from([
                        ("hard".to_string(), FlagDef {
                            short: vec![],
                            long: Some("--hard".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                        ("soft".to_string(), FlagDef {
                            short: vec![],
                            long: Some("--soft".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                        ("mixed".to_string(), FlagDef {
                            short: vec![],
                            long: Some("--mixed".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                    ]),
                    positional: vec![],
                }),
                ("clean".to_string(), SubcommandDef {
                    flags: HashMap::from([
                        ("force".to_string(), FlagDef {
                            short: vec!["-f".to_string()],
                            long: Some("--force".to_string()),
                            flag_type: FlagType::Boolean,
                        }),
                        ("directories".to_string(), FlagDef {
                            short: vec!["-d".to_string()],
                            long: None,
                            flag_type: FlagType::Boolean,
                        }),
                        ("ignored".to_string(), FlagDef {
                            short: vec!["-x".to_string()],
                            long: None,
                            flag_type: FlagType::Boolean,
                        }),
                    ]),
                    positional: vec![],
                }),
            ]),
            is_wrapper: false,
            parsing: ParsingOptions::default(),
        });

        CommandDefinitions {
            commands,
            defaults: ParsingOptions::default(),
        }
    }

    /// Get command definition by name
    pub fn get(&self, name: &str) -> Option<&CommandDef> {
        self.commands.get(name)
    }
}
