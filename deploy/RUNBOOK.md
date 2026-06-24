# Emergency Messenger Runbook

This is the fast path for an emergency chat when normal messengers are unavailable.

## What You Need Prepared

- A small VPS with Docker and Docker Compose installed.
- Ports open in firewall/security group:
  - `8080` for direct IP emergency mode.
  - Later, `80` and `443` for domain/HTTPS mode.
- Repository URL:
  - `https://github.com/vyacheslav666-cpu/messenger_server.git`

## Fastest Start Without Domain

SSH into the server and run:

```bash
git clone https://github.com/vyacheslav666-cpu/messenger_server.git
cd messenger_server
cp deploy/.env.prod.example .env
docker compose -f deploy/docker-compose.prod.yml --env-file .env up -d --build
```

Open:

```text
http://SERVER_IP:8080
```

Replace `SERVER_IP` with the real server IP.

## Update Existing Server

If the repo is already cloned:

```bash
cd messenger_server
git pull
docker compose -f deploy/docker-compose.prod.yml --env-file .env up -d --build
```

## Stop

```bash
cd messenger_server
docker compose -f deploy/docker-compose.prod.yml --env-file .env down
```

Messages stay in the Docker volume `messenger_data` unless you remove volumes manually.

## Logs

```bash
cd messenger_server
docker compose -f deploy/docker-compose.prod.yml --env-file .env logs -f messenger
```

## SMS Template

Short version:

```text
Экстренный чат: http://SERVER_IP:8080
Логин любой, пароль от 8 символов.
```

More careful version:

```text
Связь тут: http://SERVER_IP:8080
Создай логин и пароль от 8 символов. Не пиши туда то, что нельзя потерять.
```

## Later: Domain And HTTPS

1. Create DNS `A` record:

```text
chat.example.com -> SERVER_IP
```

2. Edit `.env`:

```env
DOMAIN=chat.example.com
RUST_LOG=messenger_server=info
```

3. Start HTTPS mode:

```bash
cd messenger_server
docker compose -f deploy/docker-compose.caddy.yml --env-file .env up -d --build
```

Open:

```text
https://chat.example.com
```

SMS after domain is ready:

```text
Экстренный чат: https://chat.example.com
Логин любой, пароль от 8 символов.
```

## Sanity Check

After deploy:

1. Open the link in one browser and register user `test1`.
2. Open private/incognito window and register user `test2`.
3. Send messages both ways.
4. If it works, send the SMS link.
