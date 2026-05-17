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

	function setStatus(text, cls) {
		statusEl.textContent = text;
		statusEl.className = 'chat-status' + (cls ? ' ' + cls : '');
	}

	function setIdentity(text) {
		identityEl.textContent = text;
	}

	function formatTime(iso) {
		try {
			const d = new Date(iso);
			const now = new Date();
			const sameDay =
				d.getFullYear() === now.getFullYear() &&
				d.getMonth() === now.getMonth() &&
				d.getDate() === now.getDate();

			if (sameDay) {
				let h = d.getHours();
				const m = String(d.getMinutes()).padStart(2, '0');
				const s = String(d.getSeconds()).padStart(2, '0');
				const ampm = h >= 12 ? 'pm' : 'am';
				h = h % 12;
				if (h === 0) h = 12;
				return `${h}:${m}:${s}${ampm}`;
			} else {
				const yyyy = d.getFullYear();
				const mm = String(d.getMonth() + 1).padStart(2, '0');
				const dd = String(d.getDate()).padStart(2, '0');
				return `${yyyy}-${mm}-${dd}`;
			}
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

	function clearEmpty() {
		const empty = messagesEl.querySelector('.chat-empty');
		if (empty) empty.remove();
	}

	function appendMessage(msg) {
		clearEmpty();
		const p = document.createElement('p');
		p.className = 'chat-msg' + (msg.is_anon ? ' is-anon' : '');
		p.innerHTML =
			`<span class="chat-meta">${formatTime(msg.ts)}</span>` +
			`<span class="chat-user">${escapeHtml(msg.user)}</span>` +
			`<span class="chat-text">${escapeHtml(msg.text)}</span>`;

		// for anon messages, also reveal on click (helps on touch devices)
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
				setIdentity(`logged in as ${me.username}`);
			} else {
				setIdentity('not logged in — messages sent as anon');
			}
		} catch {
			setIdentity('not logged in — messages sent as anon');
		}
	}

	function connect() {
		setStatus('connecting...', '');
		ws = new WebSocket(WS_URL);

		ws.addEventListener('open', () => {
			reconnectAttempts = 0;
			setStatus('connected', 'connected');
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
			setStatus('disconnected. retrying...', 'disconnected');
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
		// server determines the sender from the session cookie now
		ws.send(JSON.stringify({ text }));
		textEl.value = '';
	});

	loadIdentity();
	loadHistory().then(connect);
})();
