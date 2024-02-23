#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/capybara-data/$1

input_file=$exptid.mig_latency
num_columns=$(head -1 $input_file | awk -F, '{print NF}')

# Loop through each column
for ((col=1; col<=$num_columns; col++)); do
    # Extract the column and sort it
    cut -d, -f$col $input_file | sort -n | tail -n +11 | head -n -10 | uniq --count | awk '
    BEGIN {sum = 0}
    {
    	sum=sum+$1; 
    	records[NR,0]=$2;
    	records[NR,1]=$1;
    	records[NR,2]=sum;
    }
    END{
    	for(i=1; i<=NR; i++) {
    		printf "%d,%.8f\n", records[i,0], records[i,2]/sum
    	}
    }' > $exptid.mig_latency_cdf_step${col}
done

