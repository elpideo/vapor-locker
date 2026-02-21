(() => {
  'use strict';

  const $ = (id) => document.getElementById(id);

  const netStatus = $('netStatus');

  const getForm = $('getForm');
  const getKey = $('get_key');
  const getMessage = $('getMessage');
  const getMessageText = $('getMessageText');
  const getValue = $('getValue');
  const resultDisplay = $('resultDisplay');
  const resultPlain = $('resultPlain');
  const revealBtn = $('revealBtn');
  const copyBtn = $('copyBtn');

  const setForm = $('setForm');
  const setKey = $('set_key');
  const setValue = $('set_value');
  const setEphemeral = $('ephemeral');
  const setMessage = $('setMessage');
  const setMessageText = $('setMessageText');
  const setStoredLink = $('setStoredLink');
  const setStoredLinkAnchor = $('setStoredLinkAnchor');
  const setStoredLinkCopyBtn = $('setStoredLinkCopyBtn');
  const keyRandomBtn = $('keyRandomBtn');
  const keyCopyBtn = $('keyCopyBtn');

  let csrfToken = null;
  let revealed = false;
  let salts = null; // base64url strings, most recent first

  const ITERATIONS = 200000;
  const textEncoder = new TextEncoder();
  const textDecoder = new TextDecoder();

  function setNetStatus(text) {
    if (!netStatus) return;
    netStatus.textContent = text;
  }

  function showGetMessage(text) {
    getValue.classList.add('hidden');
    getMessage.classList.remove('hidden');
    getMessageText.textContent = text;
  }

  function showGetValue(value) {
    getMessage.classList.add('hidden');
    getValue.classList.remove('hidden');

    resultPlain.textContent = (value || '').replace(/\r\n/g, '\n');
    resultDisplay.textContent = '*******';
    revealed = false;
    revealBtn.classList.remove('revealed');
    revealBtn.setAttribute('aria-pressed', 'false');
    revealBtn.setAttribute('aria-label', 'Afficher');
  }

  function hideGetResult() {
    getMessage.classList.add('hidden');
    getValue.classList.add('hidden');
  }

  function showSetMessage(text) {
    setMessage.classList.remove('hidden');
    setMessageText.textContent = text;
  }

  function hideSetMessage() {
    setMessage.classList.add('hidden');
  }

  const STORED_LINK_BASE = 'https://vapor-locker.com';

  function showStoredLink(key) {
    if (!setStoredLink || !setStoredLinkAnchor) return;
    const link = STORED_LINK_BASE + '?key=' + encodeURIComponent(key);
    setStoredLinkAnchor.href = link;
    setStoredLinkAnchor.textContent = link;
    setStoredLink.classList.remove('hidden');
  }

  function hideStoredLink() {
    if (setStoredLink) setStoredLink.classList.add('hidden');
  }

  function fadeOutAndClearValue() {
    if (!setValue) return;
    setValue.classList.add('valueFadeOut');
    window.setTimeout(() => {
      setValue.value = '';
      setValue.classList.remove('valueFadeOut');
    }, 420);
  }

  function generateRandomKey(length) {
    const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    const arr = new Uint8Array(length);
    crypto.getRandomValues(arr);
    return Array.from(arr, (c) => chars[c % chars.length]).join('');
  }

  function copyText(text) {
    if (navigator && navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text);
    }
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'absolute';
    ta.style.left = '-9999px';
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand('copy'); } catch (e) {}
    document.body.removeChild(ta);
    return Promise.resolve();
  }

  async function fetchJson(url, opts) {
    const res = await fetch(url, Object.assign({ credentials: 'same-origin' }, opts || {}));
    let data = null;
    try {
      data = await res.json();
    } catch (e) {
      // ignore
    }
    return { res, data };
  }

  async function ensureCsrf() {
    if (csrfToken) return csrfToken;
    setNetStatus('Chargement CSRF…');
    const { res, data } = await fetchJson('/api/csrf', { method: 'GET' });
    if (!res.ok || !data || !data.csrf) {
      setNetStatus('CSRF indisponible');
      return null;
    }
    csrfToken = data.csrf;
    setNetStatus('Prêt');
    return csrfToken;
  }

  function b64urlEncode(u8) {
    let bin = '';
    for (let i = 0; i < u8.length; i += 0x8000) {
      bin += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
    }
    return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
  }

  function b64urlDecode(s) {
    const b64 = (s || '').replace(/-/g, '+').replace(/_/g, '/');
    const padded = b64 + '==='.slice((b64.length + 3) % 4);
    const bin = atob(padded);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  function concatBytes(a, b) {
    const out = new Uint8Array(a.length + b.length);
    out.set(a, 0);
    out.set(b, a.length);
    return out;
  }

  async function sha256(bytes) {
    const hash = await crypto.subtle.digest('SHA-256', bytes);
    return new Uint8Array(hash);
  }

  async function deriveForSalt(key, saltB64url) {
    const saltBytes = b64urlDecode(saltB64url);
    const keyMaterial = await crypto.subtle.importKey(
      'raw',
      textEncoder.encode(key),
      { name: 'PBKDF2' },
      false,
      ['deriveBits']
    );
    const baseBits = await crypto.subtle.deriveBits(
      { name: 'PBKDF2', hash: 'SHA-256', salt: saltBytes, iterations: ITERATIONS },
      keyMaterial,
      256
    );
    const baseKey = new Uint8Array(baseBits);

    const lookupPrefix = textEncoder.encode('lookup');
    const encPrefix = textEncoder.encode('enc');
    const lookupHashBytes = await sha256(concatBytes(lookupPrefix, baseKey));
    const encKeyBytes = await sha256(concatBytes(encPrefix, baseKey));

    const aesKey = await crypto.subtle.importKey('raw', encKeyBytes, { name: 'AES-GCM' }, false, [
      'encrypt',
      'decrypt'
    ]);

    return { salt: saltB64url, lookupHash: b64urlEncode(lookupHashBytes), aesKey };
  }

  async function encryptValue(aesKey, plaintext) {
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const pt = textEncoder.encode((plaintext || '').replace(/\r\n/g, '\n'));
    const ctBuf = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, aesKey, pt);
    const ct = new Uint8Array(ctBuf);
    return { v: 1, iv: b64urlEncode(iv), ct: b64urlEncode(ct) };
  }

  async function decryptValue(aesKey, valueEnc) {
    const iv = b64urlDecode(valueEnc.iv || '');
    const ct = b64urlDecode(valueEnc.ct || '');
    const ptBuf = await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, aesKey, ct);
    return textDecoder.decode(ptBuf);
  }

  async function ensureSalts() {
    if (salts && salts.length) return salts;
    setNetStatus('Chargement des sels…');
    const { res, data } = await fetchJson('/api/salts', { method: 'GET' });
    if (!res.ok || !data || !Array.isArray(data.salts) || !data.salts.length) {
      setNetStatus('Sels indisponibles');
      return null;
    }
    salts = data.salts;
    setNetStatus('Prêt');
    return salts;
  }

  revealBtn.addEventListener('click', () => {
    if (getValue.classList.contains('hidden')) return;
    if (!resultPlain.textContent) return;

    if (revealed) {
      resultDisplay.textContent = '*******';
      revealed = false;
      revealBtn.classList.remove('revealed');
      revealBtn.setAttribute('aria-pressed', 'false');
      revealBtn.setAttribute('aria-label', 'Afficher');
    } else {
      resultDisplay.textContent = resultPlain.textContent.replace(/\r\n/g, '\n');
      revealed = true;
      revealBtn.classList.add('revealed');
      revealBtn.setAttribute('aria-pressed', 'true');
      revealBtn.setAttribute('aria-label', 'Masquer');
    }
  });

  if (keyRandomBtn) {
    keyRandomBtn.addEventListener('click', () => {
      setKey.value = generateRandomKey(64);
    });
  }
  if (keyCopyBtn) {
    keyCopyBtn.addEventListener('click', () => {
      const text = (setKey.value || '').trim();
      if (!text) return;
      copyText(text).then(() => {
        keyCopyBtn.classList.add('copied');
        window.setTimeout(() => keyCopyBtn.classList.remove('copied'), 900);
      });
    });
  }

  copyBtn.addEventListener('click', () => {
    if (getValue.classList.contains('hidden')) return;
    const text = (resultPlain.textContent || '').replace(/\r\n/g, '\n');
    if (!text) return;
    copyText(text).then(() => {
      copyBtn.classList.add('copied');
      window.setTimeout(() => copyBtn.classList.remove('copied'), 900);
    });
  });

  getForm.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    hideSetMessage();
    hideGetResult();

    const key = (getKey.value || '').trim();
    if (!key) {
      showGetMessage('Validation error: key required');
      return;
    }

    if (!window.crypto || !crypto.subtle) {
      showGetMessage('WebCrypto indisponible sur ce navigateur');
      return;
    }

    const s = await ensureSalts();
    if (!s) {
      showGetMessage('Sels indisponibles');
      return;
    }

    setNetStatus('Recherche…');
    const derivations = [];
    for (let i = 0; i < s.length; i++) {
      derivations.push(await deriveForSalt(key, s[i]));
    }
    const hashes = derivations.map((d) => d.lookupHash);
    const { res, data } = await fetchJson('/api/get', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ hashes })
    });
    if (!res.ok) {
      const msg = (data && data.error) ? data.error : ('HTTP ' + res.status);
      showGetMessage(msg);
      setNetStatus('Prêt');
      return;
    }

    if (data && data.found) {
      // Try decrypt with each derived key (most recent salt first).
      const valueEnc = data.value;
      let decrypted = null;
      for (let i = 0; i < derivations.length; i++) {
        try {
          decrypted = await decryptValue(derivations[i].aesKey, valueEnc);
          break;
        } catch (e) {
          // continue
        }
      }
      if (decrypted !== null) {
        showGetValue(decrypted);
      } else {
        showGetMessage('Déchiffrement impossible (clé incorrecte ?)');
      }
    } else if (data && data.error) {
      showGetMessage(data.error);
    } else {
      showGetMessage('Not found');
    }
    setNetStatus('Prêt');
  });

  setForm.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    hideGetResult();
    hideSetMessage();
    hideStoredLink();

    const token = await ensureCsrf();
    if (!token) {
      showSetMessage('CSRF init failed');
      return;
    }

    const key = (setKey.value || '').trim();
    const value = (setValue.value || '');
    const ephemeral = !!setEphemeral.checked;
    if (!key) {
      showSetMessage('Validation error: key required');
      return;
    }

    if (!window.crypto || !crypto.subtle) {
      showSetMessage('WebCrypto indisponible sur ce navigateur');
      return;
    }

    const s = await ensureSalts();
    if (!s) {
      showSetMessage('Sels indisponibles');
      return;
    }
    const mostRecentSalt = s[0];

    setNetStatus('Enregistrement…');
    let derivation = null;
    let valueEnc = null;
    try {
      derivation = await deriveForSalt(key, mostRecentSalt);
      valueEnc = await encryptValue(derivation.aesKey, value);
    } catch (e) {
      showSetMessage('Erreur chiffrement');
      setNetStatus('Prêt');
      return;
    }

    const body = JSON.stringify({ key_hash: derivation.lookupHash, value: valueEnc, ephemeral, csrf: token });
    const { res, data } = await fetchJson('/api/set', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body
    });

    if (!res.ok) {
      // if CSRF mismatch, try refresh once
      if (res.status === 403) {
        csrfToken = null;
        const refreshed = await ensureCsrf();
        if (refreshed) {
          setNetStatus('Réessai…');
          const retry = await fetchJson('/api/set', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ key, value, ephemeral, csrf: refreshed })
          });
          if (retry.res.ok && retry.data && retry.data.ok) {
            fadeOutAndClearValue();
            showStoredLink(key);
            setNetStatus('Prêt');
            return;
          }
          const m2 = (retry.data && retry.data.error) ? retry.data.error : ('HTTP ' + retry.res.status);
          showSetMessage(m2);
          setNetStatus('Prêt');
          return;
        }
      }
      const msg = (data && data.error) ? data.error : ('HTTP ' + res.status);
      showSetMessage(msg);
      setNetStatus('Prêt');
      return;
    }

    if (data && data.ok) {
      fadeOutAndClearValue();
      showStoredLink(key);
    } else if (data && data.error) {
      showSetMessage(data.error);
    } else {
      fadeOutAndClearValue();
      showStoredLink(key);
    }
    setNetStatus('Prêt');
  });

  if (setStoredLinkCopyBtn && setStoredLinkAnchor) {
    setStoredLinkCopyBtn.addEventListener('click', () => {
      const link = (setStoredLinkAnchor.href || '').trim();
      if (!link) return;
      copyText(link).then(() => {
        setStoredLinkCopyBtn.classList.add('copied');
        window.setTimeout(() => setStoredLinkCopyBtn.classList.remove('copied'), 900);
      });
    });
  }

  // boot: fill RETRIEVE key from URL ?key=...
  (function initFromUrl() {
    try {
      const params = new URLSearchParams(window.location.search);
      const key = params.get('key');
      if (key && getKey) getKey.value = key;
    } catch (e) {}
  })();

  // boot
  Promise.resolve()
    .then(() => ensureCsrf())
    .then(() => ensureSalts())
    .catch(() => setNetStatus('Prêt'));
})();

