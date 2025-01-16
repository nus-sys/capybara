#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/autokernel-data/$1

cat $exptid.server_reply | sort -t, -k2,2n | awk -v exptid="$exptid" -F, '                              
{
  print > exptid".server_reply_"$4
}'

cat $exptid.server_reply_10000 | awk -F, '
BEGIN { first_val = 0; }
NR == 1 { first_val = $3; }
{
  tstamp = $3 - first_val;  
  group = int(tstamp / 1000000);
  num_conn[group] = $7;
}
END {
  for (g in num_conn) {
    print g","num_conn[g];
  }
}' > $exptid.server_ms_num_conn_10000


cat $exptid.server_reply_10001 | awk -F, '
BEGIN { first_val = 0; }
NR == 1 { first_val = $3; }
{
  tstamp = $3 - first_val;  
  group = int(tstamp / 1000000);
  num_conn[group] = $7;
}
END {
  for (g in num_conn) {
    print g","num_conn[g];
  }
}' > $exptid.server_ms_num_conn_10001



cat $exptid.server_reply_10002 | awk -F, '
BEGIN { first_val = 0; }
NR == 1 { first_val = $3; }
{
  tstamp = $3 - first_val;  
  group = int(tstamp / 1000000);
  num_conn[group] = $7;
}
END {
  for (g in num_conn) {
    print g","num_conn[g];
  }
}' > $exptid.server_ms_num_conn_10002


cat $exptid.server_reply_10003 | awk -F, '
BEGIN { first_val = 0; }
NR == 1 { first_val = $3; }
{
  tstamp = $3 - first_val;  
  group = int(tstamp / 1000000);
  num_conn[group] = $7;
}
END {
  for (g in num_conn) {
    print g","num_conn[g];
  }
}' > $exptid.server_ms_num_conn_10003



awk -F, '$4 != 18245' $exptid.server_reply | sort -t, -k2,2n > $exptid.server_reply_sorted


cat $exptid.server_reply_sorted | awk -F, '
BEGIN { first_val = 0; }
NR == 1 { first_val = $3 + $8; }
{
  tstamp = $3 + $8 - first_val;
  group = int(tstamp / 1000000);
  count[group]++;
  sum[group] += $5;
  values[group] = values[group] " " $5;
}
END {
  for (g in count) {
    avg = sum[g] / count[g];
    n = split(values[g], arr, " ");
    asort(arr);
    idx99 = n * 0.99;
    ceil_val = idx99 == int(idx99) ? idx99 : int(idx99) + (idx99 > 0);
    if (idx99 < 1) idx99 = 1;
    if (idx99 > n) idx99 = n;
    percentile99 = arr[ceil_val];
    print g","avg","percentile99;
  }
}' > $exptid.server_ms_avg_99p_lat
