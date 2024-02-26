//! Given an exported genesis of a provider, this translates the bonded amounts
//! to `aonex` balances that will be put into a partial genesis without accounts
//! to create the partial genesis
//!
//! NOTE this will overwrite the file at `partial-genesis-path`, use source
//! control

#[rustfmt::skip]
/*
e.x.

cargo r --bin process_accounts --release -- --partial-genesis-without-accounts-path ./../environments/mainnet/onex-mainnet/partial-genesis-without-accounts.json --exported-genesis-path ./../../../Downloads/mainnet-snapshot-for-onex.json --partial-genesis-path ./../environments/mainnet/onex-mainnet/partial-genesis.json

*/

use std::collections::{btree_map::Entry, BTreeMap, HashSet};

use clap::Parser;
use common::MODULE_ACCOUNTS;
use onomy_test_lib::super_orchestrator::{
    stacked_errors::{Result, StackableErr},
    stacked_get, stacked_get_mut, std_init, FileOptions,
};
use serde::ser::Serialize;
use serde_json::{json, ser::PrettyFormatter, Serializer, Value};
use u64_array_bigints::U256;

#[derive(Parser, Debug, Clone)]
#[command(about)]
struct Args {
    #[arg(long)]
    pub partial_genesis_without_accounts_path: String,
    #[arg(long)]
    pub exported_genesis_path: String,
    #[arg(long)]
    pub partial_genesis_path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;
    let args = Args::parse();
    //let logs_dir = "./tests/logs";

    // must remove these from accounts
    let module_accounts = MODULE_ACCOUNTS;
    let module_accounts: HashSet<&str> = module_accounts.iter().cloned().collect();

    let partial_genesis_without_accounts =
        FileOptions::read_to_string(&args.partial_genesis_without_accounts_path)
            .await
            .stack()?;
    let exported_genesis = FileOptions::read_to_string(&args.exported_genesis_path)
        .await
        .stack()?;

    let exported: Value = serde_json::from_str(&exported_genesis).stack()?;
    let mut genesis: Value = serde_json::from_str(&partial_genesis_without_accounts).stack()?;

    let validators_value: &[Value] = stacked_get!(exported["app_state"]["staking"]["validators"])
        .as_array()
        .stack()?;

    struct Total {
        shares: U256,
        tokens: U256,
    }
    let mut validators: BTreeMap<String, Total> = BTreeMap::new();
    for validator in validators_value {
        let shares = &validator["delegator_shares"];
        let shares = shares.as_str().unwrap();
        // the shares can be fractional, truncate at the decimal point
        let i = shares.find('.').unwrap();
        let shares = &shares[..i];
        let shares = U256::from_dec_or_hex_str(shares).unwrap();
        let tokens = U256::from_dec_or_hex_str(validator["tokens"].as_str().unwrap()).unwrap();
        validators.insert(
            validator["operator_address"].as_str().unwrap().to_owned(),
            Total { shares, tokens },
        );
    }

    // use only bonded amounts
    let delegations: &[Value] = stacked_get!(exported["app_state"]["staking"]["delegations"])
        .as_array()
        .stack()?;

    let mut allocations = BTreeMap::<String, u128>::new();
    for delegation in delegations {
        let address = stacked_get!(delegation["delegator_address"]);
        let address = address.as_str().unwrap();
        if module_accounts.contains(address) {
            // there shouldn't be any modules delegating to anyone
            panic!();
            //continue
        }
        let shares = stacked_get!(delegation["shares"]);
        let shares = shares.as_str().unwrap();
        // the shares can be fractional, truncate at the decimal point
        let i = shares.find('.').unwrap();
        let shares = &shares[..i];
        let shares = U256::from_dec_or_hex_str(shares).unwrap();

        let total = validators
            .get(
                stacked_get!(delegation["validator_address"])
                    .as_str()
                    .stack()?,
            )
            .unwrap();

        // delegated tokens = (shares * total_tokens) / total_shares
        let tmp = shares.checked_mul(total.tokens).unwrap();
        let tmp = tmp.divide(total.shares).unwrap().0;
        let tmp = tmp.try_resize_to_u128().unwrap();

        match allocations.entry(address.to_owned()) {
            Entry::Vacant(v) => {
                v.insert(tmp);
            }
            Entry::Occupied(mut o) => {
                // if multiple delegations from same address, add them up
                *o.get_mut() = o.get().checked_add(tmp).unwrap();
            }
        }
    }

    let mut total_supply: u128 = allocations.values().sum();
    println!(
        "total supply: {total_supply} ({} * 10^18)",
        total_supply / 1000000000000000000
    );

    let result_denom = "aonex";

    // for manual testing
    /*allocations.insert(
        "onomy1y3c6q58vvuxr5tcmesay74wvhrey3pqv8g6y3r".to_owned(),
        1000000000000000000077,
    );*/

    // for onex mainnet: special address with 5% of liquidity
    let special = ((0.05f64 / 0.95f64) * (total_supply as f64)) as u128;
    let any_already_inserted = allocations.insert(
        "onomy1cn8dfn77allkgte2hdfcpsypmsasy3lzeq9kcj".to_owned(),
        special,
    );
    assert!(any_already_inserted.is_none());
    println!(
        "special address: {special} ({} * 10^18)",
        special / 1000000000000000000
    );
    total_supply += special;
    println!(
        "total supply with special address: {total_supply} ({} * 10^18)",
        total_supply / 1000000000000000000
    );

    // alternatively, the partial without accounts can have some accounts and bank
    // balances with desired customization

    // special addresses excluded from the vesting schedule or minimum
    let base_account_addresses: &[&str] = &["onomy1cn8dfn77allkgte2hdfcpsypmsasy3lzeq9kcj"];

