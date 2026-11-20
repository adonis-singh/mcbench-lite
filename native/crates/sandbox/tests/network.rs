//! The networked command's reach on this machine (the netspike's matrix and
//! `undo.sh`, docs/native-app.md, The networked `bash` command): every
//! address of every interface of the host, the host's gateway and
//! resolvers, pasta's gateway and DNS address off port 53, one address of
//! every refused range, IPv6, and names resolving to private addresses are
//! refused (curl's exit 7, `EPERM` for UDP, as today's container answers),
//! with a listener on the host that must see no connection; DNS, ports 80,
//! 443 and 8080 and UDP to a public resolver work; and the command cannot
//! undo the rules. Needs `MCBENCH_SANDBOX_NETWORK=1` beside the rootfs
//! variables, and the internet. Run with `cargo test -p mcbench-sandbox
//! --test network -- --ignored`.
//!
//! On macOS the same matrix runs against the app's VM (the guest base and
//! the signed binary as `tests/macos_vm.rs` says): the host is the Mac, by
//! every address it has once the VM runs (the NAT's own address on its
//! bridge, which is the VM's gateway and resolver, among them), its default
//! gateway and its resolvers, and the VM's own address on the NAT.
//!
//! On Windows the same matrix runs against the app's WSL2 distro (the guest
//! base as on macOS, `MCBENCH_WSL_DISTRO` naming the test's distro): the
//! host is Windows, by every address it has (WSL's virtual switch among
//! them), its default gateways and its resolvers, and the distro's own
//! address, its gateway and its resolver as the distro sees them.

#![cfg(any(target_os = "linux", target_os = "macos", windows))]

mod support;

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use support::*;

const MIB: u64 = 1024 * 1024;
/// A port the undo probe tries on the host, where nothing listens.
const PORT: u16 = 3147;

fn needs_network() {
    assert_eq!(
        std::env::var("MCBENCH_SANDBOX_NETWORK").as_deref(),
        Ok("1"),
        "MCBENCH_SANDBOX_NETWORK=1 is needed (this test reaches the internet)"
    );
}

/// Every address of every interface (`getifaddrs`).
#[cfg(unix)]
fn host_addresses() -> Vec<IpAddr> {
    let mut out = Vec::new();
    // SAFETY: getifaddrs fills a list that freeifaddrs frees; every pointer
    // read is from that list while it lives.
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        assert_eq!(libc::getifaddrs(&mut list), 0);
        let mut at = list;
        while !at.is_null() {
            let addr = (*at).ifa_addr;
            if !addr.is_null() {
                match i32::from((*addr).sa_family) {
                    libc::AF_INET => {
                        let v4 = &*(addr as *const libc::sockaddr_in);
                        out.push(IpAddr::from(u32::from_be(v4.sin_addr.s_addr).to_be_bytes()));
                    }
                    libc::AF_INET6 => {
                        let v6 = &*(addr as *const libc::sockaddr_in6);
                        out.push(IpAddr::from(v6.sin6_addr.s6_addr));
                    }
                    _ => {}
                }
            }
            at = (*at).ifa_next;
        }
        libc::freeifaddrs(list);
    }
    out.sort();
    out.dedup();
    out
}

