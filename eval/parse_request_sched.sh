#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/capybara-data/$1

cat $exptid.request_sched | sort -t, -k1,1n |  awk -F, '
BEGIN { first_val = 0; }
NR == 1 { first_val = $1; }                                         
{
  tstamp = $1 - first_val;
  group = int(tstamp / 1000000);
  count[group]++;
}
END {
  for (g in count) {
    print g","count[g];
  }
}' > $exptid.sched_ms_req