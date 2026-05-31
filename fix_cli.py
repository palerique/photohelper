with open("crates/photohelper-cli/tests/cli.rs", "r") as f:
    cli_rs = f.read()

cli_rs = cli_rs.replace(
"""    if !output.status.success() {
        panic!("Command failed!\\nstderr:\\n{stderr}");
    }""",
"""    assert!(output.status.success(), "Command failed!\\nstderr:\\n{stderr}");"""
)

with open("crates/photohelper-cli/tests/cli.rs", "w") as f:
    f.write(cli_rs)

with open("crates/photohelper-cli/src/commands/develop.rs", "r") as f:
    develop_rs = f.read()

develop_rs = develop_rs.replace(
"""anyhow::bail!("heartbeat thread panicked: {:?}", e);""",
"""anyhow::bail!("heartbeat thread panicked: {e:?}");"""
)

with open("crates/photohelper-cli/src/commands/develop.rs", "w") as f:
    f.write(develop_rs)

with open("crates/photohelper-cli/src/commands/util.rs", "r") as f:
    util_rs = f.read()

util_rs = util_rs.replace(
"""assert_eq!(format_nima_score_label(3.1415), "03.14");""",
"""assert_eq!(format_nima_score_label(3.1425), "03.14");"""
)

with open("crates/photohelper-cli/src/commands/util.rs", "w") as f:
    f.write(util_rs)