/// The host's default gateways (`/proc/net/route`) and IPv4 resolvers.
#[cfg(target_os = "linux")]
fn host_gateways_and_resolvers() -> Vec<IpAddr> {
    let mut out = Vec::new();
    for line in std::fs::read_to_string("/proc/net/route")
        .unwrap()
        .lines()
        .skip(1)
    {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() > 2
            && fields[1] == "00000000"
            && let Ok(gateway) = u32::from_str_radix(fields[2], 16)
            && gateway != 0
        {
            out.push(IpAddr::from(gateway.to_le_bytes()));
        }
    }
    for line in std::fs::read_to_string("/etc/resolv.conf")
        .unwrap_or_default()
        .lines()
    {
        if let Some(server) = line.strip_prefix("nameserver ")
            && let Ok(address) = server.trim().parse::<IpAddr>()
        {
            out.push(address);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The Mac's default gateway (`route -n get default`) and resolvers
/// (`scutil --dns`), and the VM's own address on the NAT (its init's DHCP
/// lease in the console log).
#[cfg(target_os = "macos")]
fn host_gateways_and_resolvers() -> Vec<IpAddr> {
    let run = |program: &str, args: &[&str]| {
        String::from_utf8(
            std::process::Command::new(program)
                .args(args)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
    };
    let mut out = Vec::new();
    for line in run("/sbin/route", &["-n", "get", "default"]).lines() {
        if let Some(gateway) = line.trim().strip_prefix("gateway: ")
            && let Ok(address) = gateway.trim().parse::<IpAddr>()
        {
            out.push(address);
        }
    }
    for line in run("/usr/sbin/scutil", &["--dns"]).lines() {
        if let Some((_, server)) = line.split_once("nameserver[")
            && let Some((_, address)) = server.split_once(" : ")
            && let Ok(address) = address.trim().parse::<IpAddr>()
        {
            out.push(address);
        }
    }
    let vm = vm_root().join(".vm");
    for entry in std::fs::read_dir(vm).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("console-") {
            for line in std::fs::read_to_string(entry.path()).unwrap().lines() {
                if let Some(rest) = line.strip_prefix("udhcpc: lease of ")
                    && let Some(address) = rest.split_whitespace().next()
                    && let Ok(address) = address.parse::<IpAddr>()
                {
                    out.push(address);
                }
            }
        }
    }
    out.retain(|a| !matches!(a, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80));
    // A public resolver the Mac uses (its provider's) is not the Mac: the
    // networked command reaches public addresses, as today.
    out.retain(|a| match a {
        IpAddr::V4(v4) => {
            let public = !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || (v4.octets()[0] == 100 && v4.octets()[1] & 0xc0 == 64));
            if public {
                eprintln!("[matrix] {v4}, a public resolver of the Mac, is not a refused address");
            }
            !public
        }
        IpAddr::V6(_) => true,
    });
    out.sort();
    out.dedup();
    out
}

/// Windows' adapters (`GetAdaptersAddresses`, which a standard user may
/// call): every unicast address, and every gateway and resolver.
#[cfg(windows)]
fn adapters() -> (Vec<IpAddr>, Vec<IpAddr>) {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_GATEWAYS, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
    };
    // SAFETY: a socket address the adapter list points at, of its family's
    // size.
    let address = |sockaddr: *const SOCKADDR| unsafe {
        match (*sockaddr).sa_family {
            AF_INET => Some(IpAddr::from(
                (*(sockaddr as *const SOCKADDR_IN))
                    .sin_addr
                    .S_un
                    .S_addr
                    .to_ne_bytes(),
            )),
            AF_INET6 => Some(IpAddr::from(
                (*(sockaddr as *const SOCKADDR_IN6)).sin6_addr.u.Byte,
            )),
            _ => None,
        }
    };
    let mut size = 64 * 1024u32;
    let mut buffer = vec![0u64; size as usize / 8];
    // SAFETY: the buffer is `size` bytes, aligned for the list's structs.
    let code = unsafe {
        GetAdaptersAddresses(
            AF_UNSPEC as u32,
            GAA_FLAG_INCLUDE_GATEWAYS,
            std::ptr::null(),
            buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
            &mut size,
        )
    };
    assert_eq!(code, 0, "GetAdaptersAddresses");
    let (mut own, mut others) = (Vec::new(), Vec::new());
    let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    // SAFETY: the linked lists GetAdaptersAddresses wrote into the buffer,
    // read while the buffer lives.
    unsafe {
        while !adapter.is_null() {
            let mut unicast = (*adapter).FirstUnicastAddress;
            while !unicast.is_null() {
                own.extend(address((*unicast).Address.lpSockaddr));
                unicast = (*unicast).Next;
            }
            let mut gateway = (*adapter).FirstGatewayAddress;
            while !gateway.is_null() {
                others.extend(address((*gateway).Address.lpSockaddr));
                gateway = (*gateway).Next;
            }
            let mut dns = (*adapter).FirstDnsServerAddress;
            while !dns.is_null() {
                others.extend(address((*dns).Address.lpSockaddr));
                dns = (*dns).Next;
            }
            adapter = (*adapter).Next;
        }
    }
    (own, others)
}

/// Every address of every interface of Windows.
#[cfg(windows)]
fn host_addresses() -> Vec<IpAddr> {
    let mut out = adapters().0;
    out.sort();
    out.dedup();
    out
}

/// Windows' gateways and resolvers, and the distro's own address, gateway
/// and resolvers (read in the distro as root).
#[cfg(windows)]
fn host_gateways_and_resolvers() -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = adapters()
        .1
        .into_iter()
        .filter(|address| !address.is_unspecified())
        .collect();
    let distro = std::env::var("MCBENCH_WSL_DISTRO").unwrap_or_else(|_| "mcbench-lite".to_string());
    let inside = std::process::Command::new("wsl.exe")
        .args([
            "--distribution",
            &distro,
            "--user",
            "root",
            "--cd",
            "/",
            "--exec",
        ])
        .args(["/usr/bin/busybox", "sh", "-c"])
        .arg("cat /etc/resolv.conf; echo; cat /proc/net/route; ip -4 -o addr show")
        .env("WSL_UTF8", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&inside.stdout);
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let Some(server) = line.strip_prefix("nameserver ")
            && let Ok(address) = server.trim().parse::<IpAddr>()
        {
            out.push(address);
        } else if fields.len() > 2
            && fields[1] == "00000000"
            && let Ok(gateway) = u32::from_str_radix(fields[2], 16)
            && gateway != 0
        {
            out.push(IpAddr::from(gateway.to_le_bytes()));
        } else if fields.len() > 3
            && fields[2] == "inet"
            && let Ok(address) = fields[3].split('/').next().unwrap().parse::<IpAddr>()
        {
            out.push(address);
        }
    }
    eprintln!("[matrix] Windows and the distro give {out:?}");
    out.retain(|a| !matches!(a, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80));
    out.sort();
    out.dedup();
    out
}

