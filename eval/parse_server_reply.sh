#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/capybara-data/$1

cat $exptid.server_reply | sort -t, -k2,2n | awk -v exptid="$exptid" -F, '                              
{
  print > exptid".server_reply_"$3
}'