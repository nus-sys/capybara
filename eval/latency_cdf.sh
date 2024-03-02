#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/capybara-data/$1

cat $exptid.latency_trace | awk -F',' '{print $3}' | sort -n | uniq --count | awk '
BEGIN {
  sum = 0
}
{
  sum=sum+$1;
  records[NR,0]=$2;
  records[NR,1]=$1;
  records[NR,2]=sum;
}
END {
  for(i=1;i<=NR;i++){
    printf "%d,%.8f\n", records[i,0], records[i,2]/sum
  }
}' > $exptid.lat_cdf