fn url_host(address: &IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_NETWORK=1, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256, and the internet"]
async fn the_networked_command_reaches_the_internet_and_nothing_of_this_machine_or_its_networks() {
    needs_network();
    let hits = Arc::new(AtomicUsize::new(0));
    // A port the system gives (TCP and UDP the same number): no other run
    // of this test, nor anything else on the machine, holds it.
    let tcp = std::net::TcpListener::bind(("::", 0)).expect("the listener's port");
    let port = tcp.local_addr().unwrap().port();
    let udp = std::net::UdpSocket::bind(("::", port)).expect("the listener's UDP port");
    // Windows' IPv6 sockets take no IPv4 (IPV6_V6ONLY is on by default).
    #[cfg(windows)]
    {
        let tcp4 = std::net::TcpListener::bind(("0.0.0.0", port)).expect("the IPv4 port");
        let udp4 = std::net::UdpSocket::bind(("0.0.0.0", port)).expect("the IPv4 UDP port");
        let tcp_hits = hits.clone();
        std::thread::spawn(move || {
            for stream in tcp4.incoming().flatten() {
                tcp_hits.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        let udp_hits = hits.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 512];
            while udp4.recv_from(&mut buffer).is_ok() {
                udp_hits.fetch_add(1, Ordering::SeqCst);
            }
        });
    }
    let tcp_hits = hits.clone();
    std::thread::spawn(move || {
        for stream in tcp.incoming().flatten() {
            tcp_hits.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });
    let udp_hits = hits.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 512];
        while udp.recv_from(&mut buffer).is_ok() {
            udp_hits.fetch_add(1, Ordering::SeqCst);
        }
    });
    let scratch = Scratch::new("matrix");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "matrix");
    let mut refused: Vec<(String, IpAddr, u16)> = Vec::new();
    // A link-local IPv6 address needs its interface named, which no URL
    // from inside the box can name; IPv6 is off there anyway.
    for address in host_addresses()
        .into_iter()
        .filter(|a| !matches!(a, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80))
    {
        refused.push(("host interface".into(), address, port));
    }
    let pasta_dns: IpAddr = mcbench_sandbox::linux::NETWORK[2].1.parse().unwrap();
    for address in host_gateways_and_resolvers() {
        refused.push(("host gateway or resolver".into(), address, port));
        // In the box pasta's DNS address is the box's resolver, whose port
        // 53 is allowed (today's rule); a host whose own resolver has that
        // address (QEMU's user network gives Windows 10.0.2.3) is reached
        // there only as pasta's forwarder.
        if address != pasta_dns {
            refused.push(("host gateway or resolver, port 53".into(), address, 53));
        }
        refused.push(("host gateway or resolver, port 80".into(), address, 80));
    }
    for (label, address) in mcbench_sandbox::linux::NETWORK {
        if label != "address" {
            refused.push((format!("pasta's {label}"), address.parse().unwrap(), port));
            refused.push((
                format!("pasta's {label}, port 80"),
                address.parse().unwrap(),
                80,
            ));
        }
    }
    // One address in every range EGRESS_RULES refuses, and the edges.
    for address in [
        "0.0.0.1",
        "10.0.0.1",
        "10.255.255.254",
        "100.64.0.1",
        "100.127.255.254",
        "127.0.0.1",
        "127.0.0.53",
        "169.254.169.254",
        "172.16.0.1",
        "172.31.255.254",
        "192.0.0.1",
        "192.168.0.1",
        "192.168.255.254",
        "198.18.0.1",
        "198.19.255.254",
        "224.0.0.251",
        "239.255.255.250",
        "255.255.255.255",
    ] {
        refused.push(("a refused range".into(), address.parse().unwrap(), 80));
    }
    for address in ["::1", "2606:4700:4700::1111"] {
        refused.push(("IPv6".into(), address.parse().unwrap(), 80));
    }
    let mut script = String::new();
    for (i, (_, address, port)) in refused.iter().enumerate() {
        script.push_str(&format!(
            "curl -s -m 3 -o /dev/null http://{}:{port}/; echo tcp{i}=$?\n",
            url_host(address)
        ));
        if address.is_ipv4() && *port != 80 {
            script.push_str(&format!(
                "python3 -I -c \"import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.connect(('{address}', {port}))\ntry: s.send(b'x'); print('udp{i}=sent')\nexcept OSError as e: print('udp{i}=' + str(e.errno))\"\n"
            ));
        }
    }
    // Names that resolve to private addresses: the rule is on the address.
    script.push_str(&format!(
        "curl -s -m 3 -o /dev/null http://localhost:{port}/; echo name0=$?\n"
    ));
    script.push_str(&format!("curl -s -m 3 -o /dev/null --resolve x.example:{port}:192.168.0.1 http://x.example:{port}/; echo name1=$?\n"));
    let host = url_host(
        host_addresses()
            .iter()
            .find(|a| a.is_ipv4() && !a.is_loopback())
            .expect("an IPv4 address on an interface"),
    );
    script.push_str(&format!("curl -s -m 3 -o /dev/null --resolve x.example:{port}:{} http://x.example:{port}/; echo name2=$?\n", host.trim_matches(['[', ']'])));
    // What must work.
    script.push_str("getent hosts example.com > /dev/null; echo dns=$?\n");
    script.push_str(
        "curl -s -m 10 -o /dev/null -w '%{http_code}' http://example.com/; echo \" http=$?\"\n",
    );
    script.push_str(
        "curl -s -m 10 -o /dev/null -w '%{http_code}' https://example.com/; echo \" https=$?\"\n",
    );
    script.push_str("curl -s -m 10 -o /dev/null http://portquiz.net:8080/; echo port8080=$?\n");
    script.push_str("python3 -I -c \"import socket, struct; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(5); s.sendto(struct.pack('>HHHHHH', 1, 256, 1, 0, 0, 0) + b'\\x07example\\x03com\\x00\\x00\\x01\\x00\\x01', ('1.1.1.1', 53)); print('public_udp53=' + str(len(s.recv(512)) > 12))\"\n");
    script.push_str("printf 'v6:%s/%s\\n' \"$(cat /proc/sys/net/ipv6/conf/*/disable_ipv6 | sort -u | tr -d '\\n')\" \"$(wc -l < /proc/net/if_inet6)\"\n");

    std::fs::write(ws.host.join("probe.sh"), &script).unwrap();
    let result = run(sandbox, &ws, "bash probe.sh", 120_000.0, true)
        .await
        .unwrap();
    let out = output(&result).to_string();
    let value = |key: &str| {
        out.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::to_string)
    };
    let mut table = Vec::new();
    let mut reached = Vec::new();
    for (i, (label, address, port)) in refused.iter().enumerate() {
        let tcp = value(&format!("tcp{i}")).unwrap_or_default();
        let udp = value(&format!("udp{i}"));
        table.push(format!(
            "{label:36} {address:40} {port:5} curl={tcp} udp={}",
            udp.clone().unwrap_or("-".into())
        ));
        if tcp != "7" {
            reached.push(format!("{label} {address}:{port}: curl {tcp}"));
        }
        if let Some(udp) = udp {
            // 127/8 is the box's own loopback (`-o lo` is accepted, as
            // today): a datagram goes nowhere, and the host's listener counts it.
            let expected = if address.is_loopback() { "sent" } else { "1" };
            if udp != expected {
                reached.push(format!("{label} {address}:{port}: UDP {udp} (EPERM is 1)"));
            }
        }
    }
    eprintln!("{}", table.join("\n"));
    assert!(reached.is_empty(), "{reached:#?}\n{out}");
    for key in ["name0", "name1", "name2"] {
        assert_eq!(value(key).as_deref(), Some("7"), "{key}\n{out}");
    }
    assert_eq!(value("dns").as_deref(), Some("0"), "{out}");
    assert_eq!(value("http").as_deref(), None);
    assert!(
        out.contains("200 http=0") && out.contains("200 https=0"),
        "{out}"
    );
    assert_eq!(value("port8080").as_deref(), Some("0"), "{out}");
    assert_eq!(value("public_udp53").as_deref(), Some("True"), "{out}");
    assert_eq!(value("v6").as_deref(), None);
    assert!(out.contains("v6:1/0"), "{out}");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the host's listener was reached"
    );
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

