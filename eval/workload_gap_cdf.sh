#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/capybara-data/$1

paste -d, $exptid.be0_rps_signal $exptid.be1_rps_signal | awk -F, '{print ($3 - $6) < 0 ? -($3 - $6) : ($3 - $6)}' | sort -n | uniq --count | awk '
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
}' > $exptid.workload_gap_cdf

paste -d, $exptid.be0_rps_signal $exptid.be1_rps_signal | awk -F, '
{
  diff = ($3 - $6) < 0 ? -($3 - $6) : ($3 - $6); 
  sum += diff; 
  count++
} 
END {
  print sum/count
}' > $exptid.workload_gap_avg
