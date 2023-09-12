
# Multiverse

This repo is used as the common base for Onomy network consumer chains.

# Creating a consumer chain on testnet

Someone runs a test chain until after the proposal is complete, and runs `cosmovisor run query provider consumer genesis [consumer chain id]` to get an example consumer chain genesis. "soft_opt_out_threshold", "provider_reward_denoms", and "reward_denoms" currently have to be manually set in the consumer genesis file bcause of technical reasons (a future ICS upgrade will fix this). We agree on a genesis template and a corresponding proposal.

Someone runs a "consumer-addition" proposal with the argument file
```
{
    "title": "Propose the addition of a new chain",
    "description": "Add the [name] consumer chain",
    "chain_id": "[chain-id]",
    "initial_height": {
        "revision_number": 0,
        "revision_height": 1
    },
    "genesis_hash": "Z2VuX2hhc2g=",
    "binary_hash": "YmluX2hhc2g=",
    "spawn_time": "2023-05-18T01:15:49.83019476-05:00",
    "unbonding_period": 1728000000000000,
    "consumer_redistribution_fraction": "0.5",
    "provider_reward_denoms": ["anom"],
    "reward_denoms": ["anative"],
    "blocks_per_distribution_transmission": 1000,
    "soft_opt_out_threshold": "0.0",
    "historical_entries": 10000,
    "ccv_timeout_period": 2419200000000000,
    "transfer_timeout_period": 3600000000000,
    "deposit": "500000000000000000000anom"
}
```
- "genesis_hash" is used for off-chain confirmation of the genesis state without CCV module params (e.x. `cat genesis.json | openssl dgst -binary -sha256 | openssl base64 -A`)
- "binary_hash" is used for off-chain confirmation of the hash of the initialization binary
- "spawn_time" is the time at which validators will be responsible for starting their consumer binaries
- "unbonding_period" is the unbonding period, should be less than the unbonding period for the provider (e.x. 24 hours less than the standard 21 days)
- "ccv_timeout_period" timeout period of CCV related IBC packets
- "transfer_timeout_period" timeout period of transfer related IBC packets
- "consumer_redistribution_fraction": "0.75" means that 75% of distribution events will be allocated to be sent back to the provider through the `cons_redistribute` address
- "soft_opt_out_threshold" should only be nonzero on really large PoS provider chains that want to be easier on smaller validators, Onomy is more strict
- "deposit" the deposit is included with the proposal command, which is 500 NOM for Onomy

After the proposal passes, each validator needs to run
`cosmovisor run tx provider assign-consensus-key [consumer chain id] [tendermint key] [flags]`
where the tendermint key could be from `cosmovisor run tendermint show-validator` if the same key is going to be used for the consumer node.
e.x.
`cosmovisor run tx provider assign-consensus-key haven '{"@type":"/cosmos.crypto.ed25519.PubKey","key":"2YSpwSW4FhMxIOhBmGpyyLGIDKszRA1v+HSRPuMMcQk="}' --fees 1000000anom -y -b block --from validator`

If the consumer chain has its own staking coin, a team member needs to run `cosmovisor run tx provider register-consumer-reward-denom [IBC-version-of-consumer-reward-denom]` in order for redistribution to start working later.

After a supermajority of validators assign their consensus keys, we run `cosmovisor run query provider consumer genesis [consumer chain id]` to get the real CCV state and insert it into the consumer genesis. The params are missing "soft_opt_out_threshold", "provider_reward_denoms", and "reward_denoms" again so those need to be added. Validators need to get their consumer nodes ready with this complete genesis and the same tendermint keys that they assigned earlier.

Upon chain start, a team member will initialize the ICS channels, the transfer channel of which will be the canonical IBC NOM channel. Bootstrap is complete and normal transactions can start once the first CCV packets start arriving (else you will receive "tx contains unsupported message types" errors).

Validators also need to bond their consumer-side validators with `cosmovisor run tx staking create-validator` with their consumer node tendermint keys (their nodes will work without this, but the consumer-side governance will use consumer-side staked tokens for determining voting weights). In a future version of ICS, if the consumer chain does not have its own staking denom, we will probably incorporate provider-driven governance for binary upgrades and some parameter changes.