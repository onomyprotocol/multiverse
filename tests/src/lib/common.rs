use onomy_test_lib::dockerfiles::onomy_std_cosmos_daemon_with_arbitrary;

#[rustfmt::skip]
const DOWNLOAD_ONOMYD: &str = r#"ADD https://github.com/onomyprotocol/onomy/releases/download/$DAEMON_VERSION/onomyd $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/onomyd"#;

pub fn dockerfile_onomyd() -> String {
    onomy_std_cosmos_daemon_with_arbitrary("onomyd", ".onomy", "v1.1.1", DOWNLOAD_ONOMYD)
}
