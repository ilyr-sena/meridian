# Meridian Port Matrix & Dynamic Allocation Guide

## 1. Production Port Standard

In the unified Meridian architecture, the following ports are reserved and managed:

| Function | Base Port | Range (32 Slots) | Protocol | Handled By |
| :--- | :--- | :--- | :--- | :--- |
| **WDA Automation** | `8100` | `8100 - 8131` | HTTP / REST | WebDriverAgent runner on iPhone |
| **Control Bridge** | `9001` | `9001 - 9032` | HTTP & WebSocket (`/ws`) | `meridian-hub` (pure Rust bridge) |
| **Screen Stream** | `9200` | `9200 - 9231` | HTTP, HTTPS & WebSocket (`/stream.ws`) | `MeridianRunner` H.264 engine |

> **Note**: Legacy port `49151` (`tunneld`) has been completely removed. CoreDevice DVT communication runs natively in pure Rust via userspace `jktcp` TCP over usbmuxd.

---

## 2. Dynamic Slot Allocation (strictly USB Connection Order)

Slots are assigned dynamically by `SlotManager` in thread-safe order of physical USB connection:

| Slot # | Device Order | WDA Port | Bridge Port | Stream Port |
| :---: | :--- | :---: | :---: | :---: |
| **Slot 0** | 1st connected iPhone | `8100` | `9001` | `9200` |
| **Slot 1** | 2nd connected iPhone | `8101` | `9002` | `9201` |
| **Slot 2** | 3rd connected iPhone | `8102` | `9003` | `9202` |
| **Slot N** | (N+1)th connected iPhone | `8100 + N` | `9001 + N` | `9200 + N` |

When a device is detached or its session is stopped:
1. Hub terminates local tunnels on WDA and Stream ports.
2. Hub stops the bridge server.
3. Hub sends an immediate heartbeat setting `host_ports: null` in MongoDB.
4. The slot is released and made available for subsequent connections without port collisions.

---

## 3. Host Firewall Configuration

### Windows
```cmd
netsh advfirewall firewall add rule name="Meridian-WDA" dir=in action=allow protocol=TCP localport=8100-8131
netsh advfirewall firewall add rule name="Meridian-Bridge" dir=in action=allow protocol=TCP localport=9001-9032
netsh advfirewall firewall add rule name="Meridian-Stream" dir=in action=allow protocol=TCP localport=9200-9231
```

### Linux (ufw / iptables)
```bash
sudo ufw allow 8100:8131/tcp
sudo ufw allow 9001:9032/tcp
sudo ufw allow 9200:9231/tcp
```

---

## 4. VPS Nginx Dynamic Routing Rules

On the VPS (`meridianhub.cc`), Nginx dynamically routes requests to the specific Tailscale IP of the host machine running the device session:

```nginx
# Dynamic multi-node routing: /dev/<tailscale_ip>/<port>/<path>
location ~ ^/dev/(100\.\d+\.\d+\.\d+)/(\d+)(?:/(.*))?$ {
    proxy_pass http://$1:$2/$3$is_args$args;
    proxy_buffering off;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_read_timeout 300s;
    proxy_send_timeout 300s;
    proxy_connect_timeout 10s;
}

# Dedicated SSL reverse proxy for Port 9200 (WebCodecs Secure Context)
server {
    listen 9200 ssl;
    server_name www.meridianhub.cc meridianhub.cc 100.51.75.20 "";

    ssl_certificate /etc/letsencrypt/live/meridianhub.cc/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/meridianhub.cc/privkey.pem;
    include /etc/letsencrypt/options-ssl-nginx.conf;
    ssl_dhparam /etc/letsencrypt/ssl-dhparams.pem;

    error_page 497 https://meridianhub.cc:9200$request_uri;

    location / {
        proxy_pass http://100.93.183.86:9200;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_buffering off;
        proxy_read_timeout 300s;
        proxy_send_timeout 300s;
        proxy_connect_timeout 10s;
    }
}
```

### Public Endpoints Exposed to Browser:
* **Control WebSocket**: `wss://meridianhub.cc/dev/{tailscale_ip}/9001/ws`
* **Stream WebSocket**: `wss://meridianhub.cc/dev/{tailscale_ip}/9200/stream.ws`
* **Direct Stream Page (SSL)**: `https://meridianhub.cc:9200/`
* **Installed Apps List**: `https://meridianhub.cc/dev/{tailscale_ip}/9001/apps.json`
* **App Icon PNG**: `https://meridianhub.cc/dev/{tailscale_ip}/9001/icon/{bundle_id}.png`
* **WDA Automation**: `https://meridianhub.cc/dev/{tailscale_ip}/8100/status`
