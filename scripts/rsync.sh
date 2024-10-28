#!/bin/bash -ex
HOST=nsl-node8.d2
DIR=/local/$USER

ssh $HOST mkdir -p $DIR/capybara
rsync -a bin lib scripts Makefile linux.mk $HOST:/local/$USER/capybara/
ssh $HOST mkdir -p $DIR/capybara-redis/src
rsync -a ../capybara-redis/config $HOST:/local/$USER/capybara-redis/
rsync -a ../capybara-redis/src/redis-server $HOST:/local/$USER/capybara-redis/src/
rsync -a $HOME/lib $HOST:$HOME/lib