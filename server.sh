if [ "$1" = "8a" ]
then make be-dpdk-ctrl-node8
fi
if [ "$1" = "9a" ]
then make be-dpdk-ctrl-node9
fi
if [ "$1" = "8b" ]
then sudo -E CAPY_LOG=all LIBOS=catnip MTU=1500 MSS=1500 NUM_CORES=4 RUST_BACKTRACE=full CORE_ID=1 CONFIG_PATH=/homes/nimishw/Capybara/config/node8_config.yaml LD_LIBRARY_PATH=/homes/nimishw/lib:/homes/nimishw/lib/x86_64-linux-gnu PKG_CONFIG_PATH=/homes/nimishw/lib/x86_64-linux-gnu/pkgconfig ./bin/examples/rust/prism-fe.elf 10.0.1.8:10000
fi
if [ "$1" = "9b" ]
then sudo -E CAPY_LOG=all LIBOS=catnip MTU=1500 MSS=1500 NUM_CORES=4 RUST_BACKTRACE=full CORE_ID=2 CONFIG_PATH=/homes/nimishw/Capybara/config/node9_config.yaml LD_LIBRARY_PATH=/homes/nimishw/lib:/homes/nimishw/lib/x86_64-linux-gnu PKG_CONFIG_PATH=/homes/nimishw/lib/x86_64-linux-gnu/pkgconfig ./bin/examples/rust/prism-be-http.elf 10.0.1.9:10001 10.0.1.8:10000
fi