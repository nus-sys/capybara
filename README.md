# Running on nsl cluster
## Cluster Information
- We use three nodes (node 7, 8, 9) and one switch (sw1)
- Node 7 runs client (Caladan), node 8 runs tcpdump, and node 9 runs server (Capybara)
- (Important) You need to setup some ssh configurations to run this test. Please let me know for this configuration. 

## Setup
#### 1. On node 7 (client)
- On your $HOME directory
- Run `git clone https://github.com/ihchoi12/caladan.git && cd caladan`
- Run `make submodules && make -j`
- Run `cd ksched; make ; cd ..`
- Run `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --default-toolchain nightly`
- Run `cd apps/synthetic; cargo build --release; cd ../../;`

#### 2. On node 8 (tcpdump)
- Run `mkdir /local/$USER/capybara-pcap`

#### 3. On node 9 (server)
- On your $HOME directory
- Run `mkdir Capybara && cd Capybara`
- Run `git clone https://github.com/nus-sys/capybara.git && cd capybara`
- Run `./scripts/setup/dpdk.sh`
- Run `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Run `make LibOS=catnip all-examples-rust`

## Run test
#### On node 7 
- Run `mkdir $HOME/capybara-data`
- Open a virtual screen (e.g., tmux).
- Run `cd ~/caladan; sudo scripts/setup_machine.sh; sudo ./iokerneld ias nicpci 0000:31:00.1` on a screen.
- Edit `~/Capybara/capybara/eval/test_config.py` according to the test you want to run
- Run (once) `pip3 install -r ~/Capybara/capybara/eval/requirements.txt`
- Run `cd ~/Capybara/capybara/eval`
- Run `python3 run_eval.py`
- It will run tests as in the python script. For each test, it will print out a summary of result, and store some log files into the `~/capybara-data/` directory.
- log files: 
  * [test_id].be[x]: console output from the backend server be[x] running on node9
  * [test_id].client: console output from the client running on node 7
  * [test_id].latency: latency CDF
  * [test_id].latency_raw: latency of each request



## pcap data
- If you set `TCPDUMP = True` in the `~/Capybara/capybara/eval/test_config.py` file, the script will run only the first test, parse the pcap data files, and then terminate the script.
- It will store some log files generated from pcap trace into the `node8:/local/$USER/capybara-pcap/` directory
- log files: 
  * [test_id].pcap: original pcap file
  * [test_id].csv: pcap data parsed into a csv format
  * [test_id].request_times: timestamps of captured request packets in order
  * [test_id].response_times: timestamps of captured response packets in order
  * [test_id].pcap_latency: latency of each request in order



#
# Running `redis-server`

- Make sure that the [capybara-redis](https://github.com/nus-sys/capybara-redis) repo is in the same parent directory as this repo.
- Run `make redis-server` to compile `redis-server` without migration.
    OR
  Run `make redis-server-mig` to compile `redis-server` with migration.
- Run `make run-redis-server` to run `redis-server`. By default, Redis uses the `redis.conf` config file. Use a custom config file through environment variable `REDIS_CONF` (for replication, for instance).

# Demikernel

[![Join us on Slack!](https://img.shields.io/badge/chat-on%20Slack-e01563.svg)](https://join.slack.com/t/demikernel/shared_invite/zt-11i6lgaw5-HFE_IAls7gUX3kp1XSab0g)
[![Catnip LibOS](https://github.com/demikernel/demikernel/actions/workflows/catnip.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catnip.yml)
[![Catnap LibOS](https://github.com/demikernel/demikernel/actions/workflows/catnap.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catnap.yml)
[![Catpowder LibOS](https://github.com/demikernel/demikernel/actions/workflows/catpowder.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catpowder.yml)
[![Catcollar LibOS](https://github.com/demikernel/demikernel/actions/workflows/catcollar.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catcollar.yml)

_Demikernel_ is a library operating system (LibOS) architecture designed for use
with kernel-bypass I/O devices. This architecture offers a uniform system call
API across kernel-bypass technologies (e.g., RDMA, DPDK) and OS functionality
(e.g., a user-level networking stack for DPDK).

To read more about the motivation behind the _Demikernel_, check out
this [blog
post](http://irenezhang.net/blog/2019/05/21/demikernel.html).

To get details about the system, read our paper in [SOSP '21](https://doi.org/10.1145/3477132.3483569).

> To read more about Demikernel check out <https://aka.ms/demikernel>.

## Building

> **Follow these instructions to build Demikernel on a fresh Ubuntu 20.04 system.**

### 1. Clone This Repository

```bash
export WORKDIR=$HOME                                                  # Change this to whatever you want.
cd $WORKDIR                                                           # Switch to working directory.
git clone --recursive https://github.com/demikernel/demikernel.git    # Recursive clone.
cd $WORKDIR/demikernel                                                # Switch to repository's source tree.
```

### 2. Install Prerequisites (Only Once)

```bash
sudo -H scripts/setup/debian.sh                                   # Install third party libraries.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh    # Get Rust toolchain.
```

### 3. Build DPDK Libraries (For Catnip and Only Once)

```bash
./scripts/setup/dpdk.sh
```

### 4. Build Demikernel with Default Parameters

```bash
make
```

### 5. Build Demikernel with Custom Parameters (Optional)

```bash
make LIBOS=[catnap|catnip|catpowder|catcollar]    # Build using a specific LibOS.
make DRIVER=[mlx4|mlx5]                           # Build using a specific driver.
make LD_LIBRARY_PATH=/path/to/libs                # Override path to shared libraries. Applicable to Catnap and Catcollar.
make PKG_CONFIG_PATH=/path/to/pkgconfig           # Override path to config files. Applicable to Catnap and Catcollar.
```

### 6. Install Artifacts (Optional)

```bash
make install                                     # Copies build artifacts to your $HOME directory.
make install INSTALL_PREFIX=/path/to/location    # Copies build artifacts to a specific location.
```

## Running

> **Follow these instructions to run examples that are shipped in the source tree**.

### 1. Setup Configuration File (Only Once)

- Copy the template from `scripts/config/default.yaml` to `$HOME/config.yaml`.
- Open the file in `$HOME/config.yaml` for editing and do the following:
  - Change `XX.XX.XX.XX` to match the IPv4 address of your server host.
  - Change `YY.YY.YY.YY` to match the IPv4 address of your client host.
  - Change `PPPP` to the port number that you will expose in the server host.
  - Change `ZZ.ZZ.ZZ.ZZ` to match the IPv4 address that in the local host.
  - Change `ff:ff:ff:ff:ff:ff` to match the MAC address in the local host.
  - Change `abcde` to match the name of the interface in the local host.
  - Change the `arp_table` according to your setup.
  - If using DPDK, change `WW:WW.W` to match the PCIe address of your NIC.
- Save the file.

### 2. Enable Huge Pages (For Catnip at Every System Reboot)

```bash
sudo -E ./scripts/setup/hugepages.sh
```

### 3. Run UDP Push-Pop Demo

> For Catnap and Catcollar, you don't need to run with super-user privileges.

```bash
# Server-Side
PEER=server TEST=udp_push_pop sudo -E make LIBOS=catnip test-system

