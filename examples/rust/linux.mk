# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Suffix for executable files.
export EXEC_SUFFIX := elf

all: all-examples
	mkdir -p $(BINDIR)/examples/rust
# 	cp -f $(BUILD_DIR)/examples/udp-dump  $(BINDIR)/examples/rust/udp-dump.$(EXEC_SUFFIX)
# 	cp -f $(BUILD_DIR)/examples/udp-echo  $(BINDIR)/examples/rust/udp-echo.$(EXEC_SUFFIX)
# 	cp -f $(BUILD_DIR)/examples/udp-pktgen  $(BINDIR)/examples/rust/udp-pktgen.$(EXEC_SUFFIX)
# 	cp -f $(BUILD_DIR)/examples/udp-relay  $(BINDIR)/examples/rust/udp-relay.$(EXEC_SUFFIX)
# 	cp -f $(BUILD_DIR)/examples/udp-push-pop  $(BINDIR)/examples/rust/udp-push-pop.$(EXEC_SUFFIX)
# 	cp -f $(BUILD_DIR)/examples/udp-ping-pong $(BINDIR)/examples/rust/udp-ping-pong.$(EXEC_SUFFIX)
# 	cp -f $(BUILD_DIR)/examples/tcp-dump  $(BINDIR)/examples/rust/tcp-dump.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-echo  $(BINDIR)/examples/rust/tcp-echo.$(EXEC_SUFFIX)
# 	cp -f $(BUILD_DIR)/examples/tcp-pktgen  $(BINDIR)/examples/rust/tcp-pktgen.$(EXEC_SUFFIX)
#	cp -f $(BUILD_DIR)/examples/tcp-push-pop  $(BINDIR)/examples/rust/tcp-push-pop.$(EXEC_SUFFIX)
#	cp -f $(BUILD_DIR)/examples/tcp-ping-pong $(BINDIR)/examples/rust/tcp-ping-pong.$(EXEC_SUFFIX)
#	cp -f $(BUILD_DIR)/examples/tcp-migration $(BINDIR)/examples/rust/tcp-migration.$(EXEC_SUFFIX)
#	cp -f $(BUILD_DIR)/examples/tcp-migration-ping-pong $(BINDIR)/examples/rust/tcp-migration-ping-pong.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcpmig-client $(BINDIR)/examples/rust/tcpmig-client.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcpmig-server-multi $(BINDIR)/examples/rust/tcpmig-server-multi.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcpmig-server-single $(BINDIR)/examples/rust/tcpmig-server-single.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/dpdk-ctrl $(BINDIR)/examples/rust/dpdk-ctrl.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/http-server $(BINDIR)/examples/rust/http-server.$(EXEC_SUFFIX)

all-examples:
#	@echo "$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)"
#	$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)
#	@echo "$(CARGO) build --example tcp-migration-ping-pong $(CARGO_FEATURES) $(CARGO_FLAGS)"
#	$(CARGO) build --example tcp-migration-ping-pong $(CARGO_FEATURES) $(CARGO_FLAGS)
	@echo "$(CARGO) build --example tcpmig-server-multi $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --example tcpmig-server-multi $(CARGO_FEATURES) $(CARGO_FLAGS) --features=tcp-migration
	@echo "$(CARGO) build --example tcpmig-server-single $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --example tcpmig-server-single $(CARGO_FEATURES) $(CARGO_FLAGS) --features=tcp-migration
	@echo "$(CARGO) build --example tcpmig-client $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --example tcpmig-client $(CARGO_FEATURES) $(CARGO_FLAGS)
	@echo "$(CARGO) build --example dpdk-ctrl $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --example dpdk-ctrl $(CARGO_FEATURES) $(CARGO_FLAGS)
#	@echo "$(CARGO) build --example tcp-echo $(CARGO_FEATURES) $(CARGO_FLAGS)"
#	$(CARGO) build --example tcp-echo $(CARGO_FEATURES) $(CARGO_FLAGS)
	@echo "$(CARGO) build --example http-server $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --example http-server $(CARGO_FEATURES) $(CARGO_FLAGS)
		


clean:
# 	@rm -rf $(BINDIR)/examples/rust/udp-dump.$(EXEC_SUFFIX)
# 	@rm -rf $(BINDIR)/examples/rust/udp-echo.$(EXEC_SUFFIX)
# 	@rm -rf $(BINDIR)/examples/rust/udp-pktgen.$(EXEC_SUFFIX)
# 	@rm -rf $(BINDIR)/examples/rust/udp-relay.$(EXEC_SUFFIX)
# 	@rm -rf $(BINDIR)/examples/rust/udp-push-pop.$(EXEC_SUFFIX)
# 	@rm -rf $(BINDIR)/examples/rust/udp-ping-pong.$(EXEC_SUFFIX)
# 	@rm -rf $(BINDIR)/examples/rust/tcp-dump.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-echo.$(EXEC_SUFFIX)
# 	@rm -rf $(BINDIR)/examples/rust/tcp-pktgen.$(EXEC_SUFFIX)
#	@rm -rf $(BINDIR)/examples/rust/tcp-push-pop.$(EXEC_SUFFIX)
#	@rm -rf $(BINDIR)/examples/rust/tcp-ping-pong.$(EXEC_SUFFIX)
#	@rm -rf $(BINDIR)/examples/rust/tcp-migration.$(EXEC_SUFFIX)
#	@rm -rf $(BINDIR)/examples/rust/tcp-migration-ping-pong.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcpmig-client.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcpmig-server-multi.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcpmig-server-client.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/dpdk-ctrl.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/http-server.$(EXEC_SUFFIX)
	
	
