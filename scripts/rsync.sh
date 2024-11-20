#!/bin/bash -ex
HOSTS="nsl-node8.d2 nsl-node9.d2"
DIR=/local/$USER

for HOST in $HOSTS
do
    ssh $HOST mkdir -p $DIR/capybara
    rsync -a bin lib scripts Makefile linux.mk shim $HOST:$DIR/capybara/
    ssh $HOST mkdir -p $DIR/capybara-redis/src
    rsync -a ../capybara-redis/config $HOST:$DIR/capybara-redis/
    rsync -a ../capybara-redis/src/redis-server $HOST:$DIR/capybara-redis/src/
done