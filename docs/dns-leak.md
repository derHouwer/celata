# DNS leaks

On some systems, name lookups ("which address belongs to example.org?") can travel **past the VPN** while
you are connected. Your internet provider or the operator of your local network can then still see which
names you look up, although the rest of your traffic goes through the tunnel. This is called a DNS leak.

Celata shows a hint in the overview when your system is set up in a way that makes this possible:

> DNS leak risk: this system resolves names through systemd-resolved, so DNS requests can bypass the VPN.
> See “DNS leaks” in the Celata documentation.

This page explains the cause, how to check your own system, and what you can do.

## Why it happens

The hide.me client writes the DNS servers of the tunnel into `/etc/resolv.conf` when it connects and
restores the file when it disconnects. It does not know `systemd-resolved`.

Where `systemd-resolved` runs, most programs do not read `/etc/resolv.conf` at all. They ask
`systemd-resolved`, and it keeps asking the DNS servers of your normal network connection, outside the
tunnel.

## Who is affected

| Distribution | As installed |
|---|---|
| Ubuntu 24.04, 26.04, 26.10 | affected: `systemd-resolved` is active by default |
| Fedora 44, 45 | affected: `systemd-resolved` is active by default |
| Debian 13 | usually not affected: `systemd-resolved` is not installed by default |
| Arch Linux | depends on your setup: `systemd-resolved` is installed but not enabled by default |

What counts is your own system, not the table. Check it.

## Checking your system

Connect the VPN with Celata. Then run:

```bash
dig +short TXT whoami.ds.akahelp.net
dig +short whoami.akamai.net
```

(`dig` is in the package `dnsutils` on Debian and Ubuntu, `bind-utils` on Fedora and `bind` on Arch Linux.)

The first command shows the address of the DNS server that asked on your behalf (line `"ns" "…"`); the
second one shows it as a plain address. Run them once without the VPN and once with it:

- If the address with VPN is **the same as without**, or belongs to your internet provider, your lookups
  bypass the VPN.
- If it changes to an address of hide.me, your lookups go through the tunnel.

You can also see which path a lookup takes:

```bash
resolvectl status
```

If the section of your normal network connection (not the one named `vpn`) lists DNS servers and
"Current DNS Server", `systemd-resolved` is asking outside the tunnel.

## What you can do

### Turn systemd-resolved off

This makes the system read `/etc/resolv.conf` directly again, which is the file the hide.me client manages.

```bash
sudo systemctl disable --now systemd-resolved
sudo systemctl mask systemd-resolved
sudo systemctl restart NetworkManager
```

`mask` is needed in addition to `disable`: otherwise NetworkManager starts the service again through D-Bus.

Check afterwards: the hint in Celata disappears within ten seconds, and the commands under "Checking your
system" show an address of hide.me while you are connected.

**Side effects**

- Without `systemd-resolved`, the system uses the DNS servers your network connection provides, without
  the cache and the extra features of `systemd-resolved` (for example DNS over TLS or per-connection DNS
  servers).
- If the VPN connection breaks while the kill switch blocks the internet, name lookups fail instead of
  bypassing the tunnel. That is the intended behavior.
- Other software that relies on `systemd-resolved` (some VPN clients, some container tools) may need its
  own configuration.

**Undoing it**

```bash
sudo systemctl unmask systemd-resolved
sudo systemctl enable --now systemd-resolved
sudo systemctl restart NetworkManager
```

This description assumes NetworkManager, which the desktop editions of Fedora, Ubuntu and Debian use. If
your system manages the network with `systemd-networkd`, turning `systemd-resolved` off needs a DNS
configuration of your own; in that case the check above is the reliable guide, not this recipe.

## Limits of the hint

- The hint says "can", not "does". Celata looks at four local places (`/run/systemd/resolve`,
  `/run/systemd/units`, `/etc/nsswitch.conf`, `/etc/resolv.conf`) and never sends a test request anywhere.
  It does not measure a leak.
- Celata does not recognize other local DNS services such as `dnsmasq` or `resolvconf`. No hint does not
  mean that your lookups are safe.
- If you have configured `systemd-resolved` yourself so that it uses the DNS servers of the tunnel, the
  hint is shown anyway.
- Browsers with their own "secure DNS" (DNS over HTTPS) send lookups to the provider chosen in the
  browser, with or without VPN. That is independent of Celata.

The only reliable answer is the check described above.
