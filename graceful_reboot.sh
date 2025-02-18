#!/bin/bash

FPID=/data/system/run/chain-proxy.pid
ROOTPATH=/data/phemex/chain-proxy
NEW_INSTANCE_PATH="$ROOTPATH/artifacts/$(ls $ROOTPATH/artifacts | grep -vE 'metadata|reader')"
CONFIG_PATH="$ROOTPATH/conf/chain-proxy.yaml"

# 获取当前运行的进程 PID
if [ -f "$FPID" ]; then
    OLD_PID=$(cat "$FPID")
    if [ -d "/proc/$OLD_PID" ]; then
        echo "Old instance PID: $OLD_PID"
    else
        echo "No running instance found, starting a new one..."
        OLD_PID=""
    fi
else
    echo "No PID file found, starting a new instance..."
    OLD_PID=""
fi

# 启动新实例
echo "Starting new instance..."
cd "$ROOTPATH"
nohup $NEW_INSTANCE_PATH --upgrade --config "$CONFIG_PATH" > "$ROOTPATH/nohup.out" 2>&1 &
NEW_PID=$!
echo "$NEW_PID" > "$FPID"
echo "New instance PID: $NEW_PID"

# 等待新实例启动
sleep 2

# 终止旧实例
if [ -n "$OLD_PID" ]; then
    echo "Sending SIGQUIT signal to old instance (PID: $OLD_PID)..."
    kill -SIGQUIT "$OLD_PID"
    echo "Old instance gracefully stopped."
fi

echo "Upgrade success!"
