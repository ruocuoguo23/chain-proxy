#!/bin/bash

# Set the target directory
TARGET_DIR="bin"

# Create the target directory if it doesn't exist
mkdir -p $TARGET_DIR

# Execute cargo build --release
echo "Building the project in release mode..."
cargo build --release

# Check if the build was successful
# shellcheck disable=SC2181
if [ $? -eq 0 ]; then
    echo "Build successful."

    # Set the project name
    PROJECT_NAME="chain-proxy"

    # Set the path to the release binary
    RELEASE_BINARY="target/release/$PROJECT_NAME"

    # Check if the binary file exists
    if [ -f "$RELEASE_BINARY" ]; then
        echo "Copying binary to $TARGET_DIR..."
        mv "$RELEASE_BINARY" $TARGET_DIR/
        echo "Binary copied to $TARGET_DIR."
    else
        echo "Error: Binary file not found."
        exit 1
    fi
else
    echo "Build failed."
    exit 1
fi
