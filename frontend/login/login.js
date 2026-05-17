(() => {
	const formEl = document.getElementById('login-form');
	const userEl = document.getElementById('login-username');
	const passEl = document.getElementById('login-password');
	const msgEl = document.getElementById('login-message');

	const base = `${location.protocol}//${location.host}`;

	function setMessage(text, isError) {
		msgEl.textContent = text;
		msgEl.className = 'auth-message' + (isError ? ' error' : '');
	}

	formEl.addEventListener('submit', async (e) => {
		e.preventDefault();
		setMessage('logging in...', false);

		const username = userEl.value.trim();
		const password = passEl.value;
		if (!username || !password) {
			setMessage('username and password required', true);
			return;
		}

		try {
			const res = await fetch(`${base}/login`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				credentials: 'same-origin',
				body: JSON.stringify({ username, password }),
			});

			if (res.ok) {
				setMessage('logged in. redirecting...', false);
				location.assign('/chat');
				return;
			}

			const errText = await res.text();
			setMessage(errText || 'login failed', true);
		} catch (err) {
			setMessage('network error', true);
		}
	});
})();
