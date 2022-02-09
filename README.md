# Sui (pre-alpha)

[![Build Status](https://github.com/mystenlabs/fastnft/actions/workflows/rust.yml/badge.svg)](https://github.com/mystenlabs/fastnft/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-Apache-green.svg)](LICENSE.md)

This repository is dedicated to sharing material related to the Sui protocol, developed at Novi Financial (formerly Calibra). Software is provided for research-purpose only and is not meant to be used in production.

## Summary

Sui extends Sui by allowing objects to be transacted.

Sui allows a set of distributed authorities, some of which are Byzantine, to maintain a high-integrity and availability settlement system for pre-funded payments. It can be used to settle payments in a native unit of value (crypto-currency), or as a financial side-infrastructure to support retail payments in fiat currencies. Sui is based on Byzantine Consistent Broadcast as its core primitive, foregoing the expenses of full atomic commit channels (consensus). The resulting system has low-latency for both confirmation and payment finality. Remarkably, each authority can be sharded across many machines to allow unbounded horizontal scalability. Our experiments demonstrate intra-continental confirmation latency of less than 100ms, making Sui applicable to point of sale payments. In laboratory environments, we achieve over 80,000 transactions per second with 20 authorities---surpassing the requirements of current retail card payment networks, while significantly increasing their robustness.

## Quickstart with local Sui network and interactive wallet
### 1. Build the binaries
```shell
cargo build --release
cd target/release
```
This will create `fastx` and `wallet` binaries in `target/release` directory. 

### 2. Genesis
```shell
./fastx genesis
```
The genesis command creates 4 authorities, 5 user accounts each with 5 gas objects.
The network configuration are stored in `network.conf` and can be used subsequently to start the network.  
A `wallet.conf` will also be generated to be used by the `wallet` binary to manage the newly created accounts.

### 3. Starting the network
Run the following command to start the local Sui network: 
```shell
./fastx start 
```
or 
```shell
./fastx start --config [config file path]
```
The network config file path is defaulted to `./network.conf` if not specified.  

### 4. Running interactive wallet
To start the interactive wallet:
```shell
./wallet
```
or
```shell
./wallet --config [config file path]
```
The wallet config file path is defaulted to `./wallet.conf` if not specified.  

The following commands are supported by the interactive wallet:

    addresses      Obtain the Account Addresses managed by the wallet
    call           Call Move
    help           Prints this message or the help of the given subcommand(s)
    new-address    Generate new address and keypair
    object         Get obj info
    objects        Obtain all objects owned by the account address
    publish        Publish Move modules
    sync           Synchronize client state with authorities
    transfer       Transfer funds

Use `help <command>` to see more information on each command.

### 5. Using the wallet without interactive shell
The wallet can also be use without the interactive shell
```shell
USAGE:
    wallet --no-shell [SUBCOMMAND]
```
#### Example
```shell
sui@MystenLab release % ./wallet --no-shell addresses                                                                         
Showing 5 results.
4f145f9a706ae4932452c90ce006fbddc8ab2ced34584f26d8953df14a76463e
47ea7f45ca66fc295cd10fbdf8a41828db2ed71c145476c710e04871758f48e9
e91c22628771f1465947fe328ed47983b1a1013afbdd1c8ded2009ec4812054d
9420c11579a0e4a75a48034d9617fd68406de4d59912e9d08f5aaf5808b7013c
1be661a8d7157bffbb2cf7f652d270bbefb07b0b436aa10f2c8bdedcadcc22cb
```

## Quickstart with Sui Prototype

```bash
cargo build --release
cd target/release
rm -f *.json *.toml
rm -rf db*
killall server

# Create DB dirs and configuration files for 4 authorities.
# * Private server states are stored in `server*.json`.
# * `committee.json` is the public description of the Sui committee.
for I in 1 2 3 4
do
    mkdir ./db"$I"
    ./server --server server"$I".json generate --host 127.0.0.1 --port 9"$I"00 --database-path ./db"$I" >> committee.json
done

# Create configuration files for 100 user accounts, with 10 gas objects per account and 2000000 value each.
# * Private account states are stored in one local wallet `accounts.json`.
# * `initial_accounts.toml` is used to mint the corresponding initially randomly generated (for now) objects at startup on the server side.
./client --committee committee.json --accounts accounts.json create-accounts --num 100 \
--gas-objs-per-account 10 --value-per-obj 2000000 initial_accounts.toml
# Start servers
for I in 1 2 3 4
do
    ./server --server server"$I".json run --initial-accounts initial_accounts.toml --committee committee.json &
done
 
# Query account addresses
./client --committee committee.json --accounts accounts.json query-accounts-addrs

# Query (locally cached) object info for first and last user account
ACCOUNT1=`./client --committee committee.json --accounts accounts.json query-accounts-addrs | head -n 1`
ACCOUNT2=`./client --committee committee.json --accounts accounts.json query-accounts-addrs | tail -n -1`
./client --committee committee.json --accounts accounts.json query-objects --address "$ACCOUNT1"
./client --committee committee.json --accounts accounts.json query-objects --address "$ACCOUNT2"

# Get the first ObjectId for Account1
ACCOUNT1_OBJECT1=`./client --committee committee.json --accounts accounts.json query-objects --address "$ACCOUNT1" | head -n 1 |  awk -F: '{ print $1 }'`
# Pick the last item as gas object for Account1
ACCOUNT1_GAS_OBJECT=`./client --committee committee.json --accounts accounts.json query-objects --address "$ACCOUNT1" | tail -n -1 |  awk -F: '{ print $1 }'`
# Transfer object by ObjectID
./client --committee committee.json --accounts accounts.json transfer "$ACCOUNT1_OBJECT1" "$ACCOUNT1_GAS_OBJECT" --to "$ACCOUNT2"
# Query objects again
./client --committee committee.json --accounts accounts.json query-objects --address "$ACCOUNT1"
./client --committee committee.json --accounts accounts.json query-objects --address "$ACCOUNT2"

# Launch local benchmark using all user accounts
./client --committee committee.json --accounts accounts.json benchmark

# Inspect state of first account
grep "$ACCOUNT1" accounts.json

# Kill servers
kill %1 %2 %3 %4 %5 %6 %7 %8 %9 %10 %11 %12 %13 %14 %15 %16

# Additional local benchmark
./bench

cd ../..
```

## References

* Sui is based on FastPay: [FastPay: High-Performance Byzantine Fault Tolerant Settlement](https://arxiv.org/pdf/2003.11506.pdf)

## Contributing

Read [Eng Plan](https://docs.google.com/document/d/1Cqxaw23PR2hc5bkbhXIDCnWjxA3AbfjsuB45ltWns4U/edit#).

## License

The content of this repository is licensed as [Apache 2.0](https://github.com/MystenLabs/fastnft/blob/update-readme/LICENSE)
