use std::time::Duration;

use common::dockerfile_onomyd;
use log::info;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_bank_send, cosmovisor_get_addr, cosmovisor_get_balances, cosmovisor_start,
        fast_block_times, force_chain_id, set_minimum_gas_price, sh_cosmovisor,
        sh_cosmovisor_no_dbg, sh_cosmovisor_tx, wait_for_num_blocks,
    },
    dockerfiles::{dockerfile_hermes, onomy_std_cosmos_daemon},
    hermes::{hermes_start, sh_hermes, write_hermes_config, HermesChainConfig, IbcPair},
    onomy_std_init, reprefix_bech32,
    setups::{cosmovisor_add_consumer, onomyd_setup, test_proposal},
    super_orchestrator::{
        docker::{Container, ContainerNetwork, Dockerfile},
        net_message::NetMessenger,
        remove_files_in_dir, sh,
        stacked_errors::{MapAddError, Result},
        FileOptions, STD_DELAY, STD_TRIES,
    },
    token18, Args, ONOMY_IBC_NOM, TIMEOUT,
};
use serde_json::{json, Value};
use tokio::time::sleep;

const CONSUMER_ID: &str = "haven";
const PROVIDER_ACCOUNT_PREFIX: &str = "onomy";
const CONSUMER_ACCOUNT_PREFIX: &str = "onomy";

const IBC_KUDOS: &str = "ibc/1A9CC9E90B4706CE7C3460CB138F0839B8A0B129C377644D5563428773B879D3";
const KUDOS_TEST_ADDR: &str = "onomy1y046r7wtrcss63kauwpee5rkmm322fn8twluug";

pub async fn havend_setup(
    daemon_home: &str,
    chain_id: &str,
    ccvconsumer_state_s: &str,
) -> Result<()> {
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id]).await?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    // add `ccvconsumer_state` to genesis
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    let genesis_s = genesis_s.replace("\"stake\"", "\"akudos\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;

    force_chain_id(daemon_home, &mut genesis, chain_id).await?;

    let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s)?;
    genesis["app_state"]["ccvconsumer"] = ccvconsumer_state;
    // TODO remove this in next ICS version
    genesis["app_state"]["ccvconsumer"]["params"]["provider_reward_denoms"] = json!(["anom"]);
    genesis["app_state"]["ccvconsumer"]["params"]["reward_denoms"] = json!(["akudos"]);

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    let genesis_s = genesis.to_string();

    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;
    FileOptions::write_str(&format!("/logs/{chain_id}_genesis.json"), &genesis_s).await?;

    let addr: &String = &cosmovisor_get_addr("validator").await?;

    // we need some native token in the bank, and don't need gentx
    sh_cosmovisor("add-genesis-account", &[addr, &token18(2.0e6, "akudos")]).await?;

    fast_block_times(daemon_home).await?;
    set_minimum_gas_price(daemon_home, "1akudos").await?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path).await?,
    )
    .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "onomyd" => onomyd_runner(&args).await,
            "consumer" => consumer(&args).await,
            "hermes" => hermes_runner(&args).await,
            _ => format!("entry_name \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        sh("go build ./cmd/consumer-democracy", &[]).await?;
        sh(
            "cp ./consumer-democracy ./tests/dockerfiles/dockerfile_resources/havend",
            &[],
        )
        .await?;
        container_runner(&args).await
    }
}

async fn container_runner(args: &Args) -> Result<()> {
    let logs_dir = "./tests/logs";
    let dockerfiles_dir = "./tests/dockerfiles";
    let bin_entrypoint = &args.bin_name;
    let container_target = "x86_64-unknown-linux-gnu";

    // build internal runner with `--release`
    sh("cargo build --release --bin", &[
        bin_entrypoint,
        "--target",
        container_target,
    ])
    .await?;

    // prepare volumed resources
    remove_files_in_dir("./tests/resources/keyring-test/", &[".address", ".info"]).await?;

    // prepare hermes config
    write_hermes_config(
        &[
            HermesChainConfig::new("onomy", "onomy", false, "anom", true),
            HermesChainConfig::new(CONSUMER_ID, CONSUMER_ACCOUNT_PREFIX, true, "akudos", true),
        ],
        &format!("{dockerfiles_dir}/dockerfile_resources"),
    )
    .await?;

    let entrypoint = Some(format!(
        "./target/{container_target}/release/{bin_entrypoint}"
    ));
    let entrypoint = entrypoint.as_deref();

    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "hermes",
                Dockerfile::Contents(dockerfile_hermes("__tmp_hermes_config.toml")),
                entrypoint,
                &["--entry-name", "hermes"],
            ),
            Container::new(
                "onomyd",
                Dockerfile::Contents(dockerfile_onomyd()),
                entrypoint,
                &["--entry-name", "onomyd"],
            )
            .volumes(&[(
                "./tests/resources/keyring-test",
                "/root/.onomy/keyring-test",
            )]),
            Container::new(
                "havend",
                Dockerfile::Contents(onomy_std_cosmos_daemon(
                    "havend",
                    ".onomy_haven",
                    "v0.1.0",
                    "havend",
                )),
                entrypoint,
                &["--entry-name", "consumer"],
            )
            .volumes(&[(
                "./tests/resources/keyring-test",
                "/root/.onomy_haven/keyring-test",
            )]),
        ],
        Some(dockerfiles_dir),
        true,
        logs_dir,
    )?
    .add_common_volumes(&[(logs_dir, "/logs")]);
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await?;
    Ok(())
}

