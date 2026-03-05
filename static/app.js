(() => {
  'use strict';

  const $ = (id) => document.getElementById(id);

  const netStatus = $('netStatus');

  const getForm = $('getForm');
  const getKey = $('get_key');
  const getMessage = $('getMessage');
  const getMessageText = $('getMessageText');
  const getValue = $('getValue');
  const getTtl = $('getTtl');
  const getTtlText = $('getTtlText');
  const resultDisplay = $('resultDisplay');
  const resultPlain = $('resultPlain');
  const retrieveEphemeralIcon = $('retrieveEphemeralIcon');
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
  const setStoredCountdown = $('setStoredCountdown');
  const keyRandomBtn = $('keyRandomBtn');
  const keyCopyBtn = $('keyCopyBtn');
  const appVersionEl = $('appVersion');

  let csrfToken = null;
  let revealed = false;
  let salts = null; // base64url strings, most recent first
  let storeCountdownTimer = null;
  let retrieveCountdownTimer = null;
  let ephemeralEvaporateTimeout = null;
  let ephemeralEvaporateCleanupTimeout = null;

  const ITERATIONS = 200000;
  const textEncoder = new TextEncoder();
  const textDecoder = new TextDecoder();
  const COUNTDOWN_PREFIX = 'evaporating in ';

  function clearEphemeralEvaporationTimers() {
    if (ephemeralEvaporateTimeout) {
      window.clearTimeout(ephemeralEvaporateTimeout);
      ephemeralEvaporateTimeout = null;
    }
    if (ephemeralEvaporateCleanupTimeout) {
      window.clearTimeout(ephemeralEvaporateCleanupTimeout);
      ephemeralEvaporateCleanupTimeout = null;
    }
  }

  function setNetStatus(text) {
    if (!netStatus) return;
    netStatus.textContent = text;
  }

  function showGetMessage(text) {
    getValue.classList.add('hidden');
    if (getTtl) getTtl.classList.add('hidden');
    if (retrieveCountdownTimer) {
      window.clearInterval(retrieveCountdownTimer);
      retrieveCountdownTimer = null;
    }
    clearEphemeralEvaporationTimers();
    if (getTtlText) getTtlText.classList.remove('ttlEvaporateOut');
    getMessage.classList.remove('hidden');
    getMessageText.textContent = text;
  }

  function showGetValue(value, isEphemeral) {
    getMessage.classList.add('hidden');
    getValue.classList.remove('hidden');
    if (getTtl) getTtl.classList.add('hidden');
    if (getTtlText) getTtlText.textContent = '';
    if (retrieveCountdownTimer) {
      window.clearInterval(retrieveCountdownTimer);
      retrieveCountdownTimer = null;
    }
    clearEphemeralEvaporationTimers();
    if (getTtlText) getTtlText.classList.remove('ttlEvaporateOut');

    if (retrieveEphemeralIcon) {
      if (isEphemeral) {
        retrieveEphemeralIcon.classList.remove('hidden');
      } else {
        retrieveEphemeralIcon.classList.add('hidden');
      }
    }

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
    if (retrieveEphemeralIcon) retrieveEphemeralIcon.classList.add('hidden');
    if (getTtl) getTtl.classList.add('hidden');
    if (getTtlText) getTtlText.textContent = '';
    if (retrieveCountdownTimer) {
      window.clearInterval(retrieveCountdownTimer);
      retrieveCountdownTimer = null;
    }
    clearEphemeralEvaporationTimers();
    if (getTtlText) getTtlText.classList.remove('ttlEvaporateOut');
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
    if (storeCountdownTimer) {
      window.clearInterval(storeCountdownTimer);
      storeCountdownTimer = null;
    }
    if (setStoredCountdown) setStoredCountdown.textContent = '';
  }

  function fadeOutAndClearValue() {
    if (!setValue) return;
    setValue.classList.add('valueFadeOut');
    window.setTimeout(() => {
      setValue.value = '';
      setValue.classList.remove('valueFadeOut');
    }, 420);
  }

  function formatSecondsToHms(totalSeconds) {
    const s = Math.max(0, Math.floor(totalSeconds || 0));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const ss = s % 60;
    const pad2 = (n) => String(n).padStart(2, '0');
    return String(h).padStart(2, '0') + ':' + pad2(m) + ':' + pad2(ss);
  }

  function startStoreCountdown() {
    if (!setStoredCountdown) return;
    if (storeCountdownTimer) window.clearInterval(storeCountdownTimer);
    const deadline = Date.now() + 24 * 60 * 60 * 1000;
    const tick = () => {
      const remainingMs = deadline - Date.now();
      const remainingSecs = Math.max(0, Math.floor((remainingMs + 999) / 1000));
      setStoredCountdown.textContent = COUNTDOWN_PREFIX + formatSecondsToHms(remainingSecs);
      if (remainingSecs <= 0 && storeCountdownTimer) {
        window.clearInterval(storeCountdownTimer);
        storeCountdownTimer = null;
      }
    };
    tick();
    storeCountdownTimer = window.setInterval(tick, 1000);
  }

  function startRetrieveCountdown(ttlSeconds) {
    if (!getTtl || !getTtlText) return;
    if (retrieveCountdownTimer) window.clearInterval(retrieveCountdownTimer);
    getTtl.classList.remove('hidden');
    const deadline = Date.now() + Math.max(0, Math.floor(ttlSeconds || 0)) * 1000;
    const tick = () => {
      const remainingMs = deadline - Date.now();
      const remainingSecs = Math.max(0, Math.floor((remainingMs + 999) / 1000));
      getTtlText.textContent = COUNTDOWN_PREFIX + formatSecondsToHms(remainingSecs);
      if (remainingSecs <= 0 && retrieveCountdownTimer) {
        window.clearInterval(retrieveCountdownTimer);
        retrieveCountdownTimer = null;
      }
    };
    tick();
    retrieveCountdownTimer = window.setInterval(tick, 1000);
  }

  function animateEphemeralCountdown() {
    if (!getTtl || !getTtlText) return;
    if (retrieveCountdownTimer) window.clearInterval(retrieveCountdownTimer);
    clearEphemeralEvaporationTimers();
    getTtlText.classList.remove('ttlEvaporateOut');
    getTtl.classList.remove('hidden');
    const start = 24 * 60 * 60 - 1; // 23:59:59
    const steps = 10;
    const stepMs = 100;
    let i = 0;
    getTtlText.textContent = COUNTDOWN_PREFIX + formatSecondsToHms(start);
    retrieveCountdownTimer = window.setInterval(() => {
      i += 1;
      const t = Math.min(1, i / steps);
      const secs = Math.max(0, Math.round(start * (1 - t)));
      getTtlText.textContent = COUNTDOWN_PREFIX + formatSecondsToHms(secs);
      if (i >= steps && retrieveCountdownTimer) {
        window.clearInterval(retrieveCountdownTimer);
        retrieveCountdownTimer = null;

        // Hold 00:00:00 briefly, then "evaporate" the countdown away.
        ephemeralEvaporateTimeout = window.setTimeout(() => {
          if (!getTtlText || !getTtl) return;
          getTtlText.classList.remove('ttlEvaporateOut');
          void getTtlText.offsetWidth; // restart animation reliably
          getTtlText.classList.add('ttlEvaporateOut');
          ephemeralEvaporateCleanupTimeout = window.setTimeout(() => {
            if (getTtl) getTtl.classList.add('hidden');
            if (getTtlText) {
              getTtlText.classList.remove('ttlEvaporateOut');
              getTtlText.textContent = '';
            }
          }, 3000);
        }, 100);
      }
    }, stepMs);
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

  async function initVersionBadge() {
    if (!appVersionEl) return;
    try {
      const { res, data } = await fetchJson('/api/version', { method: 'GET' });
      if (!res.ok || !data || !data.version) return;
      appVersionEl.textContent = 'v' + String(data.version);
    } catch (e) {
      // ignore errors: le badge de version est purement informatif
    }
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
      if (res.status === 429 || (data && (data.error === 'too many requests' || data.error === 'to many request'))) {
        showGetMessage('too many requests');
      } else {
        const msg = (data && data.error) ? data.error : ('HTTP ' + res.status);
        showGetMessage(msg);
      }
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
        showGetValue(decrypted, data.ephemeral === true);
        if (data.ephemeral === true) {
          animateEphemeralCountdown();
        } else if (typeof data.ttl_secs === 'number') {
          startRetrieveCountdown(data.ttl_secs);
        }
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
      if (res.status === 429 || (data && (data.error === 'too many requests' || data.error === 'to many request'))) {
        showSetMessage('too many requests');
        setNetStatus('Prêt');
        return;
      }
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
          if (retry.res.status === 429 || (retry.data && (retry.data.error === 'too many requests' || retry.data.error === 'to many request'))) {
            showSetMessage('too many requests');
            setNetStatus('Prêt');
            return;
          }
          if (retry.res.ok && retry.data && retry.data.ok) {
            fadeOutAndClearValue();
            showStoredLink(key);
            startStoreCountdown();
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
      startStoreCountdown();
    } else if (data && data.error) {
      showSetMessage(data.error);
    } else {
      fadeOutAndClearValue();
      showStoredLink(key);
      startStoreCountdown();
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
    .then(() => initVersionBadge())
    .catch(() => setNetStatus('Prêt'));
})();

