#!/bin/sh
# busybox udhcpc's event script in the bash sandbox's VM: the NAT's lease
# becomes eth0's address, the default route and /etc/resolv.conf (which
# pasta reads to forward the networked box's DNS address to).
case "$1" in
  deconfig)
    ip -4 addr flush dev "$interface"
    ;;
  bound | renew)
    ip -4 addr flush dev "$interface"
    ip -4 addr add "$ip/$mask" dev "$interface"
    [ -z "$router" ] || ip -4 route replace default via "${router%% *}" dev "$interface"
    : > /etc/resolv.conf.new
    for server in $dns; do echo "nameserver $server" >> /etc/resolv.conf.new; done
    mv /etc/resolv.conf.new /etc/resolv.conf
    ;;
esac
exit 0
