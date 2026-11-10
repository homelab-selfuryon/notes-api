# notes-api

REST API for notes on Postgres; team `notes`. A copy of `rust-app-template` (read its CLAUDE.md for
the shared parts: config, secrets, auth, workflows). Contract: `meta/delivery.md`, card `meta/notes-api.md`.

## Layout

```
src/store/        Store trait; postgres.rs (sqlx runtime queries, embedded migrations), memory.rs (tests)
src/routes.rs     /v1/notes CRUD, /v1/notes/count (Caller), health
src/auth/         token verifier (from the template)
migrations/       sqlx migrations, embedded with sqlx::migrate!, run at start-up in the background
tests/            notes.rs (HTTP over MemoryStore), auth.rs, kube_jwks.rs, db.rs (#[ignore]; devenv test runs it against devenv's Postgres)
deploy/deploy.cue public, database, allowFrom notes-stats, kubeAPI
devenv.yaml/.nix  tools (platform-kit devenv module), Containerfile image build
```

## Commands

```sh
devenv shell -- cargo test                         # no database needed
devenv shell -- treefmt                            # format (devenv test uses --fail-on-change)
devenv test                                        # the CI contract: treefmt, toolchain/Containerfile match,
                                                   # clippy -D warnings, cargo test, cargo deny check
                                                   # + the database tests against devenv's Postgres
devenv shell -- cue vet ./deploy
docker buildx build -f Containerfile .             # or: podman build -f Containerfile .
devenv shell -- actionlint && devenv shell -- zizmor --offline .github/workflows
```

devenv.lock is not committed until platform-kit v0.1.0 is published (the import needs it); then
`devenv update` and commit; refresh it the same way. Against an unpublished platform-kit:
`devenv -o platform-kit path:../platform-kit test`.

## Rules

- Queries are runtime `sqlx::query` with bound parameters, no string building, no `query!` (no database at
  build time). A new migration is a new numbered file; never edit an applied one.
- The database URI is read from a file (`DB_URI_FILE`), never logged, never an env variable.
- Errors from the store are logged and answered as a plain 500; the client learns nothing about the DB.
- `/v1/notes/count` stays behind `Caller`: notes-stats is the only allowed subject (`allowFrom` in
  deploy.cue becomes `AUTH_ALLOWED_SUBJECTS`).
- Conventional Commits, `!` for breaking changes, signed commits.
