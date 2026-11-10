{ ... }:
{
  # Toolchain, tools, treefmt and `devenv test` come from the platform-kit module (see devenv.yaml).

  # A local Postgres over its unix socket. The shell exports DB_URI_FILE (a file with the connection
  # string, like /var/run/secrets/db/uri in the cluster) and DATABASE_URL (for the tests).
  platformKit.postgres.enable = true;

  # `devenv test` starts Postgres, runs the module's checks, then the database tests (#[ignore]d, so
  # that a plain `cargo test` needs no database).
  enterTest = ''
    cargo test --locked --test db -- --ignored
  '';
}
