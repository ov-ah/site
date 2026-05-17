(() => {
	const messagesEl = document.getElementById('chat-messages');
	const statusEl = document.getElementById('chat-status');
	const identityEl = document.getElementById('chat-identity');
	const formEl = document.getElementById('chat-form');
	const textEl = document.getElementById('chat-text');
	const sendBtn = document.getElementById('chat-send');

	const HTTP_BASE = `${location.protocol}//${location.host}`;
	const WS_URL = `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws`;

	let ws = null;
	let reconnectAttempts = 0;
	let reconnectTimer = null;

	function setStatus(text) {
		statusEl.textContent = text;
	}

	function setIdentity(text) {
		identityEl.textContent = text;
	}

	function formatTime(iso) {
		try {
			const d = new Date(iso);
			let h = d.getHours();
			const m = String(d.getMinutes()).padStart(2, '0');
			const s = String(d.getSeconds()).padStart(2, '0');
			h = String(h).padStart(2, '0');
			return `${h}:${m}:${s}`;
		} catch {
			return '';
		}
	}

	function escapeHtml(s) {
		return s
			.replace(/&/g, '&amp;')
			.replace(/</g, '&lt;')
			.replace(/>/g, '&gt;')
			.replace(/"/g, '&quot;')
			.replace(/'/g, '&#39;');
	}

	// validate a hex color so we don't inject arbitrary CSS
	function safeColor(c) {
		if (typeof c !== 'string') return null;
		return /^#[0-9a-fA-F]{6}$/.test(c) ? c : null;
	}

	function clearEmpty() {
		const empty = messagesEl.querySelector('.chat-empty');
		if (empty) empty.remove();
	}

	function buildUserLabel(msg) {
		// for logged-in users: "username [#42]"
		// for anon: just the anon handle, no UID
		const name = escapeHtml(msg.user);
		const color = safeColor(msg.color);
		const style = color ? ` style="color: ${color}"` : '';
		if (msg.is_anon || msg.uid == null) {
			return `<span class="chat-user"${style}>${name}</span>`;
		}
		return `<span class="chat-user"${style}>${name} [#${msg.uid}]</span>`;
	}

	function buildTextHtml(msg) {
		if (!msg.is_anon) {
			return `<span class="chat-text">${escapeHtml(msg.text)}</span>`;
		}
		// anon: show **** matched to length, real text revealed on hover/click
		const mask = '*'.repeat(Math.min(msg.text.length, 40));
		return `<span class="chat-text">` +
			`<span class="chat-mask">${mask}</span>` +
			`<span class="chat-real">${escapeHtml(msg.text)}</span>` +
			`</span>`;
	}

	function appendMessage(msg) {
		clearEmpty();
		const p = document.createElement('p');
		p.className = 'chat-msg' + (msg.is_anon ? ' is-anon' : '');
		p.innerHTML =
			`<span class="chat-meta">${formatTime(msg.ts)}</span>` +
			buildUserLabel(msg) + ' ' +
			buildTextHtml(msg);

		if (msg.is_anon) {
			const textSpan = p.querySelector('.chat-text');
			textSpan.addEventListener('click', () => {
				p.classList.toggle('revealed');
			});
		}

		messagesEl.appendChild(p);
		const nearBottom = messagesEl.scrollHeight - messagesEl.scrollTop - messagesEl.clientHeight < 80;
		if (nearBottom) messagesEl.scrollTop = messagesEl.scrollHeight;
	}

	async function loadHistory() {
		try {
			const res = await fetch(`${HTTP_BASE}/history`, { credentials: 'same-origin' });
			if (!res.ok) throw new Error('history fetch failed');
			const messages = await res.json();
			messagesEl.innerHTML = '';
			if (messages.length === 0) {
				const empty = document.createElement('p');
				empty.className = 'chat-empty';
				empty.textContent = 'no messages yet. say hi.';
				messagesEl.appendChild(empty);
			} else {
				messages.forEach(appendMessage);
				messagesEl.scrollTop = messagesEl.scrollHeight;
			}
		} catch (err) {
			messagesEl.innerHTML = '';
			const empty = document.createElement('p');
			empty.className = 'chat-empty';
			empty.textContent = 'could not load history.';
			messagesEl.appendChild(empty);
		}
	}

	async function loadIdentity() {
		try {
			const res = await fetch(`${HTTP_BASE}/me`, { credentials: 'same-origin' });
			if (res.ok) {
				const me = await res.json();
				setIdentity(`${me.username} [#${me.uid}]`);
			} else {
				setIdentity('anon');
			}
		} catch {
			setIdentity('anon');
		}
	}

	function connect() {
		setStatus('connecting...');
		ws = new WebSocket(WS_URL);

		ws.addEventListener('open', () => {
			reconnectAttempts = 0;
			setStatus('connected');
			sendBtn.disabled = false;
		});

		ws.addEventListener('message', (ev) => {
			try {
				const msg = JSON.parse(ev.data);
				appendMessage(msg);
			} catch {
				// ignore malformed
			}
		});

		ws.addEventListener('close', () => {
			sendBtn.disabled = true;
			setStatus('disconnected. retrying...');
			scheduleReconnect();
		});

		ws.addEventListener('error', () => {
			// close handler will follow
		});
	}

	function scheduleReconnect() {
		if (reconnectTimer) return;
		const delay = Math.min(1000 * Math.pow(2, reconnectAttempts), 15000);
		reconnectAttempts += 1;
		reconnectTimer = setTimeout(() => {
			reconnectTimer = null;
			connect();
		}, delay);
	}

	formEl.addEventListener('submit', (e) => {
		e.preventDefault();
		const text = textEl.value.trim();
		if (!text) return;
		if (!ws || ws.readyState !== WebSocket.OPEN) return;
		ws.send(JSON.stringify({ text }));
		textEl.value = '';
	});

	loadIdentity();
	loadHistory().then(connect);
})();