# Client-Side
PEER=client TEST=udp_push_pop sudo -E make LIBOS=catnip test-system
```

### 4. Run UDP Ping-Pong Demo

> For Catnap and Catcollar, you don't need to run with super-user privileges.

```bash
# Server-Side
PEER=server TEST=udp_ping_pong sudo -E make LIBOS=catnip test-system

# Client-Side
PEER=client TEST=udp_ping_pong sudo -E make LIBOS=catnip test-system
```

## Documentation

- Legacy system call API documentation [`doc/syscalls.md`](./doc/syscalls.md)
- Instructions for running Demikernel on CloudLab [`doc/cloudlab.md`](./doc/cloudlab.md)

### 1. Build API Documentation (Optional)

```bash
cargo doc --no-deps    # Build API Documentation
cargo doc --open       # Open API Documentation
```

## Code of Conduct

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/)
or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Contributing

See [CONTRIBUTING](./CONTRIBUTING) for details regarding how to contribute
to this project.

## Usage Statement

This project is a prototype. As such, we provide no guarantees that it will
work and you are assuming any risks with using the code. We welcome comments
and feedback. Please send any questions or comments to one of the following
maintainers of the project:

- [Irene Zhang](https://github.com/iyzhang) - [irene.zhang@microsoft.com](mailto:irene.zhang@microsoft.com)
- [Pedro Henrique Penna](https://github.com/ppenna) - [ppenna@microsoft.com](mailto:ppenna@microsoft.com)

> By sending feedback, you are consenting that it may be used  in the further
> development of this project.
