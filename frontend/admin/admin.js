(() => {
	const base = `${location.protocol}//${location.host}`;

	// ---- auth guard ----

	async function init() {
		const res = await fetch(`${base}/me`, { credentials: 'same-origin' });
		if (!res.ok) { location.assign('/login'); return; }
		const me = await res.json();
		if (!me.is_admin) { location.assign('/'); return; }
		setupTabs();
		loadUsers();
	}

	// ---- tabs ----

	function setupTabs() {
		document.querySelectorAll('.admin-tab').forEach(btn => {
			btn.addEventListener('click', () => {
				document.querySelectorAll('.admin-tab').forEach(b => b.classList.remove('active'));
				document.querySelectorAll('.tab-content').forEach(t => t.style.display = 'none');
				btn.classList.add('active');
				const tab = btn.dataset.tab;
				document.getElementById(`tab-${tab}`).style.display = '';
				if (tab === 'users') loadUsers();
				if (tab === 'ips') loadIps();
				if (tab === 'messages') loadMessages();
			});
		});
	}

	// ---- helpers ----

	function escapeHtml(s) {
		return String(s)
			.replace(/&/g, '&amp;')
			.replace(/</g, '&lt;')
			.replace(/>/g, '&gt;')
			.replace(/"/g, '&quot;');
	}

	function formatTs(ts) {
		try {
			const d = new Date(ts);
			return d.toLocaleDateString() + ' ' +
				String(d.getHours()).padStart(2, '0') + ':' +
				String(d.getMinutes()).padStart(2, '0') + ':' +
				String(d.getSeconds()).padStart(2, '0');
		} catch { return ts; }
	}

	function setStatus(id, text) {
		const el = document.getElementById(id);
		if (el) el.textContent = text;
	}

	// ---- users ----

	async function loadUsers() {
		setStatus('users-status', 'loading...');
		document.getElementById('users-table').style.display = 'none';

		const res = await fetch(`${base}/admin/api/users`, { credentials: 'same-origin' });
		if (!res.ok) { setStatus('users-status', 'failed to load users.'); return; }
		const users = await res.json();

		setStatus('users-status', '');
		renderUsers(users);
	}

	function renderUsers(users) {
		const tbody = document.getElementById('users-tbody');
		tbody.innerHTML = '';

		for (const u of users) {
			const tr = document.createElement('tr');

			const colorId = `color-${u.uid}`;
			const adminLabel = u.is_admin ? 'yes' : 'no';
			const ipCell = u.last_ip
				? `<a class="admin-btn ip-link" data-ip="${escapeHtml(u.last_ip)}">${escapeHtml(u.last_ip)}</a>`
				: '—';

			tr.innerHTML = `
				<td>#${u.uid}</td>
				<td>${escapeHtml(u.username)}</td>
				<td><input type="color" class="color-picker" id="${colorId}" value="${escapeHtml(u.chat_color)}" data-uid="${u.uid}" data-original="${escapeHtml(u.chat_color)}"></td>
				<td><button class="admin-btn admin-toggle-btn" data-uid="${u.uid}" data-admin="${u.is_admin}">${adminLabel}</button></td>
				<td>${u.message_count}</td>
				<td>${ipCell}</td>
			`;
			tbody.appendChild(tr);
		}

		document.getElementById('users-table').style.display = '';

		// color picker — save on change
		tbody.querySelectorAll('.color-picker').forEach(input => {
			input.addEventListener('change', async () => {
				const uid = input.dataset.uid;
				const color = input.value;
				const ok = await setColor(uid, color);
				if (!ok) input.value = input.dataset.original;
				else input.dataset.original = color;
			});
		});

		// admin toggle
		tbody.querySelectorAll('.admin-toggle-btn').forEach(btn => {
			btn.addEventListener('click', async () => {
				const uid = btn.dataset.uid;
				const current = btn.dataset.admin === 'true';
				const ok = await setAdmin(uid, !current);
				if (ok) {
					btn.dataset.admin = String(!current);
					btn.textContent = !current ? 'yes' : 'no';
				}
			});
		});

		// ip link → jump to messages filtered by ip
		tbody.querySelectorAll('.ip-link').forEach(link => {
			link.addEventListener('click', () => {
				filterMessagesByIp(link.dataset.ip);
			});
		});
	}

	async function setColor(uid, color) {
		const res = await fetch(`${base}/admin/api/users/${uid}/color`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			credentials: 'same-origin',
			body: JSON.stringify({ color }),
		});
		return res.ok;
	}

	async function setAdmin(uid, isAdmin) {
		const res = await fetch(`${base}/admin/api/users/${uid}/admin`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			credentials: 'same-origin',
			body: JSON.stringify({ is_admin: isAdmin }),
		});
		return res.ok;
	}

	// ---- ips ----

	async function loadIps() {
		setStatus('ips-status', 'loading...');
		document.getElementById('ips-table').style.display = 'none';

		const res = await fetch(`${base}/admin/api/ips`, { credentials: 'same-origin' });
		if (!res.ok) { setStatus('ips-status', 'failed to load ips.'); return; }
		const ips = await res.json();

		setStatus('ips-status', ips.length === 0 ? 'no data yet.' : '');
		renderIps(ips);
	}

	function renderIps(ips) {
		if (ips.length === 0) return;
		const tbody = document.getElementById('ips-tbody');
		tbody.innerHTML = '';

		for (const entry of ips) {
			const tr = document.createElement('tr');
			const multi = entry.users.length > 1;
			const usersHtml = entry.users.length === 0
				? '<span style="opacity:0.4">anon only</span>'
				: entry.users.map(u => escapeHtml(u)).join(', ') + (multi ? ' <span class="multi-ip">[!]</span>' : '');

			const ipLink = `<a class="admin-btn ip-link" data-ip="${escapeHtml(entry.ip)}">${escapeHtml(entry.ip)}</a>`;
			const lastSeen = entry.last_seen ? formatTs(entry.last_seen) : '—';

			tr.innerHTML = `
				<td>${ipLink}</td>
				<td>${usersHtml}</td>
				<td>${entry.message_count}</td>
				<td>${lastSeen}</td>
			`;
			tbody.appendChild(tr);
		}

		document.getElementById('ips-table').style.display = '';

		tbody.querySelectorAll('.ip-link').forEach(link => {
			link.addEventListener('click', () => {
				filterMessagesByIp(link.dataset.ip);
			});
		});
	}

	// ---- messages ----

	let currentMsgFilter = { user: null, ip: null };

	function filterMessagesByIp(ip) {
		document.querySelectorAll('.admin-tab').forEach(b => b.classList.remove('active'));
		document.querySelectorAll('.tab-content').forEach(t => t.style.display = 'none');
		document.querySelector('[data-tab="messages"]').classList.add('active');
		document.getElementById('tab-messages').style.display = '';

		document.getElementById('filter-ip').value = ip;
		document.getElementById('filter-user').value = '';
		loadMessages({ ip });
	}

	async function loadMessages(filter = currentMsgFilter) {
		currentMsgFilter = filter;
		setStatus('messages-status', 'loading...');
		document.getElementById('messages-table').style.display = 'none';

		const params = new URLSearchParams();
		if (filter.user) params.set('user', filter.user);
		if (filter.ip) params.set('ip', filter.ip);

		const res = await fetch(`${base}/admin/api/messages?${params}`, { credentials: 'same-origin' });
		if (!res.ok) { setStatus('messages-status', 'failed to load messages.'); return; }
		const messages = await res.json();

		setStatus('messages-status', messages.length === 0 ? 'no messages.' : '');
		renderMessages(messages);
	}

	function renderMessages(messages) {
		if (messages.length === 0) return;
		const tbody = document.getElementById('messages-tbody');
		tbody.innerHTML = '';

		for (const m of messages) {
			const tr = document.createElement('tr');
			tr.dataset.id = m.id;

			const userLabel = m.is_anon
				? `<span style="opacity:0.5">${escapeHtml(m.user)}</span>`
				: `${escapeHtml(m.user)}${m.uid != null ? ` <span style="opacity:0.4">[#${m.uid}]</span>` : ''}`;

			const ipCell = m.ip
				? `<a class="admin-btn ip-link" data-ip="${escapeHtml(m.ip)}">${escapeHtml(m.ip)}</a>`
				: '—';

			tr.innerHTML = `
				<td style="white-space:nowrap">${formatTs(m.ts)}</td>
				<td style="white-space:nowrap">${userLabel}</td>
				<td style="white-space:nowrap">${ipCell}</td>
				<td><span class="msg-text" title="${escapeHtml(m.text)}">${escapeHtml(m.text)}</span></td>
				<td><button class="admin-btn danger del-btn">del</button></td>
			`;
			tbody.appendChild(tr);
		}

		document.getElementById('messages-table').style.display = '';

		tbody.querySelectorAll('.del-btn').forEach(btn => {
			btn.addEventListener('click', async () => {
				const tr = btn.closest('tr');
				const id = tr.dataset.id;
				if (!confirm('delete this message?')) return;
				const ok = await deleteMessage(id);
				if (ok) tr.remove();
			});
		});

		tbody.querySelectorAll('.ip-link').forEach(link => {
			link.addEventListener('click', () => {
				filterMessagesByIp(link.dataset.ip);
			});
		});
	}

	async function deleteMessage(id) {
		const res = await fetch(`${base}/admin/api/messages/${id}`, {
			method: 'DELETE',
			credentials: 'same-origin',
		});
		return res.ok;
	}

	// ---- filter controls ----

	document.getElementById('filter-apply').addEventListener('click', () => {
		const user = document.getElementById('filter-user').value.trim() || null;
		const ip = document.getElementById('filter-ip').value.trim() || null;
		loadMessages({ user, ip });
	});

	document.getElementById('filter-clear').addEventListener('click', () => {
		document.getElementById('filter-user').value = '';
		document.getElementById('filter-ip').value = '';
		loadMessages({ user: null, ip: null });
	});

	['filter-user', 'filter-ip'].forEach(id => {
		document.getElementById(id).addEventListener('keydown', e => {
			if (e.key === 'Enter') document.getElementById('filter-apply').click();
		});
	});

	init();
})();
