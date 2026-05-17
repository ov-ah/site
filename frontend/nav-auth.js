(async () => {
	const navAuth = document.getElementById('nav-auth');
	if (!navAuth) return;

	const base = `${location.protocol}//${location.host}`;

	function renderLoggedOut() {
		navAuth.innerHTML = '<a href="/login">login</a> · <a href="/register">register</a>';
	}

	function renderLoggedIn(me) {
		const userSpan = document.createElement('span');
		userSpan.textContent = me.username;

		const logoutLink = document.createElement('a');
		logoutLink.href = '#';
		logoutLink.textContent = 'logout';
		logoutLink.addEventListener('click', async (e) => {
			e.preventDefault();
			try {
				await fetch(`${base}/logout`, { method: 'POST', credentials: 'same-origin' });
			} catch {}
			location.reload();
		});

		const nodes = [userSpan, document.createTextNode(' · '), logoutLink];

		if (me.is_admin) {
			const adminLink = document.createElement('a');
			adminLink.href = '/admin';
			adminLink.textContent = 'admin';
			nodes.push(document.createTextNode(' · '), adminLink);
		}

		navAuth.replaceChildren(...nodes);
	}

	try {
		const res = await fetch(`${base}/me`, { credentials: 'same-origin' });
		if (res.ok) {
			renderLoggedIn(await res.json());
		} else {
			renderLoggedOut();
		}
	} catch {
		renderLoggedOut();
	}
})();
