# notes-api

Example app of team `notes` in the [homelab-selfuryon](https://github.com/homelab-selfuryon) org: a REST
API for notes on Postgres. Made from
[`rust-app-template`](https://github.com/homelab-selfuryon/rust-app-template); delivered by the
platform as described in `meta/delivery.md` (signed image and manifests, gitops PR, Argo CD).

| Method and path                | Auth      |                                                                                                                         |
| ------------------------------ | --------- | ----------------------------------------------------------------------------------------------------------------------- |
| `GET /healthz`                 | none      | liveness                                                                                                                |
| `GET /readyz`                  | none      | readiness: migrations done, database answers, not shutting down                                                         |
| `POST /v1/notes`               | none      | `{"title": "...", "body": "..."}` → 201 and the note. Title 1..200 characters, body up to 10000, unknown fields refused |
| `GET /v1/notes?limit=&offset=` | none      | newest first; limit 1..200 (default 50)                                                                                 |
| `GET /v1/notes/{id}`           | none      | 404 if there is no such note                                                                                            |
| `PUT /v1/notes/{id}`           | none      | replaces title and body                                                                                                 |
| `DELETE /v1/notes/{id}`        | none      | 204                                                                                                                     |
| `GET /v1/notes/count`          | **token** | `{"count": n}`; only for `system:serviceaccount:notes-<env>:notes-stats`                                                |

The notes routes are public on the environment's Gateway and unauthenticated: this is an example, and the
interesting part is `/v1/notes/count`. A caller sends its projected ServiceAccount token
(`Authorization: Bearer ...`, audience `notes-api`). The app verifies the signature against the
cluster's signing keys (fetched from the kube-apiserver with its own token, cached, re-fetched on rotation,
rate limited), the issuer, the audience, the expiry, and that the subject is on the allowlist. 401 for a
missing or invalid token, 403 for a valid token of anybody else.

## Configuration

Set by the platform from `deploy/deploy.cue`: `PORT`, `APP_ENV`, `APP_VERSION`, `AUTH_AUDIENCE`
(`notes-api`), `AUTH_ALLOWED_SUBJECTS` (from `network.allowFrom`), and the files
`/var/run/secrets/db/uri` (CloudNativePG `notes-api-db-app` Secret) and
`/var/run/secrets/kube-api/{token,ca.crt}` (`network.kubeAPI`). Own settings: `LOG_LEVEL`,
`DB_URI_FILE`, `DB_MAX_CONNECTIONS` (5), `SHUTDOWN_DELAY_SECS` (2), `REQUEST_TIMEOUT_SECS` (30). The
database connection string is never an environment variable and is never logged.

The pool connects lazily and the migrations (`migrations/`, embedded in the binary) run in the background
at start-up, retried until the database answers: the server is up at once, `/readyz` says no until
the schema is in place. Postgres is reached without certificate verification (`sslmode=prefer` from the
CloudNativePG `uri`); to verify the server certificate, mount the cluster CA and add
`sslmode=verify-full&sslrootcert=...` (not done here).

## Development

Tools come from devenv (no Nix flakes; `.envrc` has `use devenv`); `devenv test` is what CI runs.

```sh
devenv shell           # toolchain, cue (0.18 pre-release), cargo-deny, treefmt, git hooks, local Postgres
treefmt                # format everything (same config as the git hook)
cargo test             # unit and HTTP tests over an in-memory store; no database, no cluster
devenv test            # treefmt --fail-on-change, toolchain/Containerfile match, clippy -D warnings,
                       # cargo test, cargo deny check (network: advisories), then the database tests
docker buildx build -f Containerfile .   # the image (podman build works too)
devenv shell -- cue vet ./deploy
```

The database tests (`tests/db.rs`) are `#[ignore]`d so that a plain `cargo test` needs no database.
`devenv test` starts devenv's Postgres (`platformKit.postgres.enable` in `devenv.nix`; unix socket,
trust authentication, database `app`) and runs them. To run them by hand: `devenv up -d`, then
`devenv shell -- cargo test --test db -- --ignored`, then `devenv processes down`. Inside the shell
`DB_URI_FILE` points at a file with the connection string (like `/var/run/secrets/db/uri` in the
cluster) and `DATABASE_URL` holds the same string for the tests.

Run the app against it:

```sh
devenv up -d
devenv shell -- cargo run          # DB_URI_FILE is set by the shell
curl -s localhost:8080/v1/notes -d '{"title":"hello"}' -H 'content-type: application/json'
```

`devenv.lock` is not committed yet: the import needs platform-kit v0.1.0 to be published. Then
`devenv update` and commit it; refresh it the same way (Renovate has no devenv manager).
Against an unpublished platform-kit: `devenv -o platform-kit path:../platform-kit test`.

Delivery (preview per pull request, release-please, tag → staging, promote → prod, register) is the
template's: see its README. Commits follow Conventional Commits and are signed.
