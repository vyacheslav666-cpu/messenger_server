let isRegisterMode = false;
let currentChat = null; // { type: 'direct'|'group', userId?, chatId?, title }
let ws = null;
let reconnectTimer = null;
let lastSearchResults = [];

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
    el('deleteAccountBtn').addEventListener('click', deleteAccount);
    el('blockBtn').addEventListener('click', blockCurrentUser);
    el('searchBtn').addEventListener('click', searchContact);
    el('createGroupBtn').addEventListener('click', createGroup);
    el('chatSendBtn').addEventListener('click', sendChatMessage);
    el('historySearchBtn').addEventListener('click', () => searchHistory(false));
    el('globalSearchBtn').addEventListener('click', () => searchHistory(true));
    el('linkPreviewToggle').addEventListener('change', savePreviewSetting);

    ['loginInput', 'passInput', 'passConfirmInput'].forEach(id => el(id).addEventListener('keydown', e => { if (e.key === 'Enter') submitAuth(); }));
    el('searchInput').addEventListener('keydown', e => { if (e.key === 'Enter') searchContact(); });
    el('groupMembersInput').addEventListener('keydown', e => { if (e.key === 'Enter') createGroup(); });
    el('chatMessageInput').addEventListener('keydown', e => { if (e.key === 'Enter') sendChatMessage(); });
    el('historySearchInput').addEventListener('keydown', e => { if (e.key === 'Enter') searchHistory(false); });
}

