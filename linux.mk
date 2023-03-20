# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================
export HOME := /homes/inho
export PREFIX ?= $(HOME)
export INSTALL_PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH ?= $(HOME)/lib:$(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')

#=======================================================================================================================
# Build Configuration
#=======================================================================================================================

export BUILD := release
ifeq ($(DEBUG),yes)
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


server-test:
	CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml timeout 30 /homes/inho/Capybara/capybara/bin/examples/rust/tcp-push-pop.elf --server 20.0.0.2:22222

http-server:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/http-server.elf 

tcp-echo:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-echo.elf \
	--peer server --local 10.0.1.8:22222 --bufsize 1024

tcpmig-client:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/c1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-client.elf \
	10.0.1.8:22222
tcpmig-single-origin:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-server-single.elf \
	10.0.1.8:22222
tcpmig-single-target:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-server-single.elf \
	10.0.1.9:22222

tcpmig-multi-origin:
	sudo -E RX_TX_RATIO=10 LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-server-multi.elf \
	10.0.1.8:22222
tcpmig-multi-target1:
	sudo -E PORT_ID=1 RUST_LOG="debug" NUM_CORES=4 RX_TX_RATIO=10 LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/be1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-server-multi.elf \
	10.0.1.9:10001
tcpmig-multi-target2:
	sudo -E PORT_ID=1 RUST_LOG="debug" NUM_CORES=4 RX_TX_RATIO=10 LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/be2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcpmig-server-multi.elf \
	10.0.1.9:10002

udp-echo1:
	sudo -E RUST_LOG="debug" NUM_CORES=4 LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/be1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/udp-echo.elf \
	--local 10.0.1.9:10001

udp-echo2:
	sudo -E RUST_LOG="debug" NUM_CORES=4 LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/be2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/udp-echo.elf \
	--local 10.0.1.9:10002

dpdk-ctrl:
	sudo -E PORT_ID=1 RUST_LOG="debug" NUM_CORES=4 LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/dpdk-ctrl.elf

tcp-migration-client:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/c1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration.elf \
	--client 10.0.1.8:22222
tcp-migration-origin:
	# sudo -E echo $(PKG_CONFIG_PATH)
	# sudo -E echo $(LD_LIBRARY_PATH)
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration.elf \
	--server 10.0.1.8:22222 10.0.1.8:22223 10.0.1.9:22222
tcp-migration-dest:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration.elf \
	--dest 10.0.1.9:22222

# to enable debug logging: RUST_LOG="debug" 
tcp-migration-ping-pong-client:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/c1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration-ping-pong.elf \
	--client 10.0.1.8:22222
tcp-migration-ping-pong-origin:
#	sudo -E echo $(PKG_CONFIG_PATH)
#	sudo -E echo $(LD_LIBRARY_PATH)
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration-ping-pong.elf \
	--server 10.0.1.8:22222 10.0.1.8:22223 10.0.1.9:22222
tcp-migration-ping-pong-dest:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s2_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-migration-ping-pong.elf \
	--dest 10.0.1.9:22222

tcp-ping-pong-server:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	/homes/inho/Capybara/capybara/bin/examples/rust/tcp-ping-pong.elf \
	--server 10.0.1.8:22222

tcp-ping-pong-client:
	sudo -E RUST_BACKTRACE=1 LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/c1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	timeout 10 /homes/inho/Capybara/capybara/bin/examples/rust/tcp-ping-pong.elf \
	--client 10.0.1.8:22222

tcp-pushpop:
	sudo -E LIBOS=catnip CONFIG_PATH=/homes/inho/Capybara/config/s1_config.yaml \
	PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
	LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
	timeout 10 /homes/inho/Capybara/capybara/bin/examples/rust/tcp-push-pop.elf \
	--server 10.0.1.8:22222