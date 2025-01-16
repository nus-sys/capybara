#!/bin/bash

if [ $# -ne 1 ];then
	echo "Usage: $0 [experiment_id]"
	exit 1
fi

exptid=/homes/inho/autokernel-data/$1

cat $exptid.be0 | grep PROF | tail -200000 | awk -F, 'NR%2{val=$2; next} {sum=val+$2; print val "," $2 "," sum}' | awk -v exptid="$exptid" -F, '
{
    for (i = 1; i <= NF; i++) {
        values[i] = values[i] " " $i;
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
    
    for (g = 1; g <= cols; g++) {
        n = split(values[g], arr, " ");
        asort(arr);
        idx99 = n * 0.99;
        ceil_val = idx99 == int(idx99) ? idx99 : int(idx99) + (idx99 > 0)
        if (idx99 < 1) idx99 = 1;
        if (idx99 > n) idx99 = n;
        percentile99 = arr[ceil_val];
        if (g >1) printf ","
        printf "%d", percentile99;
    }
    printf "\n";

    # Calculating and printing the standard deviation for each column
    std_dev = sqrt(sumsq[1]/NR - (sum[1]/NR)^2);
    printf "%d", std_dev;
    for (i = 2; i <= cols; i++) {
        std_dev = sqrt(sumsq[i]/NR - (sum[i]/NR)^2);
        printf ",%d", std_dev;
    }
    printf "\n";
    
}' > $exptid.mig_cpu_ovhd_avg_min_max_99p_stddev

cat $exptid.be0 | grep PROF_EXPORT | awk -F, '{print $2}' | tail -100000 | sort -n | uniq --count | awk '
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
}' > $exptid.export_cdf


cat $exptid.be0 | grep PROF_IMPORT | awk -F, '{print $2}' | tail -100000 | sort -n | uniq --count | awk '
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
}' > $exptid.import_cdf