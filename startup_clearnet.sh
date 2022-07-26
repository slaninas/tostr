#!/bin/bash
set -x
set -e

cd /app && ./target/release/tostr --clearnet
