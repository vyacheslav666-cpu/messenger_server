let isRegisterMode = false;
let currentActiveChatUserId = null;
let currentActiveChatLogin = null;
let ws = null;
let reconnectTimer = null;

const API_URL = `${window.location.origin}/api`;
const WS_URL = `${window.location.protocol === 'https:' ? 'wss' : 'ws'}://${window.location.host}/ws`;

const el = (id) => document.getElementById(id);

window.addEventListener('load', () => {
    bindEvents();
    if (sessionStorage.getItem('userId')) showMessengerUI();
});

function bindEvents() {
    el('actionBtn').addEventListener('click', submitAuth);
    el('registerLink').addEventListener('click', () => switchMode(true));
    el('logoutBtn').addEventListener('click', logout);
    el('searchBtn').addEventListener('click', searchContact);
    el('chatSendBtn').addEventListener('click', sendChatMessage);

    el('loginInput').addEventListener('keydown', enterAuth);
    el('passInput').addEventListener('keydown', enterAuth);
    el('passConfirmInput').addEventListener('keydown', enterAuth);
    el('searchInput').addEventListener('keydown', (e) => { if (e.key === 'Enter') searchContact(); });
    el('chatMessageInput').addEventListener('keydown', (e) => { if (e.key === 'Enter') sendChatMessage(); });
}

function enterAuth(e) {
    if (e.key === 'Enter') submitAuth();
}

function showMessengerUI() {
    el('authBox').style.display = 'none';
    el('messengerBox').style.display = 'flex';
    el('myUsername').innerText = sessionStorage.getItem('userLogin');
    initWebSocket();
    loadActiveChats();
}

function initWebSocket() {
    const myId = sessionStorage.getItem('userId');
    if (!myId) return;

    if (ws && [WebSocket.OPEN, WebSocket.CONNECTING].includes(ws.readyState)) return;
    ws = new WebSocket(`${WS_URL}?user_id=${encodeURIComponent(myId)}`);

    ws.onmessage = (event) => {
        const msg = JSON.parse(event.data);
        const myIdNum = Number(sessionStorage.getItem('userId'));
        const otherUserId = msg.sender_id === myIdNum ? msg.receiver_id : msg.sender_id;

        ensureContact(otherUserId, otherUserId === currentActiveChatUserId ? currentActiveChatLogin : `user-${otherUserId}`);

        if (otherUserId === currentActiveChatUserId) {
            appendMessage(msg, msg.sender_id === myIdNum ? 'my-msg' : 'their-msg');
        } else {
            loadActiveChats();
        }
    };

    ws.onclose = () => {
        clearTimeout(reconnectTimer);
        reconnectTimer = setTimeout(initWebSocket, 3000);
    };
}

function logout() {
    sessionStorage.clear();
    if (ws) ws.close();
    location.reload();
}

function switchMode(toRegister) {
    isRegisterMode = toRegister;
    hideMessages();

    if (isRegisterMode) {
        el('formTitle').innerText = 'Регистрация';
        el('passConfirmInput').style.display = 'block';
        el('actionBtn').innerText = 'Создать аккаунт';
        el('toggleBlock').innerHTML = 'Уже есть аккаунт? <span id="loginLink">Войти</span>';
        el('loginLink').addEventListener('click', () => switchMode(false));
    } else {
        el('formTitle').innerText = 'Войти в Мессенджер';
        el('passConfirmInput').style.display = 'none';
        el('actionBtn').innerText = 'Войти';
        el('toggleBlock').innerHTML = 'Нет аккаунта? <span id="registerLink">Зарегистрироваться</span>';
        el('registerLink').addEventListener('click', () => switchMode(true));
    }
}

