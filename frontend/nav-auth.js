(async () => {
	const navAuth = document.getElementById('nav-auth');
	if (!navAuth) return;

	const base = `${location.protocol}//${location.host}`;

	function renderLoggedOut() {
		navAuth.innerHTML = '<a href="/login">login</a> · <a href="/register">register</a>';
	}

	function renderLoggedIn(username) {
		const userSpan = document.createElement('span');
		userSpan.textContent = username;

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

		navAuth.replaceChildren(userSpan, document.createTextNode(' · '), logoutLink);
	}

	try {
		const res = await fetch(`${base}/me`, { credentials: 'same-origin' });
		if (res.ok) {
			renderLoggedIn((await res.json()).username);
		} else {
			renderLoggedOut();
		}
	} catch {
		renderLoggedOut();
	}
})();
