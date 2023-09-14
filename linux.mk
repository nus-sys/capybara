# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================
export PREFIX := $(HOME)
export INSTALL_PREFIX := $(HOME)
export PKG_CONFIG_PATH := $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH := $(HOME)/lib:$(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')

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
export INCDIR ?= $(CURDIR)/include
export SRCDIR = $(CURDIR)/src
export BUILD_DIR := $(CURDIR)/target/release
ifeq ($(BUILD),dev)
export BUILD_DIR := $(CURDIR)/target/debug
endif

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Rust
export CARGO ?= $(shell which cargo || echo "$(HOME)/.cargo/bin/cargo" )
export CARGO_FLAGS += --profile $(BUILD)

#=======================================================================================================================
# Libraries
#=======================================================================================================================

export DEMIKERNEL_LIB := $(BUILD_DIR)/libdemikernel.so
export LIBS := $(DEMIKERNEL_LIB)

#=======================================================================================================================
# Build Parameters
#=======================================================================================================================

export LIBOS ?= catnip
export CARGO_FEATURES := --features=$(LIBOS)-libos

# Switch for DPDK
ifeq ($(LIBOS),catnip)
DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5]"`" ] && echo mlx5 || echo mlx4)
CARGO_FEATURES += --features=$(DRIVER)
endif

# Switch for profiler.
export PROFILER=no
ifeq ($(PROFILER),yes)
CARGO_FEATURES += --features=profiler
endif

CARGO_FEATURES += $(FEATURES)

#=======================================================================================================================

all: all-libs all-tests all-examples

# Builds documentation.
doc:
	$(CARGO) doc $(FLAGS) --no-deps

# Copies demikernel artifacts to a INSTALL_PREFIX directory.
install:
	mkdir -p $(INSTALL_PREFIX)/include $(INSTALL_PREFIX)/lib
	cp -rf $(INCDIR)/* $(INSTALL_PREFIX)/include/
	cp -f  $(DEMIKERNEL_LIB) $(INSTALL_PREFIX)/lib/

#=======================================================================================================================
# Libs
#=======================================================================================================================

# Builds all libraries.
all-libs:
	@echo "LD_LIBRARY_PATH: $(LD_LIBRARY_PATH)"
	@echo "PKG_CONFIG_PATH: $(PKG_CONFIG_PATH)"
	@echo "$(CARGO) build --libs $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS)

#=======================================================================================================================
# Tests
#=======================================================================================================================

# Builds all tests.
all-tests: all-tests-rust all-tests-c

# Builds all Rust tests.
all-tests-rust: all-libs
	@echo "$(CARGO) build  --tests $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build  --tests $(CARGO_FEATURES) $(CARGO_FLAGS)

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
all-examples-c:
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

#=======================================================================================================================
# Clean
#=======================================================================================================================

# Cleans up all build artifacts.
clean: clean-examples clean-tests
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

#=======================================================================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export MTU ?= 1500
export MSS ?= 1500
export PEER ?= server
export TEST ?= udp_push_pop
export TIMEOUT ?= 30

#=======================================================================================================================
# Capybara Environment Variables
#=======================================================================================================================
export CONFIG_DIR = $(HOME)/Capybara/capybara/config
export ELF_DIR ?= $(HOME)/Capybara/capybara/bin/examples/rust

# Runs system tests.
test-system: test-system-rust

# Rust system tests.
test-system-rust:
	timeout $(TIMEOUT) $(BINDIR)/examples/rust/$(TEST).elf $(ARGS)

# Runs unit tests.
test-unit: test-unit-rust

# C unit tests.
test-unit-c: $(BINDIR)/syscalls.elf
	$(BINDIR)/syscalls.elf

# Rust unit tests.
test-unit-rust:
# 	$(CARGO) test --lib $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(UNIT_TEST)
	$(CARGO) test tcp_migration --lib $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(UNIT_TEST)



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
	CONFIG_PATH=$(CONFIG_DIR)/fe_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 \
	$(ELF_DIR)/http-server.elf 10.0.1.8:10000

http-server-be0:
	sudo -E CAPYBARA_LOG="tcpmig" RUST_BACKTRACE=full \
	CORE_ID=1 \
	MIG_DELAY=0 \
	RECV_QUEUE_LEN=0 \
	CONFIG_PATH=$(CONFIG_DIR)/be0_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	$(ELF_DIR)/http-server.elf 10.0.1.9:10000

http-server-be1:
	sudo -E CAPYBARA_LOG="tcpmig" \
	CORE_ID=2 \
	MIG_DELAY=0 \
	RECV_QUEUE_LEN=3000 \
	CONFIG_PATH=$(CONFIG_DIR)/be1_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
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
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 4 \
	$(ELF_DIR)/dpdk-ctrl.elf

fe-dpdk-ctrl:
	sudo -E RUST_LOG="debug" \
	CONFIG_PATH=$(CONFIG_DIR)/fe_dpdk_ctrl_config.yaml \
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

tcp-ping-pong-client:
	sudo -E RUST_BACKTRACE=1 LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/c1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	timeout 10 /homes/inho/Capybara/capybara/bin/examples/rust/tcp-ping-pong.elf \
	--client 10.0.1.8:22222

tcp-pushpop:
	sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	timeout 10 /homes/inho/Capybara/capybara/bin/examples/rust/tcp-push-pop.elf \
	--server 10.0.1.8:22222


#============= REDIS =============#

DEMIKERNEL_REPO_DIR ?= $(HOME)/Capybara/capybara
DEMIKERNEL_LOG_IO ?= 0
REDIS_CONF ?= redis.conf

all-libs-mig:
	@echo "LD_LIBRARY_PATH: $(LD_LIBRARY_PATH)"
	@echo "PKG_CONFIG_PATH: $(PKG_CONFIG_PATH)"
	@echo "$(CARGO) build --libs $(CARGO_FEATURES) $(CARGO_FLAGS) --features=tcp-migration"
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS) --features=tcp-migration

redis-server: all-libs
	cd ../capybara-redis && DEMIKERNEL_REPO_DIR=$(DEMIKERNEL_REPO_DIR) DEMIKERNEL_LOG_IO=$(DEMIKERNEL_LOG_IO) make redis-server

redis-server-mig: all-libs-mig
	cd ../capybara-redis && DEMIKERNEL_REPO_DIR=$(DEMIKERNEL_REPO_DIR) DEMIKERNEL_LOG_IO=$(DEMIKERNEL_LOG_IO) DEMIKERNEL_TCPMIG=1 make redis-server

run-redis-server:
	cd ../capybara-redis && sudo -E LIBOS=catnip CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) ./src/redis-server $(REDIS_CONF)

clean-redis:
	cd ../capybara-redis && make distclean

clean-redis-data:
	cd ../capybara-redis && sudo rm -rf dir_master dir_slave