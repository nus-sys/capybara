#!/bin/bash

cecho(){  # Credit: https://stackoverflow.com/a/53463162/2886168
    RED="\033[0;31m"
    GREEN="\033[0;32m"
    YELLOW="\033[0;33m"
    # ... ADD MORE COLORS
    NC="\033[0m" # No Color

    printf "${!1}${2} ${NC}\n"
}

# Check if expt name and number is passed as an argument
if [ $# -ne 1 ];then
    cecho "YELLOW" "Usage: $0 <pcap file>"
    exit 1
fi


pcap_file=$1
outfile=${pcap_file%.*}".csv"

printf "pktno\ttimestamp\tip.src\tip.dst\ttcp.srcport\ttcp.dstport\ttcp.hdr_len\ttcp.len\ttcp.seq_raw\ttcp.ack_raw\ttcp.flags.ack\ttcp.flags.push\ttcp.flags.reset\ttcp.flags.syn\ttcp.flags.fin\ttcp.options.sack_le\ttcp.options.sack_re\thttp.request.method\thttp.response.phrase\n" > $outfile

cecho "YELLOW" "Parsing $pcap_file --> $outfile ..."

tshark -r $pcap_file -t r -Tfields -e eth.src -e ip.src -e ip.dst -e tcp.srcport -e tcp.dstport -e tcp.hdr_len -e tcp.len -e tcp.seq_raw -e tcp.ack_raw -e tcp.flags.ack -e tcp.flags.push -e tcp.flags.reset -e tcp.flags.syn -e tcp.flags.fin  -e tcp.options.sack_le -e tcp.options.sack_re -e http.request.method -e http.response.phrase -E separator=/t tcp | awk 'BEGIN {OFS="\t";} {gsub(/:/, "", $1); printf("%s\t",strtonum("0x"$1)); for(i=2;i<=NF;i++){ printf("%s",( (i>2) ? OFS : "" ) $i) } ; printf("\n") ;}' | sort -n -k 1 | awk '{printf("%d\t",NR); print;}' >> $outfile

cecho "GREEN" "Done"
