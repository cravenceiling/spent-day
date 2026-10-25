# spentday-server

The sync server. It stores encrypted batches and can't read them.

## Run it

1. Run the server where all your devices can reach it, behind HTTPS. The server speaks plain HTTP, so keep it on localhost and put a TLS proxy in front. With [Caddy](https://caddyserver.com) and a domain name that points at the machine:

   ```sh
   cargo run --release -p spentday-server -- --listen 127.0.0.1:8787 --db ./spentday-server.db
   # or: docker build -f server/Dockerfile -t spentday-server . && docker run -d -p 127.0.0.1:8787:8787 -v spentday:/data spentday-server
   caddy reverse-proxy --from sync.example.com --to 127.0.0.1:8787
   ```

   Caddy gets a certificate for `sync.example.com` and serves the server at `https://sync.example.com`.

   Anyone who can reach the server can create spaces on it and fill its disk, because it has no quotas yet. For now, keep it on a private network, or let only your devices' addresses through with an IP allowlist at the proxy or the firewall. A login page or Basic auth at the proxy would lock out the apps, which can't sign in to a proxy.

2. On one device, open Sync settings, enter the server URL, and choose **Create new sync code**.
3. On every other device, enter the same URL and **Join** with that code.

The apps accept plain `http://` only for the same device (`localhost` or 127.x) and private network addresses (10.x, 172.16 to 172.31, 192.168.x, fc00::/7), so you can test on a home network. `localhost` means 127.0.0.1, so a server that listens only on `[::1]` must be entered as `http://[::1]:<port>`. Use the address, not a `.local` name, because plain `http://` to a name could go wherever the network's name lookup says. Every other server needs `https://`.

## What the server sees

The sync code is your account key. It encrypts everything before it leaves the device, so the server never sees app names, domains or device names. The server does see which devices sync, when, from which IP address, and how much. Anyone with the code can read your synced data, so only enter it on your own devices. [`docs/DESIGN.md`](../docs/DESIGN.md) has the full threat model.
