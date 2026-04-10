package cmdguard

import rego.v1

rules["project_associated_binary"] := allow("Binary in project") if {
	regex.match(`target/(debug|release)/[^/]+$`, input.resolved_path)
	input.resolved_trust_zone == "project"
}
