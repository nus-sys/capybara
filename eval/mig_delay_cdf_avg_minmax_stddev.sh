#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/autokernel-data/$1

input_file=$exptid.mig_delay
num_columns=$(head -1 $input_file | awk -F, '{print NF}')

# Loop through each column
for ((col=1; col<=$num_columns; col++)); do
    # Extract the column and sort it
    # cut -d, -f$col $input_file | sort -n | tail -n +11 | head -n -10 | uniq --count | awk '
	cut -d, -f$col $input_file | sort -n | uniq --count | awk '
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
    }' > $exptid.mig_delay_cdf_step${col}
done

cat $input_file | awk -F, '
{
    for (i = 1; i <= NF; i++) {
        sum[i] += $i;
        sumsq[i] += ($i)^2;  # Sum of squares for std dev calculation
        if (NR == 1 || $i < min[i] || min[i] == "")
            min[i] = $i;
        if (NR == 1 || $i > max[i])
            max[i] = $i;
    }
    if (NR == 1) cols = NF;
}
END {
    printf "%d", sum[1]/NR;
    for (i = 2; i <= cols; i++)
        printf ",%d", sum[i]/NR;
    printf "\n";
    
    printf "%d", min[1];
    for (i = 2; i <= cols; i++)
        printf ",%d", min[i];
    printf "\n";
    
    printf "%d", max[1];
    for (i = 2; i <= cols; i++)
        printf ",%d", max[i];
    printf "\n";
    
    # Calculating and printing the standard deviation for each column
    std_dev = sqrt(sumsq[1]/NR - (sum[1]/NR)^2);
    printf "%d", std_dev;
    for (i = 2; i <= cols; i++) {
        std_dev = sqrt(sumsq[i]/NR - (sum[i]/NR)^2);
        printf ",%d", std_dev;
    }
    printf "\n";
}' > $exptid.mig_delay_avg_minmax_stddev
