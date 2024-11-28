from common import check_hostname, run_shell


check_hostname("nsl-node7")
run_shell(
    "./bin/redis-cli --tls --cert /usr/local/tls/svr.crt --key /usr/local/tls/svr.key --cacert /usr/local/tls/CA.pem -h 10.0.1.8 -p 10000"
    # "./bin/redis-cli -h 10.0.1.8 -p 10000"
)
