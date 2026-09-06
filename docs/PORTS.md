# Meridian Port Matrix & Dynamic Allocation Guide

## 1. The Definitive Port Standard

In early versions of the prototype, streaming was tested on port `9100`. In the production `MeridianRunner` unified app, **port 9200 is the single definitive stream port**. All code, tunnels, configs, and reverse proxies are standardized on the following block:

| Function | Base Port | Range (32 Slots) | Protocol | Handled By |
| :--- | :--- | :--- | :--- | :--- |
| **WDA Automation** | `8100` | `8100 - 8131` | HTTP / REST | WebDriverAgent runner on iPhone |
| **Control Bridge** | `9001` | `9001 - 9032` | HTTP & WebSocket (`/ws`) | `meridian-hub` CoreDevice bridge |
| **Screen Stream** | `9200` | `9200 - 9231` | HTTP & WebSocket (`/stream.ws`) | `MeridianRunner` H.264 engine |
| **Remote Tunneld** | `49151` | `49151 - 49182` | TCP | `pymobiledevice3` CoreDevice tunneld |

---

## 2. Dynamic Slot Allocation (strictly USB Connection Order)

Slots are allocated by `SlotManager` in thread-safe, strictly increasing order of physical USB connection:

| Slot # | Device Description | WDA Port | Bridge Port | Stream Port | Tunnel Port |
| :---: | :--- | :---: | :---: | :---: | :---: |
| **Slot 0** | 1st connected iPhone | `8100` | `9001` | `9200` | `49151` |
| **Slot 1** | 2nd connected iPhone | `8101` | `9002` | `9201` | `49152` |
| **Slot 2** | 3rd connected iPhone | `8102` | `9003` | `9202` | `49153` |
| **Slot N** | (N+1)th connected iPhone | `8100 + N` | `9001 + N` | `9200 + N` | `49151 + N` |

When a device is unplugged, its slot is released immediately and made available for the next device.

---

## 3. Host Windows Firewall Rules

Meridian automatically configures inbound Windows Firewall rules on first launch for all slot ranges:

```cmd
netsh advfirewall firewall add rule name="Meridian-WDA" dir=in action=allow protocol=TCP localport=8100-8131
netsh advfirewall firewall add rule name="Meridian-Bridge" dir=in action=allow protocol=TCP localport=9001-9032
netsh advfirewall firewall add rule name="Meridian-Stream" dir=in action=allow protocol=TCP localport=9200-9231
netsh advfirewall firewall add rule name="Meridian-Tunneld" dir=in action=allow protocol=TCP localport=49151-49182
```

---

## 4. VPS Nginx Routing (Zero Leaked IPs)

On the VPS (`meridianhub.cc`), Nginx translates incoming public HTTPS / WSS requests into Tailscale traffic routed to the Windows host:

```nginx
# Device proxy: translates /dev/{port}/* to http://100.101.105.127:{port}/*
location ~ ^/dev/(\d+)/(.*) {
    proxy_pass http://100.101.105.127:$1/$2$is_args$args;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_buffering off;
    proxy_read_timeout 300s;
    proxy_send_timeout 300s;
    proxy_connect_timeout 10s;
}
```

### Endpoints exposed to browser:
* **Control WebSocket**: `wss://meridianhub.cc/dev/9001/ws`
* **Stream WebSocket**: `wss://meridianhub.cc/dev/9200/stream.ws`
* **App List & Launch**: `https://meridianhub.cc/dev/9001/apps.json`
* **WDA Automation**: `https://meridianhub.cc/dev/8100/status`
