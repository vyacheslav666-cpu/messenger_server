# Messenger Server

Нормализованная версия MVP-мессенджера:

- Rust + Axum
- SQLite
- WebSocket live-сообщения
- фронтенд отдаётся самим сервером
- Docker / Docker Compose
- готовые заготовки для VPS, systemd и nginx

## Структура

```text
messenger_anon_v02/
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── db.rs
│   ├── error.rs
│   ├── models.rs
│   ├── routes.rs
│   ├── state.rs
│   └── websocket.rs
├── static/
│   ├── index.html
│   ├── app.js
│   └── style.css
├── deploy/
│   ├── messenger.service
│   └── nginx.conf
├── Cargo.toml
├── Dockerfile
├── docker-compose.yml
└── .env.example
```

## Запуск на Windows 11 без Docker

Нужен Rust:

```powershell
winget install Rustlang.Rustup
```

Потом в папке проекта:

```powershell
cargo run
```

Открыть в браузере:

```text
http://localhost:8080
```

## Запуск на Windows 11 через Docker

Поставить Docker Desktop, потом:

```powershell
docker compose up --build
```

Открыть:

```text
http://localhost:8080
```

## Запуск на VPS Ubuntu через Docker

```bash
sudo apt update
sudo apt install -y git docker.io docker-compose-plugin
sudo systemctl enable --now docker

git clone <URL_ТВОЕГО_ПРИВАТНОГО_РЕПОЗИТОРИЯ> messenger
cd messenger
docker compose up -d --build
```

Проверка:

```bash
curl http://127.0.0.1:8080/health
```

## Запуск на VPS Ubuntu без Docker

```bash
sudo apt update
sudo apt install -y git curl build-essential pkg-config libssl-dev nginx
curl https://sh.rustup.rs -sSf | sh
source ~/.cargo/env

git clone <URL_ТВОЕГО_ПРИВАТНОГО_РЕПОЗИТОРИЯ> messenger
cd messenger
cargo build --release
```

Потом можно поставить бинарник в `/opt/messenger` и подключить `deploy/messenger.service`.

## Nginx + домен

Скопировать `deploy/nginx.conf` в:

```bash
sudo cp deploy/nginx.conf /etc/nginx/sites-available/messenger
sudo ln -s /etc/nginx/sites-available/messenger /etc/nginx/sites-enabled/messenger
sudo nginx -t
sudo systemctl reload nginx
```

В `nginx.conf` заменить `your-domain.com` на свой домен.

Для HTTPS:

```bash
sudo apt install -y certbot python3-certbot-nginx
sudo certbot --nginx -d your-domain.com
```

## Что исправлено

- Сервер сам отдаёт фронтенд, больше не надо открывать `index.html` как файл.
- API теперь лежит под `/api/...`.
- WebSocket лежит на `/ws`.
- Пароль на фронте называется `password`, а не `password_hash`.
- Сервер не верит `sender_id` из браузера.
- Текст сообщений вставляется через `textContent`, а не через `innerHTML`.
- Код разбит на модули.
- Добавлен Docker и деплой-заготовки.

## Что ещё НЕ production

Это всё ещё учебный MVP. Для реального публичного сервиса потом нужны:

- нормальные сессии/JWT вместо `sessionStorage userId`
- PostgreSQL вместо SQLite
- HTTPS обязательно
- rate limit
- миграции базы
- удаление/редактирование сообщений
- список контактов/диалогов как отдельная сущность
- защита от спама


## v0.2: анонимные принципы

Добавлено:

- список чатов сортируется по последнему сообщению;
- счётчик непрочитанных сообщений;
- автообновление списка чатов при входящих сообщениях;
- нет онлайн-статуса;
- нет read receipts: сервер хранит last_read только для счётчика самого пользователя и никому его не отдаёт;
- сервер не логирует текст сообщений;
- сообщения экранируются на фронте через textContent.

Дальше по плану:

1. блокировка пользователей;
2. удаление аккаунта;
3. групповые беседы через общую таблицу chats/chat_members;
4. поиск по истории;
5. предпросмотр ссылок без загрузки файлов на сервер;
6. production mode без лишних логов;
7. HTTPS + reverse proxy + Docker на VPS.
