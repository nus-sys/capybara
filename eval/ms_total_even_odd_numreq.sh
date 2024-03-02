#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/capybara-data/$1

cat $exptid.latency_trace | awk -F, '                                              
{
  group = int($1 / 1000000);
  even = $4 % 2 == 0 ? "even" : "odd";
  count[group]++;
  sum[group]++;
  count_even_odd[group, even]++;
}
END {
  for (g in count) {
    print g","sum[g]","count_even_odd[g, "even"]+0","count_even_odd[g, "odd"]+0;
  }
}' > $exptid.ms_total_even_odd_numreq