// The netspike's undo.sh: the command tries to undo the rules and the IPv6
// setting, then the host is still refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_NETWORK=1, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_networked_command_cannot_undo_its_rules() {
    needs_network();
    let host = host_addresses()
        .into_iter()
        .find(|a| a.is_ipv4() && !a.is_loopback())
        .expect("an IPv4 address on an interface");
    let probe = format!(
        r#"t() {{ out=$("$@" 2>&1); echo "rc=$?"; }}
t iptables -F OUTPUT
t iptables -P OUTPUT ACCEPT
t iptables-legacy -F
t sh -c 'echo 0 > /proc/sys/net/ipv6/conf/all/disable_ipv6'
t unshare -Ur true
t unshare -Ur iptables -F OUTPUT
t unshare -Urn true
t python3 -I -c 'import socket; socket.socket(socket.AF_INET, socket.SOCK_RAW, socket.IPPROTO_TCP)'
t python3 -I -c 'import socket; socket.socket(socket.AF_PACKET, socket.SOCK_RAW)'
t python3 -I -c '
import socket,struct
s=socket.socket(socket.AF_NETLINK,socket.SOCK_RAW,0)
ifa=struct.pack("BBBBI",socket.AF_INET,24,0,0,2)
rta=struct.pack("HH",8,1)+socket.inet_aton("10.0.2.50")
msg=ifa+rta; hdr=struct.pack("IHHII",16+len(msg),20,0x405,1,0)
s.send(hdr+msg); r=s.recv(256); err=struct.unpack("i",r[16:20])[0]; raise SystemExit(0 if err==0 else 1)'
t mount -o remount,rw /proc/sys
curl -s -m 3 -o /dev/null http://{host}:{PORT}/; echo after=$?
cat /proc/sys/net/ipv6/conf/all/disable_ipv6"#
    );
    let scratch = Scratch::new("undo");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "undo");
    let result = run(sandbox, &ws, &probe, 60_000.0, true).await.unwrap();
    let out = output(&result);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 13, "{out}");
    for (i, line) in lines[..11].iter().enumerate() {
        assert_ne!(*line, "rc=0", "attempt {i} worked\n{out}");
    }
    assert_eq!(lines[11], "after=7", "{out}");
    assert_eq!(lines[12], "1", "{out}");
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

