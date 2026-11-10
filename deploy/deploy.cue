package deploy

import "github.com/homelab-selfuryon/platform-kit/app"

app.#App & {
	name:   "notes-api"
	team:   "notes"
	port:   8080
	size:   "small"
	public: true
	// CloudNativePG: the connection string is the file /var/run/secrets/db/uri.
	database: postgres: {}
	env: LOG_LEVEL:     "info"
	network: {
		// notes-stats reads GET /v1/notes/count with a token whose audience is notes-api. The platform
		// sets AUTH_AUDIENCE=notes-api and AUTH_ALLOWED_SUBJECTS to its ServiceAccount.
		allowFrom: [{app: "notes-stats"}]
		// The caller's token is verified against the cluster's keys, read from the kube-apiserver.
		kubeAPI: true
	}
}