function showMessengerUI() {
    el('authBox').style.display = 'none';
    el('messengerBox').style.display = 'flex';
    el('myUsername').innerText = sessionStorage.getItem('userLogin');
    el('linkPreviewToggle').checked = localStorage.getItem('linkPreviewEnabled') === '1';
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
        const isCurrent = currentChat && (
            (currentChat.type === 'group' && msg.chat_id === currentChat.chatId) ||
            (currentChat.type === 'direct' && !msg.chat_id && (msg.sender_id === currentChat.userId || msg.receiver_id === currentChat.userId))
        );
        if (isCurrent) {
            appendMessage(msg, msg.sender_id === myIdNum ? 'my-msg' : 'their-msg');
            markCurrentRead();
        } else {
            notifyNewMessage();
        }
        loadActiveChats();
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
        const response = await fetch(`${API_URL}/${endpoint}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ login, password }) });
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
    } catch (_) { showError('Сервер недоступен'); }
}

function hideMessages() { el('errorBlock').style.display = 'none'; el('successBlock').style.display = 'none'; }
function showError(text) { el('errorBlock').innerText = text; el('errorBlock').style.display = 'block'; el('successBlock').style.display = 'none'; }
function showSuccess(text) { el('successBlock').innerText = text; el('successBlock').style.display = 'block'; el('errorBlock').style.display = 'none'; }

async function loadActiveChats() {
    const myId = sessionStorage.getItem('userId');
    try {
        const response = await fetch(`${API_URL}/chats?user_id=${encodeURIComponent(myId)}`);
        if (!response.ok) return;
        renderChatList(await response.json());
    } catch (e) { console.error('Не удалось загрузить список чатов:', e); }
}

function renderChatList(items) {
    const listBlock = el('contactsListBlock');
    listBlock.innerHTML = '';
    if (!items.length) { listBlock.innerHTML = '<div class="empty">Список чатов пуст</div>'; return; }
    items.forEach(addChatElement);
}

function addChatElement(item) {
    const listBlock = el('contactsListBlock');
    const key = item.chat_type === 'group' ? `group-${item.chat_id}` : `direct-${item.user_id}`;
    const old = document.getElementById(`contact-${key}`);
    if (old) old.remove();

    const div = document.createElement('div');
    div.className = 'contact-item';
    div.id = `contact-${key}`;
    if (currentChat && ((currentChat.type === 'group' && item.chat_type === 'group' && currentChat.chatId === item.chat_id) || (currentChat.type === 'direct' && item.chat_type === 'direct' && currentChat.userId === item.user_id))) {
        div.classList.add('active');
    }

    const name = document.createElement('span');
    name.textContent = `${item.chat_type === 'group' ? '# ' : ''}${item.title}`;
    div.appendChild(name);
    if (item.unread_count && Number(item.unread_count) > 0) {
        const badge = document.createElement('span');
        badge.className = 'unread-badge';
        badge.textContent = Number(item.unread_count) > 99 ? '99+' : String(item.unread_count);
        div.appendChild(badge);
    }
    div.addEventListener('click', () => {
        if (item.chat_type === 'group') openGroupChat(item.chat_id, item.title);
        else openDirectChat(item.user_id, item.title);
    });
    listBlock.appendChild(div);
}

async function searchContact() {
    const query = el('searchInput').value.trim();
    if (!query) return loadActiveChats();
    try {
        const myId = sessionStorage.getItem('userId');
        const response = await fetch(`${API_URL}/search?login=${encodeURIComponent(query)}&user_id=${encodeURIComponent(myId)}`);
        if (!response.ok) return;
        const users = await response.json();
        renderSearchResults(users);
    } catch (e) { console.error(e); }
}

function renderSearchResults(users) {
    const listBlock = el('contactsListBlock');
    listBlock.innerHTML = '';
    if (!users.length) { listBlock.innerHTML = '<div class="empty">Никого не найдено</div>'; return; }
    users.forEach(u => addChatElement({ chat_type: 'direct', user_id: u.user_id, title: u.login, unread_count: 0 }));
}

async function createGroup() {
    const title = el('groupTitleInput').value.trim();
    const memberLogins = el('groupMembersInput').value.split(',').map(x => x.trim()).filter(Boolean);
    if (!title) return showError('Введи название группы');
    const myId = Number(sessionStorage.getItem('userId'));
    const response = await fetch(`${API_URL}/groups`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ user_id: myId, title, member_logins: memberLogins })
    });
    if (!response.ok) return showError(await response.text());
    const group = await response.json();
    el('groupTitleInput').value = '';
    el('groupMembersInput').value = '';
    await loadActiveChats();
    openGroupChat(group.chat_id, group.title);
}

async function openDirectChat(userId, title) {
    currentChat = { type: 'direct', userId: Number(userId), title };
    openChatUi(title, true);
    await loadHistory(`${API_URL}/messages?user_id=${encodeURIComponent(sessionStorage.getItem('userId'))}&target_id=${encodeURIComponent(userId)}`);
}

async function openGroupChat(chatId, title) {
    currentChat = { type: 'group', chatId: Number(chatId), title };
    openChatUi(`# ${title}`, false);
    await loadHistory(`${API_URL}/messages?user_id=${encodeURIComponent(sessionStorage.getItem('userId'))}&chat_id=${encodeURIComponent(chatId)}`);
}

function openChatUi(title, canBlock) {
    document.querySelectorAll('.contact-item').forEach(x => x.classList.remove('active'));
    el('activeChatHeader').innerText = title;
    el('chatMessageInput').disabled = false;
    el('chatSendBtn').disabled = false;
    el('blockBtn').disabled = !canBlock;
    el('historySearchInput').disabled = false;
    el('historySearchBtn').disabled = false;
    clearSearchResults();
    el('chatMessageInput').focus();
    el('messagesDisplayBlock').innerHTML = '<div class="empty">Загрузка истории...</div>';
}

async function loadHistory(url) {
    try {
        const response = await fetch(url);
        if (!response.ok) return;
        const messages = await response.json();
        el('messagesDisplayBlock').innerHTML = '';
        const myId = Number(sessionStorage.getItem('userId'));
        messages.forEach(msg => appendMessage(msg, msg.sender_id === myId ? 'my-msg' : 'their-msg'));
        loadActiveChats();
    } catch (e) { console.error('Не удалось подгрузить историю:', e); }
}

function markCurrentRead() {
    if (!currentChat) return;
    const myId = sessionStorage.getItem('userId');
    const url = currentChat.type === 'group'
        ? `${API_URL}/messages?user_id=${encodeURIComponent(myId)}&chat_id=${encodeURIComponent(currentChat.chatId)}`
        : `${API_URL}/messages?user_id=${encodeURIComponent(myId)}&target_id=${encodeURIComponent(currentChat.userId)}`;
    fetch(url).then(() => loadActiveChats()).catch(() => {});
}

function sendChatMessage() {
    const input = el('chatMessageInput');
    const text = input.value.trim();
    if (!text || !currentChat || !ws || ws.readyState !== WebSocket.OPEN) return;
    const packet = currentChat.type === 'group' ? { chat_id: currentChat.chatId, text } : { receiver_id: currentChat.userId, text };
    ws.send(JSON.stringify(packet));
    input.value = '';
}

async function blockCurrentUser() {
    if (!currentChat || currentChat.type !== 'direct') return;
    const ok = confirm(`Заблокировать ${currentChat.title}? Переписка исчезнет из списка, новые сообщения не будут проходить.`);
    if (!ok) return;
    const myId = Number(sessionStorage.getItem('userId'));
    const response = await fetch(`${API_URL}/block`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ user_id: myId, target_id: currentChat.userId }) });
    if (!response.ok) return showError(await response.text());
    currentChat = null;
    el('activeChatHeader').innerText = 'Выберите чат для начала общения';
    el('messagesDisplayBlock').innerHTML = '';
    el('chatMessageInput').value = '';
    el('chatMessageInput').disabled = true;
    el('chatSendBtn').disabled = true;
    el('blockBtn').disabled = true;
    el('historySearchInput').value = '';
    el('historySearchInput').disabled = true;
    el('historySearchBtn').disabled = true;
    clearSearchResults();
    loadActiveChats();
}

