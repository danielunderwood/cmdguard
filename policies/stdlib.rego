package claude.permissions.stdlib

import future.keywords.in
import future.keywords.every

flag_value(flag) := input.command[i + 1] {
	input.command[i] == flag
	i + 1 < count(input.command)
	not startswith(input.command[i + 1], "-")
}

git_subcommand := input.command[1] {
	input.command[0] == "git"
	count(input.command) > 1
	not startswith(input.command[1], "-")
}

path_outside_project {
	some path in input.paths
	not startswith(path.resolved, input.project_root)
}

all_paths_in_project {
	count(input.paths) > 0
	every path in input.paths {
		startswith(path.resolved, input.project_root)
	}
}

no_paths {
	count(input.paths) == 0
}