    let mut base_account_allocations = BTreeMap::<String, u128>::new();

    for address in base_account_addresses {
        let balance = allocations.remove(*address).unwrap();
        base_account_allocations.insert(address.to_string(), balance);
    }

    for (address, allocation) in base_account_allocations {
        let allocation = allocation.to_string();
        stacked_get_mut!(genesis["app_state"]["auth"]["accounts"])
            .as_array_mut()
            .stack()?
            .push(json!(
                {
                    "@type": "/cosmos.auth.v1beta1.BaseAccount",
                    "address": address,
                    "pub_key": null,
                    "account_number": "0",
                    "sequence": "0"
                }
            ));
        stacked_get_mut!(genesis["app_state"]["bank"]["balances"])
            .as_array_mut()
            .stack()?
            .push(json!(
                {
                "address": address,
                "coins": [
                    {
                        "denom": result_denom,
                        "amount": allocation
                    }
                ]
                }
            ));
    }

    // Exclude accounts with bonded amounts less than 100 NOM
    allocations.retain(|_, amount| *amount >= 100_000000000000000000);

    #[rustfmt::skip]
    /*
    cosmovisor run tx bank send special onomy183l3wc5xfl9k7qp8akhvnd4qwm9gmz0afmw2kp 166666666666666666678aonex -y -b block --from special --fees 1000000ibc/5872224386C093865E42B18BDDA56BCB8CDE1E36B82B391E97697520053B0513

    cosmovisor run query bank balances onomy1y3c6q58vvuxr5tcmesay74wvhrey3pqv8g6y3r

    cosmovisor run query account onomy1y3c6q58vvuxr5tcmesay74wvhrey3pqv8g6y3r

    cosmovisor run tx staking delegate onomyvaloper1yks83spz6lvrrys8kh0untt22399tskkx4l7y6 500000000000000000034aonex --from special -y -b block --gas 300000 --fees 10000000ibc/5872224386C093865E42B18BDDA56BCB8CDE1E36B82B391E97697520053B0513
    */

    let local_target_time: chrono::DateTime<chrono_tz::Tz>  = chrono::TimeZone::with_ymd_and_hms(&chrono_tz::US::Central, 2024, 3, 4, 10, 0, 0)
        .single()
        .stack()?;
    let utc_target_time = local_target_time.with_timezone(&chrono::Utc);
    println!(
        "genesis time: {}",
        utc_target_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    );

    // genesis time in UNIX time in seconds
    let start_time = u64::try_from(utc_target_time.timestamp()).unwrap();
    println!("UNIX genesis time: {}", start_time);
    // 30 days between each 1/12th vesting
    let period: u64 = 24 * 3600 * 30;
    let periods: u64 = 12;
    assert!(periods >= 2);
    let end_time = start_time + (period * periods);

    println!("length of each vesting period in seconds: {}", period);
    println!("number of vesting periods: {}", periods);

    let start_time = format!("{start_time}");
    let end_time = format!("{end_time}");
    let period = format!("{period}");

    // how the vesting periods work are that a number of coins are only allowed to
    // be sent to other accounts at the end of the period. The below configuration
    // is set to have the first 1/periods amound unlocked at genesis time by having
    // `periods - 1` actual vesting periods, and subtracting one vesting period's
    // worth from the `original_vesting`, so that there is some unlocked from the
    // balance

    // vesting
    for (address, allocation) in allocations {
        let allocation_per_period = allocation / u128::from(periods);
        // there is a slight error from the division, so we calculate an exact amount.
        // The most important thing is that the original_vesting is not less than the
        // sum of the period amounts.
        let total_balance = allocation_per_period * u128::from(periods);
        let original_vesting = allocation_per_period * u128::from(periods - 1);
        let total_balance = format!("{total_balance}");
        let original_vesting = format!("{original_vesting}");
        let allocation_per_period = format!("{allocation_per_period}");
        let mut vesting_periods = vec![];
        for _ in 0..(periods - 1) {
            vesting_periods.push(json!({
                "length": period,
                "amount": [
                    {
                        "denom": result_denom,
                        "amount": allocation_per_period
                    }
                ]
            }));
        }
        stacked_get_mut!(genesis["app_state"]["auth"]["accounts"])
            .as_array_mut()
            .stack()?
            .push(json!(
                {
                    "@type": "/cosmos.vesting.v1beta1.PeriodicVestingAccount",
                    "base_vesting_account": {
                        "base_account": {
                            "address": address,
                            "pub_key": null,
                            "account_number": "0",
                            "sequence": "0"
                        },
                        "original_vesting": [
                            {
                                "denom": result_denom,
                                "amount": original_vesting
                            }
                        ],
                        "delegated_free": [],
                        "delegated_vesting": [],
                        "end_time": end_time
                    },
                    "start_time": start_time,
                    "vesting_periods": vesting_periods
                }
            ));
        stacked_get_mut!(genesis["app_state"]["bank"]["balances"])
            .as_array_mut()
            .stack()?
            .push(json!(
                {
                "address": address,
                "coins": [
                    {
                        "denom": result_denom,
                        "amount": total_balance
                    }
                ]
                }
            ));
    }

    let mut genesis_s = vec![];
    let formatter = PrettyFormatter::with_indent(&[b' ', b' ']);
    let mut ser = Serializer::with_formatter(&mut genesis_s, formatter);
    genesis.serialize(&mut ser).stack()?;
    let genesis_s = String::from_utf8(genesis_s).stack()?;

    FileOptions::write_str(&args.partial_genesis_path, &genesis_s)
        .await
        .stack()?;

    Ok(())
}