async function deleteAccount() {
    const password = prompt('Для удаления аккаунта введи пароль. Личные переписки будут удалены, из групп ты выйдешь.');
    if (!password) return;
    if (!confirm('Точно удалить аккаунт? Отменить нельзя.')) return;
    const myId = Number(sessionStorage.getItem('userId'));
    const response = await fetch(`${API_URL}/account/delete`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ user_id: myId, password }) });
    if (!response.ok) return showError(await response.text());
    sessionStorage.clear();
    if (ws) ws.close();
    location.reload();
}

function appendMessage(msg, type) {
    const display = el('messagesDisplayBlock');
    const row = document.createElement('div');
    row.className = `msg-row ${type}`;
    row.dataset.messageId = msg.id;
    const cloud = document.createElement('div');
    cloud.className = 'msg-cloud';
    if (msg.chat_id && type === 'their-msg' && msg.sender_login) {
        const author = document.createElement('div');
        author.className = 'msg-author';
        author.textContent = msg.sender_login;
        cloud.appendChild(author);
    }
    const text = document.createElement('div');
    text.textContent = msg.text;
    cloud.appendChild(text);

    if (el('linkPreviewToggle').checked) {
        const previews = buildLinkPreviews(msg.text);
        previews.forEach(preview => cloud.appendChild(preview));
    }

    const time = document.createElement('div');
    time.className = 'msg-time';
    time.textContent = formatTime(msg.timestamp);
    cloud.appendChild(time);
    row.appendChild(cloud);
    display.appendChild(row);
    display.scrollTop = display.scrollHeight;
}


async function searchHistory(globalSearch) {
    const query = el('historySearchInput').value.trim();
    if (query.length < 2) return showSearchHint('Минимум 2 символа');
    const myId = sessionStorage.getItem('userId');
    const params = new URLSearchParams({ user_id: myId, q: query });
    if (!globalSearch && currentChat) {
        if (currentChat.type === 'group') params.set('chat_id', currentChat.chatId);
        else params.set('target_id', currentChat.userId);
    }
    try {
        const response = await fetch(`${API_URL}/messages/search?${params.toString()}`);
        if (!response.ok) return showSearchHint(await response.text());
        lastSearchResults = await response.json();
        renderSearchResultsInChat(lastSearchResults, globalSearch);
    } catch (_) {
        showSearchHint('Поиск недоступен');
    }
}

