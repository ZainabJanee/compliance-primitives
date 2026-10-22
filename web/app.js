(() => {
  const input = document.querySelector('[data-search]');
  const items = [...document.querySelectorAll('[data-search-item]')];
  if (!input || items.length === 0) return;
  input.addEventListener('input', () => {
    const query = input.value.trim().toLowerCase();
    let visible = 0;
    items.forEach((item) => {
      const match = !query || item.textContent.toLowerCase().includes(query);
      item.hidden = !match;
      if (match) visible += 1;
    });
    const empty = document.querySelector('[data-search-empty]');
    if (empty) empty.hidden = visible !== 0;
  });
})();

// Copy-to-clipboard for .code-wrap blocks
(() => {
  document.querySelectorAll('.code-wrap').forEach((wrap) => {
    const btn = wrap.querySelector('.copy-btn');
    const pre = wrap.querySelector('pre');
    if (!btn || !pre) return;
    btn.addEventListener('click', () => {
      navigator.clipboard.writeText(pre.innerText).then(() => {
        btn.textContent = 'Copied!';
        btn.setAttribute('data-copied', 'true');
        setTimeout(() => {
          btn.textContent = 'Copy';
          btn.removeAttribute('data-copied');
        }, 2000);
      }).catch(() => {
        btn.textContent = 'Failed';
        setTimeout(() => { btn.textContent = 'Copy'; }, 2000);
      });
    });
  });
})();
