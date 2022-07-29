#!/bin/bash
set -x
set -e

# Block all non-tor traffic, see
iptables -F OUTPUT
iptables -A OUTPUT -j ACCEPT -m owner --uid-owner debian-tor
iptables -A OUTPUT -j ACCEPT -o lo
# iptables -A OUTPUT -j ACCEPT -p udp --dport 123
iptables -P OUTPUT DROP
# iptables -L -v

ip6tables -F OUTPUT
ip6tables -A OUTPUT -j ACCEPT -m owner --uid-owner debian-tor
ip6tables -A OUTPUT -j ACCEPT -o lo
ip6tables -P OUTPUT DROP


service tor start
service tor status

cd /app && unbuffer ./target/release/tostr --tor | tee data/log
