if [ "$1" = "1" ]
then make be-dpdk-ctrl
fi
if [ "$1" = "2" ]
then sudo -E LIBOS=catnip MTU=1500 MSS=1500 NUM_CORES=4 RUST_BACKTRACE=full CORE_ID=1 CONFIG_PATH=/homes/nimishw/Capybara/config/node9_config.yaml LD_LIBRARY_PATH=/homes/nimishw/lib:/homes/nimishw/lib/x86_64-linux-gnu PKG_CONFIG_PATH=/homes/nimishw/lib/x86_64-linux-gnu/pkgconfig CAPYBARA_LOG=all ./bin/examples/rust/http-server.elf 10.0.1.9:10000
fi
