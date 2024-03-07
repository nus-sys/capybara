#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/capybara-data/$1

cat $exptid.latency_trace | awk -F, '                              
{
  group = int($2 / 1000000);
  count[group]++;
  sum[group] += $3;
  values[group] = values[group] " " $3;
}
END {
  for (g in count) {
    avg = sum[g] / count[g];
    n = split(values[g], arr, " ");
    asort(arr);
    idx99 = n * 0.99;
    ceil_val = idx99 == int(idx99) ? idx99 : int(idx99) + (idx99 > 0)
    if (idx99 < 1) idx99 = 1;
    if (idx99 > n) idx99 = n;
    percentile99 = arr[ceil_val];
    print g","avg","percentile99;
  }
}' | sort -t, -k1,1n > $exptid.ms_avg_99p_lat