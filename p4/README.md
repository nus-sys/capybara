# P4 Programs for Capybara

P4 programs for Tofino switch (BF-SDE 9.4.0).

## Sync to Switch

After modifying P4 code locally, sync to sw1:

```bash
./p4/sync_to_sw1.sh
```

## Directory Structure

```
p4/
├── switch_fe/          # Frontend switch (load balancing)
├── port_forward/       # Simple port forwarding
├── prism/              # Prism proxy
├── endhost_switch/     # Endhost switch
├── capybara_msr/       # MSR (migration state replication)
├── resource_utilization/  # Resource utilization monitoring
└── includes/           # Shared includes
```

## Build

On switch (sw1):

```bash
# Build P4 program
~/tools/p4_build.sh ~/inho/Capybara/capybara/p4/<program>.p4

# Example: Build switch_fe with server-side source rewriting
~/tools/p4_build.sh ~/inho/Capybara/capybara/p4/switch_fe/capybara_switch_fe_src_rewriting_by_server.p4
```

## Run P4 Program

After building, run the P4 program on switch:

```bash
cd ~/bf-sde-9.4.0
./run_switchd.sh -p <program_name>

# Example: Run switch_fe with server-side source rewriting
./run_switchd.sh -p capybara_switch_fe_src_rewriting_by_server
```

## Run Setup Scripts

After building, run setup script to configure tables:

```bash
# SSH to switch and run setup
ssh sw1 "source /home/singtel/tools/set_sde.bash && \
    /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b \
    /home/singtel/inho/Capybara/capybara/p4/<folder>/<setup_script>.py"
```

## Available Programs

### switch_fe (Frontend Switch)

| P4 Program | Setup Script | Description |
|------------|--------------|-------------|
| `capybara_switch_fe.p4` | `capybara_switch_fe_setup.py` | Basic load balancing (backends on node9) |
| `capybara_switch_fe_src_rewriting_by_server.p4` | `capybara_switch_fe_src_rewriting_by_server_setup.py` | Server-side source rewriting (backends on node8/9/10) |
| `capybara_switch_fe_ip_port_hash.p4` | - | IP+port based hashing |

### port_forward

| P4 Program | Setup Script | Description |
|------------|--------------|-------------|
| `port_forward.p4` | `port_forward.py` | Simple L2/L3 forwarding |

### Others

- `prism/`: Prism proxy setup
- `endhost_switch/`: Endhost-based switching
- `capybara_msr/`: Migration state replication
- `resource_utilization/`: Resource monitoring

## Backend Configuration

`capybara_switch_fe_src_rewriting_by_server_setup.py` configures:
- Backends: node8 (10.0.1.8), node9 (10.0.1.9), node10 (10.0.1.10)
- Ports: 10000-10003 per node
- Round-robin load balancing across backends

## Quick Reference

```bash
# 0. Sync local changes to sw1
./p4/sync_to_sw1.sh

# 1. Build (on sw1)
~/tools/p4_build.sh ~/inho/Capybara/capybara/p4/switch_fe/capybara_switch_fe_src_rewriting_by_server.p4

# 2. Run P4 program (on sw1, in separate terminal)
cd ~/bf-sde-9.4.0
./run_switchd.sh -p capybara_switch_fe_src_rewriting_by_server

# 3. Run setup script (after switchd is running)
ssh sw1 "source /home/singtel/tools/set_sde.bash && \
    /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b \
    /home/singtel/inho/Capybara/capybara/p4/switch_fe/capybara_switch_fe_src_rewriting_by_server_setup.py"
```