async function submitAuth() {
    const login = el('loginInput').value.trim();
    const password = el('passInput').value;
    const passConfirm = el('passConfirmInput').value;

    if (!login || !password) return showError('Заполните все поля');
    if (isRegisterMode && password !== passConfirm) return showError('Пароли не совпадают');

    const endpoint = isRegisterMode ? 'register' : 'login';
    try {
        const response = await fetch(`${API_URL}/${endpoint}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ login, password })
        });

        if (!response.ok) return showError(await response.text());

        if (isRegisterMode) {
            showSuccess('Аккаунт создан. Теперь войди.');
            setTimeout(() => switchMode(false), 900);
        } else {
            const userData = await response.json();
            sessionStorage.setItem('userId', userData.user_id);
            sessionStorage.setItem('userLogin', userData.login);
            showMessengerUI();
        }
    } catch (_) {
        showError('Сервер недоступен');
    }
}

function hideMessages() {
    el('errorBlock').style.display = 'none';
    el('successBlock').style.display = 'none';
}
function showError(text) {
    el('errorBlock').innerText = text;
    el('errorBlock').style.display = 'block';
    el('successBlock').style.display = 'none';
}
function showSuccess(text) {
    el('successBlock').innerText = text;
    el('successBlock').style.display = 'block';
    el('errorBlock').style.display = 'none';
}

async function loadActiveChats() {
    const myId = sessionStorage.getItem('userId');
    try {
        const response = await fetch(`${API_URL}/chats?user_id=${encodeURIComponent(myId)}`);
        if (!response.ok) return;
        const users = await response.json();
        renderContacts(users);
    } catch (e) {
        console.error('Не удалось загрузить список чатов:', e);
    }
}

function renderContacts(users) {
    const listBlock = el('contactsListBlock');
    listBlock.innerHTML = '';
    if (!users.length) {
        listBlock.innerHTML = '<div class="empty">Список контактов пуст</div>';
        return;
    }
    users.forEach(addContactElement);
}

function addContactElement(u) {
    if (Number(u.user_id) === Number(sessionStorage.getItem('userId'))) return;
    const listBlock = el('contactsListBlock');
    const existing = document.getElementById(`contact-${u.user_id}`);
    if (existing) return;

    const item = document.createElement('div');
    item.className = 'contact-item';
    item.id = `contact-${u.user_id}`;
    item.textContent = `👤 ${u.login}`;
    item.addEventListener('click', () => openChat(u.user_id, u.login));
    listBlock.appendChild(item);
}

function ensureContact(userId, login) {
    const listBlock = el('contactsListBlock');
    const empty = listBlock.querySelector('.empty');
    if (empty) listBlock.innerHTML = '';
    addContactElement({ user_id: userId, login });
}

async function searchContact() {
    const query = el('searchInput').value.trim();
    if (!query) return loadActiveChats();

    try {
        const response = await fetch(`${API_URL}/search?login=${encodeURIComponent(query)}`);
        if (!response.ok) return;
        const users = await response.json();
        renderContacts(users);
    } catch (e) {
        console.error(e);
    }
}

async function openChat(targetUserId, login) {
    currentActiveChatUserId = Number(targetUserId);
    currentActiveChatLogin = login;
    const myId = sessionStorage.getItem('userId');

    document.querySelectorAll('.contact-item').forEach(x => x.classList.remove('active'));
    const activeEl = document.getElementById(`contact-${targetUserId}`);
    if (activeEl) activeEl.classList.add('active');

    el('activeChatHeader').innerText = `Чат с пользователем: ${login}`;
    el('chatMessageInput').disabled = false;
    el('chatSendBtn').disabled = false;
    el('chatMessageInput').focus();

    const display = el('messagesDisplayBlock');
    display.innerHTML = '<div class="empty">Загрузка истории...</div>';

    try {
        const response = await fetch(`${API_URL}/messages?user_id=${encodeURIComponent(myId)}&target_id=${encodeURIComponent(targetUserId)}`);
        if (!response.ok) return;
        const messages = await response.json();
        display.innerHTML = '';
        messages.forEach(msg => appendMessage(msg, msg.sender_id == myId ? 'my-msg' : 'their-msg'));
    } catch(e) {
        console.error('Не удалось подгрузить историю:', e);
    }
}

function sendChatMessage() {
    const input = el('chatMessageInput');
    const text = input.value.trim();
    if (!text || !currentActiveChatUserId || !ws || ws.readyState !== WebSocket.OPEN) return;

    ws.send(JSON.stringify({ receiver_id: currentActiveChatUserId, text }));
    input.value = '';
}

function appendMessage(msg, type) {
    const display = el('messagesDisplayBlock');
    const row = document.createElement('div');
    row.className = `msg-row ${type}`;

    const cloud = document.createElement('div');
    cloud.className = 'msg-cloud';

    const text = document.createElement('div');
    text.textContent = msg.text;

    const time = document.createElement('div');
    time.className = 'msg-time';
    time.textContent = formatTime(msg.timestamp);

    cloud.appendChild(text);
    cloud.appendChild(time);
    row.appendChild(cloud);
    display.appendChild(row);
    display.scrollTop = display.scrollHeight;
}

function formatTime(raw) {
    if (!raw) return '';
    if (raw.includes('-') && raw.includes(' ')) return raw.split(' ')[1].slice(0, 5);
    return raw;
}
