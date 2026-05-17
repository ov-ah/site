// Updates the right-side nav based on login state.
// Used on every page that includes the nav.
(async () => {
	const navAuth = document.getElementById('nav-auth');
	if (!navAuth) return;

	const base = `${location.protocol}//${location.host}`;

	function renderLoggedOut() {
		navAuth.innerHTML =
			'<a href="/login">login</a>' +
			'<span style="margin: 0 4px;">·</span>' +
			'<a href="/register">register</a>';
	}

	function renderLoggedIn(username) {
		navAuth.innerHTML = '';
		const userSpan = document.createElement('span');
		userSpan.textContent = username;
		userSpan.style.marginRight = '8px';

		const sep = document.createElement('span');
		sep.textContent = '·';
		sep.style.margin = '0 4px';

		const logoutLink = document.createElement('a');
		logoutLink.href = '#';
		logoutLink.textContent = 'logout';
		logoutLink.addEventListener('click', async (e) => {
			e.preventDefault();
			try {
				await fetch(`${base}/logout`, {
					method: 'POST',
					credentials: 'same-origin',
				});
			} catch { /* ignore */ }
			location.reload();
		});

		navAuth.appendChild(userSpan);
		navAuth.appendChild(sep);
		navAuth.appendChild(logoutLink);
	}

	try {
		const res = await fetch(`${base}/me`, { credentials: 'same-origin' });
		if (res.ok) {
			const me = await res.json();
			renderLoggedIn(me.username);
		} else {
			renderLoggedOut();
		}
	} catch {
		renderLoggedOut();
	}
})();
