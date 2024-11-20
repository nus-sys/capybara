# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================

export PREFIX ?= $(HOME)
export INSTALL_PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH = $(CURDIR)/lib:$(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')
export RUSTFLAGS = -Awarnings

#=======================================================================================================================
# Build Configuration
#=======================================================================================================================

export BUILD := release
ifeq ($(DEBUG),yes)
export RUST_LOG ?= trace
export BUILD := dev
endif

#=======================================================================================================================
# Project Directories
#=======================================================================================================================

export BINDIR ?= $(CURDIR)/bin
export INCDIR := $(CURDIR)/include
export LIBDIR ?= $(CURDIR)/lib
export SRCDIR = $(CURDIR)/src
export BUILD_DIR := $(CURDIR)/target/release
ifeq ($(BUILD),dev)
export BUILD_DIR := $(CURDIR)/target/debug
endif
export INPUT ?= $(CURDIR)/nettest/input

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Rust
export CARGO ?= $(shell which cargo || echo "$(HOME)/.cargo/bin/cargo" )
export CARGO_FLAGS += --profile $(BUILD)

# C
export CFLAGS := -I $(INCDIR)
ifeq ($(DEBUG),yes)
export CFLAGS += -O0
else
export CFLAGS += -O3
endif

#=======================================================================================================================
# Libraries
#=======================================================================================================================

export DEMIKERNEL_LIB := libdemikernel.so
export LIBS := $(BUILD_DIR)/$(DEMIKERNEL_LIB)

#=======================================================================================================================
# Build Parameters
#=======================================================================================================================

export LIBOS ?= catnip
export CARGO_FEATURES := --features=$(LIBOS)-libos,catnap-libos --no-default-features

# Switch for DPDK
ifeq ($(LIBOS),catnip)
DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5,6]"`" ] && echo mlx5 || echo mlx4)
CARGO_FEATURES += --features=$(DRIVER)
endif

# Switch for profiler.
export PROFILER ?= no
ifeq ($(PROFILER),yes)
CARGO_FEATURES += --features=profiler
endif

ifeq ($(TCP_MIG),1)
CARGO_FEATURES += --features=tcp-migration
endif

ifeq ($(MANUAL_TCP_MIG),1)
CARGO_FEATURES += --features=manual-tcp-migration
endif

ifeq ($(CAPY_LOG),1)
CARGO_FEATURES += --features=capy-log
endif

CARGO_FEATURES += $(FEATURES)

#=======================================================================================================================

all: init | all-libs all-tests all-examples

init:
	mkdir -p $(LIBDIR)
	git config --local core.hooksPath .githooks

# Builds documentation.
doc:
	$(CARGO) doc $(FLAGS) --no-deps

