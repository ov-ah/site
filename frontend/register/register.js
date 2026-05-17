(() => {
	const formEl = document.getElementById('register-form');
	const userEl = document.getElementById('register-username');
	const passEl = document.getElementById('register-password');
	const msgEl = document.getElementById('register-message');

	const base = `${location.protocol}//${location.host}`;

	function setMessage(text, isError) {
		msgEl.textContent = text;
		msgEl.className = 'auth-message' + (isError ? ' error' : '');
	}

	formEl.addEventListener('submit', async (e) => {
		e.preventDefault();
		setMessage('registering...', false);

		const username = userEl.value.trim();
		const password = passEl.value;
		if (!username || !password) {
			setMessage('username and password required', true);
			return;
		}

		try {
			const res = await fetch(`${base}/register`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				credentials: 'same-origin',
				body: JSON.stringify({ username, password }),
			});

			if (res.ok) {
				setMessage('registered. redirecting...', false);
				location.assign('/chat');
				return;
			}

			const errText = await res.text();
			setMessage(errText || 'registration failed', true);
		} catch (err) {
			setMessage('network error', true);
		}
	});
})();