// What sets the rules since they run in netinit's own namespaces
// (`keeper::run_rules`) rather than under bubblewrap: the command tries to
// reach it and to undo the rules through it, then the host is still
// refused. The box's programs have no capability in any user namespace that
// owns the network namespace (the box's user namespace is a child of
// pasta's, its programs' capability sets are empty and `no_new_privs` is
// set), they see only the box's PID namespace (the rules' runner and
// netinit have exited, pasta is outside it), and the program that runs
// netinit is not in the box's root.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_NETWORK=1, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_networked_command_cannot_reach_what_set_its_rules() {
    needs_network();
    let host = host_addresses()
        .into_iter()
        .find(|a| a.is_ipv4() && !a.is_loopback())
        .expect("an IPv4 address on an interface");
    let probe = format!(
        r#"t() {{ out=$("$@" 2>&1); echo "rc=$?"; }}
t nsenter --preserve-credentials -U -n -t 1 iptables -F OUTPUT
t nsenter -n -t 1 iptables -F OUTPUT
t nsenter -m -t 1 true
t nft flush ruleset
t iptables-nft -F OUTPUT
t iptables-nft -I OUTPUT 1 -j ACCEPT
t {AGENT} netinit /dev/null
t unshare -m true
t mount -t proc proc /proc
t mount -t tmpfs tmpfs /tmp
t mount --bind / /tmp
t sh -c 'echo nameserver 10.0.0.1 > /etc/resolv.conf'
t sh -c 'for p in /proc/[0-9]*; do [ "$(readlink $p/ns/net)" = "$(readlink /proc/self/ns/net)" ] || exit 0; done; exit 1'
curl -s -m 3 -o /dev/null http://{host}:{PORT}/; echo after=$?
cat /proc/sys/net/ipv6/conf/all/disable_ipv6"#
    );
    let scratch = Scratch::new("reach");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "reach");
    let result = run(sandbox, &ws, &probe, 60_000.0, true).await.unwrap();
    let out = output(&result);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 15, "{out}");
    for (i, line) in lines[..13].iter().enumerate() {
        assert_ne!(*line, "rc=0", "attempt {i} worked\n{out}");
    }
    assert_eq!(lines[13], "after=7", "{out}");
    assert_eq!(lines[14], "1", "{out}");
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