function renderSearchResultsInChat(results, globalSearch) {
    const box = el('historySearchResults');
    box.innerHTML = '';
    box.style.display = 'block';
    if (!results.length) { box.innerHTML = '<div class="empty">Ничего не найдено</div>'; return; }
    results.forEach(item => {
        const div = document.createElement('div');
        div.className = 'search-result-item';
        const title = document.createElement('div');
        title.className = 'search-result-title';
        title.textContent = `${item.chat_type === 'group' ? '# ' : ''}${item.title}`;
        const text = document.createElement('div');
        text.className = 'search-result-text';
        text.textContent = item.message.text;
        const time = document.createElement('div');
        time.className = 'search-result-time';
        time.textContent = formatTime(item.message.timestamp);
        div.appendChild(title); div.appendChild(text); div.appendChild(time);
        div.addEventListener('click', async () => {
            if (globalSearch || !currentChat || (item.chat_type === 'group' && currentChat.chatId !== item.chat_id) || (item.chat_type === 'direct' && currentChat.userId !== item.user_id)) {
                if (item.chat_type === 'group') await openGroupChat(item.chat_id, item.title);
                else await openDirectChat(item.user_id, item.title);
            }
            const row = document.querySelector(`[data-message-id="${item.message.id}"]`);
            if (row) { row.scrollIntoView({ behavior: 'smooth', block: 'center' }); row.classList.add('active'); setTimeout(() => row.classList.remove('active'), 1200); }
        });
        box.appendChild(div);
    });
}

function showSearchHint(text) {
    const box = el('historySearchResults');
    box.style.display = 'block';
    box.innerHTML = '';
    const div = document.createElement('div');
    div.className = 'empty';
    div.textContent = text;
    box.appendChild(div);
}

function clearSearchResults() {
    const box = el('historySearchResults');
    box.innerHTML = '';
    box.style.display = 'none';
}

function savePreviewSetting() {
    localStorage.setItem('linkPreviewEnabled', el('linkPreviewToggle').checked ? '1' : '0');
    if (currentChat) {
        // Перерисовать проще через повторную загрузку текущей истории.
        if (currentChat.type === 'group') openGroupChat(currentChat.chatId, currentChat.title);
        else openDirectChat(currentChat.userId, currentChat.title);
    }
}

function extractUrls(text) {
    const matches = text.match(/https?:\/\/[^\s<>()]+/gi) || [];
    return [...new Set(matches)].slice(0, 3);
}

function buildLinkPreviews(text) {
    return extractUrls(text).map(url => {
        let parsed;
        try { parsed = new URL(url); } catch (_) { return null; }
        const host = parsed.hostname.replace(/^www\./, '');
        const a = document.createElement('a');
        a.className = 'link-preview';
        a.href = url;
        a.target = '_blank';
        a.rel = 'noopener noreferrer';

        const title = document.createElement('div');
        title.className = 'link-preview-title';
        title.textContent = previewTitle(parsed);
        const desc = document.createElement('div');
        desc.className = 'link-preview-desc';
        desc.textContent = previewDescription(parsed);
        const hostEl = document.createElement('div');
        hostEl.className = 'link-preview-host';
        hostEl.textContent = host;
        a.appendChild(title); a.appendChild(desc); a.appendChild(hostEl);
        return a;
    }).filter(Boolean);
}

function previewTitle(url) {
    const host = url.hostname.replace(/^www\./, '');
    if (host.includes('youtube.com') || host === 'youtu.be') return 'YouTube';
    if (host === 'github.com') {
        const parts = url.pathname.split('/').filter(Boolean);
        return parts.length >= 2 ? `GitHub: ${parts[0]}/${parts[1]}` : 'GitHub';
    }
    if (host.includes('x.com') || host.includes('twitter.com')) return 'X / Twitter';
    if (host.includes('reddit.com')) return 'Reddit';
    if (host.includes('t.me')) return 'Telegram link';
    return host;
}

function previewDescription(url) {
    const host = url.hostname.replace(/^www\./, '');
    if (host.includes('youtube.com') || host === 'youtu.be') return 'Видео по ссылке. Превью без внешней загрузки.';
    if (host === 'github.com') return 'Репозиторий или страница GitHub.';
    if (host.includes('x.com') || host.includes('twitter.com')) return 'Пост или профиль.';
    if (host.includes('reddit.com')) return 'Тред или пост Reddit.';
    if (host.includes('t.me')) return 'Telegram-канал, чат или пост.';
    return 'Ссылка. Автозагрузка внешних метаданных отключена ради приватности.';
}

function formatTime(raw) {
    if (!raw) return '';
    if (raw.includes('-') && raw.includes(' ')) return raw.split(' ')[1].slice(0, 5);
    return raw;
}

function notifyNewMessage() {
    if (document.hidden) document.title = 'Новое сообщение — Наш Мессенджер';
}
document.addEventListener('visibilitychange', () => { if (!document.hidden) document.title = 'Наш Мессенджер'; });
