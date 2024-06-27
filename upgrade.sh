#!/bin/bash

# process name
SERVICE_NAME="chain-proxy"

# new instance path, current path is ./bin/chain-proxy
NEW_INSTANCE_PATH="./bin/chain-proxy"

# check if the new instance exists
OLD_PID=$(pgrep -f "$SERVICE_NAME")

if [ -z "$OLD_PID" ]; then
  echo "no instance running, upgrade failed"
  exit 1
fi

echo "old instance PID: $OLD_PID"

CONFIG_PATH="./config/testing.yaml"

# start new instance, using --upgrade to upgrade
echo "start new instance..."
$NEW_INSTANCE_PATH --upgrade --config "$CONFIG_PATH" &

# wait for new instance to start
sleep 2

# send SIGQUIT signal to old instance
echo "send SIGQUIT signal to old instance..."
kill -SIGQUIT "$OLD_PID"

echo "upgrade success!"
