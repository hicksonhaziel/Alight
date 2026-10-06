try { document.documentElement.dataset.theme = localStorage.getItem('alight-theme') === 'light' ? 'light' : 'dark'; } catch { /* Default remains available without browser storage. */ }
