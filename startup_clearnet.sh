#!/bin/bash
set -x
set -e

cd /app && unbuffer ./target/release/tostr --clearnet | tee -a data/log
