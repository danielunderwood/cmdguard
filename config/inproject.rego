package claude.permissions

import rego.v1

rules["project_associated_binary"] := {
	"decision": "allow",
	"reason": "Binary in project",
	"priority": 25,
} if {
	regex.match(`target/(debug|release)/[^/]+$`, input.resolved_path)
	input.resolved_trust_zone == "project"
}