async fn hermes_runner(_args: &Args) -> Result<()> {
    let mut nm_onomyd = NetMessenger::listen_single_connect("0.0.0.0:26000", TIMEOUT).await?;

    // get mnemonic from onomyd
    let mnemonic: String = nm_onomyd.recv().await?;
    // set keys for our chains
    FileOptions::write_str("/root/.hermes/mnemonic.txt", &mnemonic).await?;
    sh_hermes(
        "keys add --chain onomy --mnemonic-file /root/.hermes/mnemonic.txt",
        &[],
    )
    .await?;
    sh_hermes(
        &format!("keys add --chain {CONSUMER_ID} --mnemonic-file /root/.hermes/mnemonic.txt"),
        &[],
    )
    .await?;

    // wait for setup
    nm_onomyd.recv::<()>().await?;

    let ibc_pair = IbcPair::hermes_setup_ics_pair(CONSUMER_ID, "onomy").await?;
    let mut hermes_runner = hermes_start("/logs/hermes_bootstrap_runner.log").await?;
    ibc_pair.hermes_check_acks().await?;

    // tell that chains have been connected
    nm_onomyd.send::<IbcPair>(&ibc_pair).await?;

    // termination signal
    nm_onomyd.recv::<()>().await?;
    hermes_runner.terminate(TIMEOUT).await?;
    Ok(())
}

async fn onomyd_runner(args: &Args) -> Result<()> {
    let consumer_id = CONSUMER_ID;
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    let mut nm_hermes = NetMessenger::connect(STD_TRIES, STD_DELAY, "hermes:26000")
        .await
        .map_add_err(|| ())?;
    let mut nm_consumer =
        NetMessenger::connect(STD_TRIES, STD_DELAY, &format!("{consumer_id}d:26001"))
            .await
            .map_add_err(|| ())?;

    let mnemonic = onomyd_setup(daemon_home).await?;
    // send mnemonic to hermes
    nm_hermes.send::<String>(&mnemonic).await?;

    // keep these here for local testing purposes
    let addr = &cosmovisor_get_addr("validator").await?;
    sleep(Duration::ZERO).await;

    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", None).await?;

    let ccvconsumer_state = cosmovisor_add_consumer(
        daemon_home,
        consumer_id,
        &test_proposal(consumer_id, "akudos"),
    )
    .await?;

    sh_cosmovisor_tx("provider register-consumer-reward-denom", &[
        IBC_KUDOS,
        "--fees",
        "1000000anom",
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ])
    .await?;

    // send to consumer
    nm_consumer.send::<String>(&ccvconsumer_state).await?;

    // send keys
    nm_consumer
        .send::<String>(
            &FileOptions::read_to_string(&format!("{daemon_home}/config/node_key.json")).await?,
        )
        .await?;
    nm_consumer
        .send::<String>(
            &FileOptions::read_to_string(&format!("{daemon_home}/config/priv_validator_key.json"))
                .await?,
        )
        .await?;

    // wait for consumer to be online
    nm_consumer.recv::<()>().await?;
    // notify hermes to connect the chains
    nm_hermes.send::<()>(&()).await?;
    // when hermes is done
    let ibc_pair = nm_hermes.recv::<IbcPair>().await?;
    info!("IbcPair: {ibc_pair:?}");

    // send anom to consumer
    ibc_pair
        .b
        .cosmovisor_ibc_transfer(
            "validator",
            &reprefix_bech32(addr, CONSUMER_ACCOUNT_PREFIX)?,
            &token18(100.0e3, ""),
            "anom",
        )
        .await?;
    // it takes time for the relayer to complete relaying
    wait_for_num_blocks(4).await?;
    // notify consumer that we have sent NOM
    nm_consumer.send::<IbcPair>(&ibc_pair).await?;

    // recieve round trip signal
    nm_consumer.recv::<()>().await?;
    // check that the IBC NOM converted back to regular NOM
    assert_eq!(
        cosmovisor_get_balances("onomy1gk7lg5kd73mcr8xuyw727ys22t7mtz9gh07ul3").await?["anom"],
        "5000"
    );

    // by now we have accumulated some IBC Kudos rewards, we claim them and test
    // that they can be converted back
    sh_cosmovisor_tx(
        "distribution withdraw-all-rewards -y -b block --fees 1000000anom --from validator",
        &[],
    )
    .await?;
    ibc_pair
        .a
        .cosmovisor_ibc_transfer_with_flags(KUDOS_TEST_ADDR, &format!("7000{IBC_KUDOS}"), &[
            "--from",
            "validator",
            "-y",
            "-b",
            "block",
            "--gas",
            "auto",
            "--gas-adjustment",
            "1.3",
            "--gas-prices",
            "1anom",
        ])
        .await?;
    wait_for_num_blocks(4).await?;
    // wait for kudos check to complete
    nm_consumer.send::<()>(&()).await?;
    nm_consumer.recv::<()>().await?;

    // signal to collectively terminate
    nm_hermes.send::<()>(&()).await?;
    nm_consumer.send::<()>(&()).await?;
    cosmovisor_runner.terminate(TIMEOUT).await?;

    FileOptions::write_str(
        "/logs/onomyd_export.json",
        &sh_cosmovisor_no_dbg("export", &[]).await?,
    )
    .await?;

    Ok(())
}

