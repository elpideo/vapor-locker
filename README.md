# Vapor-Locker

> Secrets that evaporate. Not leak.

Minimal zero-knowledge secret sharing.

Vapor-Locker allows you to share sensitive information (passwords, API keys, private notes) using client-side encryption.  
The server never sees your secret or your encryption key.

---

## ✨ Features

- Client-side encryption (Web Crypto API)
- Zero-knowledge architecture
- AES-256-GCM authenticated encryption
- PBKDF2-SHA256 key derivation (200,000 iterations)
- SHA-256 hashed lookup keys
- Automatic expiration (24h)
- Optional self-destruct after first read
- No accounts
- No plaintext storage

---

## 🔄 Vapor-Locker vs Snappass

| Feature                | Snappass | Vapor-Locker |
|------------------------|:--------:|:------------:|
| Single-use secret      |    ✅     |      ✅       |
| Expiration             |    ✅     |      ✅       |
| Client-side encryption |    ❌     |      ✅       |
| Zero-knowledge arch.   |    ❌     |      ✅       |

---

## 🔐 How It Works

1. The secret is encrypted in your browser.
2. A key is derived using PBKDF2-SHA256 (200,000 iterations).
3. The encrypted blob is stored on the server.
4. The server stores only:
   - A SHA-256 hash of the lookup key
   - The AES-GCM encrypted ciphertext
   - Expiration metadata
5. To retrieve the secret, the correct key must be provided.
6. The secret is decrypted locally in the browser.
7. The secret is deleted after 24 hours or after first read (optional).

All cryptographic operations happen in the browser.

---

## 🛡 Cryptographic Design

Vapor-Locker relies exclusively on standard, battle-tested primitives:

| Component        | Algorithm                               |
|------------------|------------------------------------------|
| Key derivation   | PBKDF2-SHA256 (200,000 iterations)       |
| Encryption       | AES-256-GCM                              |
| Lookup hashing   | SHA-256                                  |
| Encoding         | Base64 URL-safe (no padding)             |

- A random 96-bit IV is generated per secret.
- AES-GCM provides authenticated encryption.
- No custom cryptographic algorithms are used.
- All operations use the browser’s native Web Crypto API.

---

## 🔎 What The Server Sees

The server only stores:

- A SHA-256 hash of the lookup key
- An encrypted ciphertext blob
- Expiration timestamp

The server never receives:

- The plaintext secret
- The encryption key
- The derived key
- The decrypted content

Even in the event of a database breach, secrets remain unreadable.

---

## 🎯 Threat Model

Vapor-Locker is designed to protect against:

- Server compromise
- Database leaks
- Curious administrators
- Accidental secret retention

Vapor-Locker does NOT protect against:

- Compromised user devices
- Weak user-chosen secrets
- Phishing attacks
- Insecure transmission of the lookup key
- Active man-in-the-middle attacks on non-HTTPS connections

Always share the lookup key via a secure channel.

---

## 🚀 Usage

1. Enter a secret.
2. Generate and share the lookup key securely.
3. The recipient retrieves the secret using the key.
4. The secret disappears after 24 hours or after first read.

---

## 📦 Self-Hosting

Coming soon.

A lightweight Docker edition will be available for teams who want full infrastructure control.

---

## ⚠️ Important Notes

- Vapor-Locker does not store logs of secret content.
- Vapor-Locker does not offer account-based recovery.
- Lost keys cannot be recovered.
- This tool reduces server-side exposure — it does not replace a full password manager.

---

## 🧠 Philosophy

Vapor-Locker is built on a simple principle:

> If the server can’t read it, nobody can.

No marketing fluff.  
No hidden logic.  
No custom cryptography.

Just minimal, auditable, zero-knowledge secret sharing.

---

## 🌐 Try It Now

Vapor is live and ready to use.

No signup. No tracking. No server-side access to your secrets.

👉 https://vapor-locker.com

Share a secret. Let it evaporate.

---

## 🖼 Interface (landing page)

La page d’accueil (`static/index.html`) utilise un fond fixe en canvas : grille de petits carrés espacés sur fond noir. Un halo vert lumineux suit la souris (ou le doigt sur tactile) : seuls les carrés proches du pointeur s’illuminent en vert vif ; le reste reste dans le noir total. Le titre, le tagline principal et le sous-texte sont regroupés dans un médaillon arrondi au centre, sur fond noir semi-transparent (opacity 0.75) pour renforcer la lisibilité.

---

## 🔗 Link preview (Slack, réseaux sociaux)

Le site expose des balises **Open Graph** et **Twitter Card** pour que le partage du lien affiche un aperçu (titre, description, image) sur Slack, LinkedIn, Facebook, X/Twitter, etc.

- **Image utilisée** : `static/vapor_logo.png` (par défaut).
- Pour un rendu optimal (grande carte), vous pouvez ajouter une image dédiée **1200×630 px** dans `static/og-image.png` et mettre à jour les meta `og:image` et `twitter:image` dans `static/index.html` pour pointer vers `/static/og-image.png`.
- Les URLs d’image sont relatives ; les crawlers les résolvent à partir de l’URL de la page.

---

## 📜 License

AGPL