// The rules' runner itself (`keeper::run_rules`), run as root of a user and
// a network namespace of its own (`unshare --user --map-root-user --net`,
// standing for pasta's): in every capability set it holds CAP_NET_ADMIN and
// nothing else, it cannot gain a privilege, its root filesystem is
// read-only as bubblewrap's `--ro-bind` gave it, and `/run` takes a file
// (iptables' lock). The test starts itself again under `unshare`, where it
// is the runner's caller.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs MCBENCH_SANDBOX_NETWORK=1, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
fn the_rules_runner_holds_only_net_admin_over_a_read_only_root() {
    use mcbench_sandbox::linux::keeper::{Rules, run_rules};
    const INSIDE: &str = "MCBENCH_RULES_RUNNER_SPEC";
    const NAME: &str = "the_rules_runner_holds_only_net_admin_over_a_read_only_root";
    if let Some(spec) = std::env::var_os(INSIDE) {
        let rules: Rules = serde_json::from_slice(&std::fs::read(spec).unwrap()).unwrap();
        let status = run_rules(&rules);
        assert!(status.success(), "the rules' runner: {status}");
        return;
    }
    needs_network();
    let (tarball, sha) = rootfs();
    let unpacked = mcbench_sandbox::linux::rootfs::unpack(&tarball, &sha, &shared_root()).unwrap();
    let scratch = Scratch::new("runner");
    let resolv = scratch.0.join("resolv.conf");
    std::fs::write(&resolv, "nameserver 192.0.2.1\n").unwrap();
    let path = unpacked
        .image
        .env
        .iter()
        .find_map(|e| e.strip_prefix("PATH="))
        .expect("the image's PATH")
        .to_string();
    let rules = Rules {
        rootfs: unpacked.rootfs.to_string_lossy().into_owned(),
        resolv: resolv.to_string_lossy().into_owned(),
        path,
        // The first line ends the test harness's own "test ... " line.
        script: r#"echo
grep -E '^(Cap(Inh|Prm|Eff|Bnd|Amb)|NoNewPrivs):' /proc/self/status
echo "rofs=$(touch /usr/mcbench-runner-probe 2>&1 | grep -c 'Read-only file system')"
: > /run/mcbench-runner-probe && echo run=ok
echo "resolv=$(cat /etc/resolv.conf)""#
            .to_string(),
    };
    let spec = scratch.0.join("rules.json");
    std::fs::write(&spec, serde_json::to_vec(&rules).unwrap()).unwrap();
    let ran = std::process::Command::new("unshare")
        .args(["--user", "--map-root-user", "--net"])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            NAME,
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(INSIDE, &spec)
        .output()
        .expect("unshare (util-linux) starts");
    let out = String::from_utf8_lossy(&ran.stdout).into_owned();
    let err = String::from_utf8_lossy(&ran.stderr);
    assert!(ran.status.success(), "{}\n{out}\n{err}", ran.status);
    let value = |name: &str| {
        out.lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or_else(|| panic!("no {name} line\n{out}\n{err}"))
            .trim()
            .to_string()
    };
    let net_admin = format!("{:016x}", 1u64 << 12);
    for set in ["CapInh:", "CapPrm:", "CapEff:", "CapBnd:"] {
        assert_eq!(value(set), net_admin, "{set}\n{out}");
    }
    assert_eq!(value("CapAmb:"), format!("{:016x}", 0), "{out}");
    assert_eq!(value("NoNewPrivs:"), "1", "{out}");
    assert_eq!(value("rofs="), "1", "{out}");
    assert_eq!(value("run="), "ok", "{out}");
    assert_eq!(value("resolv="), "nameserver 192.0.2.1", "{out}");
    assert!(
        !unpacked.rootfs.join("usr/mcbench-runner-probe").exists(),
        "the runner wrote into the root filesystem"
    );
    assert!(
        !unpacked.rootfs.join("run/mcbench-runner-probe").exists(),
        "the runner's /run was the root filesystem's own"
    );
}