# Copies demikernel artifacts to a INSTALL_PREFIX directory.
install:
	mkdir -p $(INSTALL_PREFIX)/include $(INSTALL_PREFIX)/lib
	cp -rf $(INCDIR)/* $(INSTALL_PREFIX)/include/
	cp -rf  $(LIBDIR)/* $(INSTALL_PREFIX)/lib/
	cp -f $(CURDIR)/scripts/config/default.yaml $(INSTALL_PREFIX)/config.yaml

#=======================================================================================================================
# Libs
#=======================================================================================================================

# Builds all libraries.
all-libs: all-shim all-libs-demikernel

all-libs-demikernel:
	@echo "LD_LIBRARY_PATH: $(LD_LIBRARY_PATH)"
	@echo "PKG_CONFIG_PATH: $(PKG_CONFIG_PATH)"
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS)
	cp -f $(BUILD_DIR)/$(DEMIKERNEL_LIB) $(LIBDIR)/$(DEMIKERNEL_LIB)

all-shim: all-libs-demikernel
	$(MAKE) -C shim all BINDIR=$(BINDIR)/shim

clean-libs: clean-shim clean-libs-demikernel

clean-libs-demikernel:
	rm -f $(LIBDIR)/$(DEMIKERNEL_LIB)
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

clean-shim:
	$(MAKE) -C shim clean BINDIR=$(BINDIR)/shim

#=======================================================================================================================
# Tests
#=======================================================================================================================

# Builds all tests.
all-tests: all-tests-rust all-tests-c

# Builds all Rust tests.
all-tests-rust:
	@echo "$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)

# Builds all C tests.
all-tests-c: all-libs
	$(MAKE) -C tests all

# Cleans up all build artifactos for tests.
clean-tests: clean-tests-c

# Cleans up all C build artifacts for tests.
clean-tests-c:
	$(MAKE) -C tests clean

#=======================================================================================================================
# Examples
#=======================================================================================================================

# Builds all examples.
all-examples: all-examples-c all-examples-rust

# Builds all C examples.
all-examples-c: all-libs
	$(MAKE) -C examples/c all

# Builds all Rust examples.
all-examples-rust:
	$(MAKE) -C examples/rust all

# Cleans all examples.
clean-examples: clean-examples-c clean-examples-rust

# Cleans all C examples.
clean-examples-c:
	$(MAKE) -C examples/c clean

# Cleans all Rust examples.
clean-examples-rust:
	$(MAKE) -C examples/rust clean

#=======================================================================================================================
# Benchmarks
#=======================================================================================================================

# Builds all C benchmarks
all-benchmarks-c: all-libs
	$(MAKE) -C benchmarks all

# Cleans up all C build artifacts for benchmarks.
clean-benchmarks-c:
	$(MAKE) -C benchmarks clean

#=======================================================================================================================
# Check
#=======================================================================================================================

# Check code style formatting.
check-fmt: check-fmt-c check-fmt-rust

# Check code style formatting for C.
check-fmt-c:
	$(shell find include/ -name "*.h" -name "*.hxx" -name "*.c" -name "*.cpp" -type f -print0 | xargs -0 clang-format --fallback-style=Microsoft --dry-run -Werror )
	@exit $(.SHELLSTATUS)

# Check code style formatting for Rust.
check-fmt-rust:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy $(CARGO_FEATURES) $(CARGO_FLAGS)

#=======================================================================================================================
# Clean
#=======================================================================================================================

# Cleans up all build artifacts.
clean: clean-examples clean-tests clean-libs

#=======================================================================================================================
# Tests
#=======================================================================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export CONFIG_DIR = $(CURDIR)/scripts/config
export ELF_DIR ?= $(CURDIR)/bin/examples/rust
export MTU ?= 9000
export MSS ?= 9000
export PEER ?= server
export TEST ?= udp-push-pop
export TEST_INTEGRATION ?= tcp-test
export TEST_UNIT ?=
export TIMEOUT ?= 120

# Runs system tests.
test-system: test-system-rust

# Rust system tests.
test-system-rust:
	timeout $(TIMEOUT) $(BINDIR)/examples/rust/$(TEST).elf $(ARGS)

# Runs unit tests.
test-unit: test-unit-rust

# C unit tests.
test-unit-c: all-tests test-unit-c-sizes test-unit-c-syscalls

test-unit-c-sizes: all-tests $(BINDIR)/sizes.elf
	timeout $(TIMEOUT) $(BINDIR)/sizes.elf

test-unit-c-syscalls: all-tests $(BINDIR)/syscalls.elf
	timeout $(TIMEOUT) $(BINDIR)/syscalls.elf

# Rust unit tests.
test-unit-rust: test-unit-rust-lib test-unit-rust-udp test-unit-rust-tcp
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big

# Rust unit tests for the library.
test-unit-rust-lib: all-tests-rust
	timeout $(TIMEOUT) $(CARGO) test --lib $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

# Rust unit tests for UDP.
test-unit-rust-udp: all-tests-rust
	timeout $(TIMEOUT) $(CARGO) test --test udp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

# Rust unit tests for TCP.
test-unit-rust-tcp: all-tests-rust
	timeout $(TIMEOUT) $(CARGO) test --test tcp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

# Runs Rust integration tests.
test-integration-rust:
	timeout $(TIMEOUT) $(CARGO) test --test $(TEST_INTEGRATION) $(CARGO_FLAGS) $(CARGO_FEATURES) -- $(ARGS)

# Cleans dangling test resources.
test-clean:
	rm -f /dev/shm/demikernel-*


ENV += CAPY_LOG=all
ENV += LIBOS=catnip
ENV += USE_JUMBO=1



tcp-echo:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/be0_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-echo.elf \
	--peer server --local 10.0.1.9:10000 --bufsize 1024

tcpmig-single-origin:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-server-single.elf \
	10.0.1.8:22222
tcpmig-single-target:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-server-single.elf \
	10.0.1.9:22222

tcpmig-client:
	sudo -E \
	LIBOS=catnap \
	CONFIG_PATH=$(CONFIG_DIR)/node7_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 \
	$(ELF_DIR)/tcpmig-client.elf 10.0.1.8:10000

http-server-fe:
	sudo -E \
	IS_FRONTEND=1 \
	NUM_CORES=4 \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ENV) \
	MIG_PER_N=3 \
	taskset --cpu-list 0 \
	$(ELF_DIR)/http-server.elf 10.0.1.8:10000

fe-tcp-server:
	sudo -E \
	IS_FRONTEND=1 \
	NUM_CORES=4 \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ENV) \
	MIG_AFTER=3 \
	taskset --cpu-list 0 \
	$(ELF_DIR)/tcp-server.elf 10.0.1.8:10000

capybara-switch:
	sudo -E \
	NUM_CORES=1 \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ENV) \
	numactl -m0 taskset --cpu-list 1 \
	$(ELF_DIR)/capybara-switch.elf 10.0.1.8:10000 10.0.1.8:10001

http-server-be0:
	sudo -E \
	NUM_CORES=4 \
	CORE_ID=1 \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ENV) \
	MIG_PER_N=1000000 \
	numactl -m0 taskset --cpu-list 1 \
	$(ELF_DIR)/http-server.elf 10.0.1.9:10000

be-tcp-server0:
	sudo -E \
	NUM_CORES=4 \
	CORE_ID=1 \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ENV) \
	MIG_AFTER=1000000 \
	numactl -m0 taskset --cpu-list 1 \
	$(ELF_DIR)/tcp-server.elf 10.0.1.9:10000

http-server-be1:
	sudo -E \
	NUM_CORES=4 \
	CORE_ID=2 \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ENV) \
	numactl -m0 taskset --cpu-list 2 \
	$(ELF_DIR)/http-server.elf 10.0.1.9:10001

http-server-be2:
	sudo -E  \
	MIG_THRESHOLD=5 \
	CONFIG_PATH=$(CONFIG_DIR)/be2_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 2 \
	$(ELF_DIR)/http-server.elf 10.0.1.9:10002

http-server-be3:
	sudo -E  \
	MIG_THRESHOLD=5 \
	CONFIG_PATH=$(CONFIG_DIR)/be3_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 3 \
	$(ELF_DIR)/http-server.elf 10.0.1.9:10003

tcpmig-multi-origin:
	sudo -E \
	IS_FRONTEND=1 \
	CONFIG_PATH=$(CONFIG_DIR)/fe_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 \
	$(ELF_DIR)/tcpmig-server-multi.elf 10.0.1.8:10000

tcpmig-multi-target0:
	sudo -E CAPYBARA_LOG="tcpmig" \
	MIG_THRESHOLD=10000 \
	CORE_ID=1 \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ELF_DIR)/tcpmig-server-multi.elf 10.0.1.9:10000

tcpmig-multi-target1:
	sudo -E CAPYBARA_LOG="tcpmig" \
	MIG_THRESHOLD=10000 \
	CORE_ID=2 \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ELF_DIR)/tcpmig-server-multi.elf 10.0.1.9:10001

tcpmig-multi-target2:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/be2_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 2 \
	$(ELF_DIR)/tcpmig-server-multi.elf 10.0.1.9:10002

tcpmig-multi-target3:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/be3_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 3 \
	$(ELF_DIR)/tcpmig-server-multi.elf 10.0.1.9:10003

client-dpdk-ctrl:
	sudo -E RUST_LOG="debug" \
	CONFIG_PATH=$(CONFIG_DIR)/client_dpdk_ctrl_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 4 \
	$(ELF_DIR)/dpdk-ctrl.elf

be-dpdk-ctrl:
	sudo -E RUST_LOG="debug" \
	NUM_CORES=4 \
	CORE_ID=5 \
	$(ENV) \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 4 \
	$(ELF_DIR)/dpdk-ctrl.elf

fe-dpdk-ctrl:
	sudo -E RUST_LOG="debug" \
	NUM_CORES=4 \
	CORE_ID=5 \
	$(ENV) \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 4 \
	$(ELF_DIR)/dpdk-ctrl.elf

be-dpdk-ctrl-node9:
	sudo -E RUST_LOG="debug" \
	NUM_CORES=4 \
	CORE_ID=5 \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 4 \
	$(ELF_DIR)/dpdk-ctrl.elf

be-dpdk-ctrl-node8:
	sudo -E RUST_LOG="debug" \
	NUM_CORES=4 \
	CORE_ID=5 \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 4 \
	$(ELF_DIR)/dpdk-ctrl.elf

udp-echo0:
	sudo -E RUST_LOG="debug" NUM_CORES=4 LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/be0_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/udp-echo.elf \
	--local 10.0.1.9:10000

udp-echo1:
	sudo -E RUST_LOG="debug" NUM_CORES=4 LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/be1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/udp-echo.elf \
	--local 10.0.1.9:10001

tcp-migration-client:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/c1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration.elf \
	--client 10.0.1.8:22222
tcp-migration-origin:
	# sudo -E echo $(PKG_CONFIG_PATH)
	# sudo -E echo $(LD_LIBRARY_PATH)
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration.elf \
	--server 10.0.1.8:22222 10.0.1.8:22223 10.0.1.9:22222
tcp-migration-dest:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration.elf \
	--dest 10.0.1.9:22222

# to enable debug logging: RUST_LOG="debug" 
tcp-migration-ping-pong-client:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/c1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration-ping-pong.elf \
	--client 10.0.1.8:22222
tcp-migration-ping-pong-origin:
#	sudo -E echo $(PKG_CONFIG_PATH)
#	sudo -E echo $(LD_LIBRARY_PATH)
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration-ping-pong.elf \
	--server 10.0.1.8:22222 10.0.1.8:22223 10.0.1.9:22222
tcp-migration-ping-pong-dest:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration-ping-pong.elf \
	--dest 10.0.1.9:22222

tcp-ping-pong-server:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-ping-pong.elf \
	--server 10.0.1.8:22222


tcp-pushpop:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	timeout 10 /homes/inho/Capybara/capybara/bin/examples/rust/tcp-push-pop.elf \
	--server 10.0.1.8:22222

state-time-test:
	$(CARGO) test measure_state_time $(CARGO_FEATURES) $(CARGO_FLAGS) --features=tcp-migration,capy-profile,capy-log,capy-time-log -- --nocapture

#============= REDIS =============#

DEMIKERNEL_REPO_DIR ?= $(HOME)/Capybara/capybara
DEMIKERNEL_LOG_IO ?= 0
CONF ?= redis0
NODE ?= 9

redis-server: all-libs
	cd ../capybara-redis && DEMIKERNEL_REPO_DIR=$(DEMIKERNEL_REPO_DIR) DEMIKERNEL_LOG_IO=$(DEMIKERNEL_LOG_IO) make redis-server

redis-server-mig: all-libs
	cd ../capybara-redis && DEMIKERNEL_REPO_DIR=$(DEMIKERNEL_REPO_DIR) DEMIKERNEL_LOG_IO=$(DEMIKERNEL_LOG_IO) DEMIKERNEL_TCPMIG=1 make redis-cli redis-server BUILD_TLS=yes

redis-server-mig-manual: all-libs
	cd ../capybara-redis && DEMIKERNEL_REPO_DIR=$(DEMIKERNEL_REPO_DIR) DEMIKERNEL_LOG_IO=$(DEMIKERNEL_LOG_IO) DEMIKERNEL_TCPMIG=1 MANUAL_TCPMIG=1 make redis-server

run-redis-server:
	cd ../capybara-redis && sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/node$(NODE)_config.yaml LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) numactl -m0 ./src/redis-server $(CONF).conf

run-redis-server-node8:
	cd ../capybara-redis && sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml CAPY_LOG=all RUST_LOG="debug" NUM_CORES=4 CORE_ID=1 LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) numactl -m0 ./src/redis-server config/node8_10001.conf

run-test:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml CAPY_LOG=all RUST_LOG="debug" NUM_CORES=4 CORE_ID=1 LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) numactl -m0 $(ELF_DIR)/http-server.elf 10.0.1.8:10000

clean-redis:
	cd ../capybara-redis && make distclean

clean-redis-data:
	cd ../capybara-redis && sudo rm -rf dir_master dir_slave


# REDIS + TLS #

redis-server-node8:
	cd ../capybara-redis/src && \
	sudo -E \
	$(ENV) \
	CORE_ID=1 \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	LD_PRELOAD=$(LIBDIR)/libshim.so \
	./redis-server ../config/node8.conf


redis-server-node9:
	cd ../cr/src && \
	sudo -E \
	$(ENV) \
	CORE_ID=1 \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	LD_PRELOAD=$(LIBDIR)/libshim.so \
	./redis-server ../config/node9.conf

tlse:
	$(MAKE) -C shim run-tlse BINDIR=$(BINDIR)/shim