async fn consumer(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    let chain_id = CONSUMER_ID;
    let mut nm_onomyd = NetMessenger::listen_single_connect("0.0.0.0:26001", TIMEOUT).await?;
    // we need the initial consumer state
    let ccvconsumer_state_s: String = nm_onomyd.recv().await?;

    havend_setup(daemon_home, chain_id, &ccvconsumer_state_s).await?;

    // get keys
    let node_key = nm_onomyd.recv::<String>().await?;
    // we used same keys for consumer as producer, need to copy them over or else
    // the node will not be a working validator for itself
    FileOptions::write_str(&format!("{daemon_home}/config/node_key.json"), &node_key).await?;

    let priv_validator_key = nm_onomyd.recv::<String>().await?;
    FileOptions::write_str(
        &format!("{daemon_home}/config/priv_validator_key.json"),
        &priv_validator_key,
    )
    .await?;

    let mut cosmovisor_runner =
        cosmovisor_start(&format!("{chain_id}d_bootstrap_runner.log"), None).await?;

    let addr = &cosmovisor_get_addr("validator").await?;

    // signal that we have started
    nm_onomyd.send::<()>(&()).await?;

    // wait for producer to send us stuff
    let ibc_pair = nm_onomyd.recv::<IbcPair>().await?;
    // get the name of the IBC NOM. Note that we can't do this on the onomyd side,
    // it has to be with respect to the consumer side
    let ibc_nom = &ibc_pair.a.get_ibc_denom("anom").await?;
    assert_eq!(ibc_nom, ONOMY_IBC_NOM);
    let balances = cosmovisor_get_balances(addr).await?;
    assert!(balances.contains_key(ibc_nom));

    // test normal transfer
    let dst_addr = &reprefix_bech32(
        "onomy1gk7lg5kd73mcr8xuyw727ys22t7mtz9gh07ul3",
        CONSUMER_ACCOUNT_PREFIX,
    )?;
    cosmovisor_bank_send(addr, dst_addr, "5000", "akudos").await?;
    assert_eq!(cosmovisor_get_balances(dst_addr).await?["akudos"], "5000");

    let test_addr = &reprefix_bech32(
        "onomy1gk7lg5kd73mcr8xuyw727ys22t7mtz9gh07ul3",
        PROVIDER_ACCOUNT_PREFIX,
    )?;
    info!("sending back to {}", test_addr);

    // send some IBC NOM back to origin chain
    ibc_pair
        .a
        .cosmovisor_ibc_transfer_with_flags(test_addr, &format!("5000{ibc_nom}"), &[
            "--from",
            "validator",
            "-y",
            "-b",
            "block",
            "--gas",
            "auto",
            "--gas-adjustment",
            "1.3",
            "--gas-prices",
            "1akudos",
        ])
        .await?;
    wait_for_num_blocks(4).await?;

    let pubkey = sh_cosmovisor("tendermint show-validator", &[]).await?;
    let pubkey = pubkey.trim();
    sh_cosmovisor_tx("staking", &[
        "create-validator",
        "--commission-max-change-rate",
        "0.01",
        "--commission-max-rate",
        "0.10",
        "--commission-rate",
        "0.05",
        "--from",
        "validator",
        "--min-self-delegation",
        "1",
        "--amount",
        &token18(1.0e3, "akudos"),
        "--fees",
        "1000000akudos",
        "--pubkey",
        pubkey,
        "-y",
        "-b",
        "block",
    ])
    .await?;

    // round trip signal
    nm_onomyd.send::<()>(&()).await?;

    // signal to check for IBC Kudos conversion
    nm_onomyd.recv::<()>().await?;
    assert_eq!(
        cosmovisor_get_balances(KUDOS_TEST_ADDR).await?["akudos"],
        "7000"
    );
    // finished checking
    nm_onomyd.send::<()>(&()).await?;

    // termination signal
    nm_onomyd.recv::<()>().await?;

    cosmovisor_runner.terminate(TIMEOUT).await?;

    let exported = sh_cosmovisor_no_dbg("export", &[]).await?;
    FileOptions::write_str(&format!("/logs/{chain_id}_export.json"), &exported).await?;

    Ok(())
}