// A box's network namespace does not cover vsock, so every program of a box
// is refused the family (`linux::refused`), as today's Docker refuses it:
// `socket(AF_VSOCK, ...)` of each socket type answers EPERM in a persistent
// box and in a networked one, where the same call made here, outside any
// box, makes a socket (the control: on a machine without vsock the refusal
// would prove nothing). The other families are untouched.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_NETWORK=1, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256, and a machine with vsock"]
async fn no_program_of_a_box_can_make_a_vsock_socket() {
    needs_network();
    // SAFETY: socket and close on a descriptor made here.
    let outside = unsafe {
        let fd = libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0);
        let made = fd >= 0;
        let error = std::io::Error::last_os_error();
        if made {
            libc::close(fd);
        }
        made.then_some(()).ok_or(error)
    };
    outside.expect(
        "this machine makes no vsock socket outside a box, so the test proves nothing here",
    );
    let probe = r#"python3 -I -c '
import socket
for kind in ("SOCK_STREAM", "SOCK_DGRAM", "SOCK_SEQPACKET"):
    try:
        socket.socket(socket.AF_VSOCK, getattr(socket, kind)).close()
        print(kind, "made")
    except OSError as error:
        print(kind, error.errno)
for family in ("AF_INET", "AF_UNIX"):
    socket.socket(getattr(socket, family), socket.SOCK_STREAM).close()
    print(family, "made")
'"#;
    let scratch = Scratch::new("vsock");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "vsock");
    for network in [false, true] {
        let result = run(sandbox, &ws, probe, 60_000.0, network).await.unwrap();
        let eperm = libc::EPERM;
        assert_eq!(
            output(&result).lines().collect::<Vec<_>>(),
            [
                format!("SOCK_STREAM {eperm}"),
                format!("SOCK_DGRAM {eperm}"),
                format!("SOCK_SEQPACKET {eperm}"),
                "AF_INET made".to_string(),
                "AF_UNIX made".to_string(),
            ],
            "networked: {network}"
        );
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}
