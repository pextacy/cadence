# Testnet Go-Live Checklist (operatör için)

Kodun tamamı hazır, test/CI yeşil. Bu dosya **canlı testnet koşusu için senin
yapman gereken** insan-tarafı adımları listeler. Sıra önemli — yukarıdan aşağı git.

> Kural: hiçbir yere sahte/placeholder değer yazma. Bir değeri gerçekten
> alamıyorsan o adımda dur — ajan eksik/yanlış config'de zaten fail-fast eder.

---

## 0. Önkoşullar (bir kez)

- [ ] Node 18+ ve repo bağımlılıkları kurulu: repo kökünde `npm install`
- [ ] Rust toolchain + `binaryen`/`wabt` (sadece contract'ı yeniden derleyeceksen):
      `brew install binaryen wabt`

---

## 1. Hesaplar (cüzdan) — 2 ayrı hesap

Custody ayrımı için treasury ve agent **ayrı** hesaplar olmalı.

- [ ] **Treasury hesabı** (fonu tutar, mandate'i imzalar)
  - Private key (secp256k1) → `TREASURY_PRIVATE_KEY`
  - Account hash (`account-hash-…`) → `TREASURY_ACCOUNT_HASH`
- [ ] **Agent hesabı** (slice'ları zincire gönderir, fon TUTMAZ)
  - Private key (64 hex) → `AGENT_PRIVATE_KEY`
  - Account hash (`account-hash-…`) → `AGENT_ACCOUNT_HASH`
- [ ] İkisini de testnet CSPR ile fonla (faucet):
      https://testnet.cspr.live/tools/faucet
  - Treasury: vault kurulumu ~300 CSPR + işlem gazı için yeterli olmalı
  - Agent: slice başına gaz için yeterli olmalı

---

## 2. API anahtarları

- [ ] **CSPR.cloud API key** → https://console.cspr.cloud → `CSPR_CLOUD_API_KEY`
- [ ] **LLM API key** (planner; Google Gemini — https://aistudio.google.com/apikey) → `LLM_API_KEY`

---

## 3. EN KRİTİK — gerçek venue + asset adresleri

> Bu adım yanlış/boşsa ajan her slice'ı **sessizce atlar**. #1 sessiz hata budur.

- [ ] **cspr.trade testnet venue/adapter contract adresi** → `VENUE_ADDRESSES`
      (cspr.trade ekibinden / dokümanından testnet adresini al)
- [ ] **Buy-asset (örn. USDC) testnet contract hash** → `BUY_ASSET_CONTRACT_HASH`
- [ ] Sell/buy sembollerini netleştir: `SELL_ASSET` (varsayılan CSPR), `BUY_ASSET`

---

## 4. `.env` dosyasını doldur

```bash
cp .env.example .env
```

Doldurman gereken zorunlu alanlar (boşsa `loadConfig()` hangisinin eksik
olduğunu söyleyerek durur — rehber gibi kullan):

| Değişken | Nereden gelir |
|----------|----------------|
| `CASPER_NETWORK` | `testnet` (varsayılan) |
| `CSPR_CLOUD_API_KEY` | Adım 2 |
| `LLM_API_KEY` | Adım 2 |
| `AGENT_PRIVATE_KEY` | Adım 1 (agent) |
| `AGENT_ACCOUNT_HASH` | Adım 1 (agent) |
| `TREASURY_PRIVATE_KEY` | Adım 1 (treasury) |
| `TREASURY_ACCOUNT_HASH` | Adım 1 (treasury) |
| `BUY_ASSET` | Adım 3 |
| `BUY_ASSET_CONTRACT_HASH` | Adım 3 |
| `VENUE_ADDRESSES` | Adım 3 |
| `VAULT_CONTRACT_HASH` | **Adım 5'te deploy basacak** — şimdilik boş bırak |

Mandate parametreleri (senin kararın — script'ler bunları okur):

| Değişken | Anlamı |
|----------|--------|
| `MANDATE_TOTAL_SIZE` | Satılacak toplam miktar (zorunlu) |
| `MANDATE_WINDOW_HOURS` | Yürütme penceresi (saat) |
| `MANDATE_SLIPPAGE_PCT` | Max slippage % |
| `MANDATE_STRATEGY` | `TWAP` / `VWAP` |
| `MANDATE_PRICE_FLOOR` / `MANDATE_PRICE_CEILING` | Fiyat bandı (opsiyonel) |

---

## 5. Çalıştırma sırası

```bash
cd scripts

# 5.1 Mandate'i imzala → mandate.signed.json üretir (treasury imzalar)
npm run sign-mandate

# 5.2 Vault'u testnet'e kur → VAULT_CONTRACT_HASH basar (finality-confirmed, idempotent)
npm run deploy:testnet
#     → çıktıdaki contract hash'i .env'deki VAULT_CONTRACT_HASH'e yaz

# 5.3 Vault'u sell-asset ile fonla
npm run fund

# 5.4 Ajanı çalıştır
cd ../agent && npm run dev
```

> Alternatif: `cd scripts && npm run demo` — sign → fund → run'ı tek akışta dener
> (yine de deploy + dolu `.env` gerekir).

### 5b. Mainnet'e geçiş (gerçek CSPR)

Aynı kod, aynı akış — tek fark ağ seçimi. Hiçbir testnet adresi sabit-kodlu
değil; her şey `CASPER_NETWORK` preset'inden gelir (chain `casper`, node
`node.mainnet.cspr.cloud`, api/streaming `cspr.cloud`, explorer `cspr.live`).

Her komutun `:mainnet` varyantı ağı satır-içi `CASPER_NETWORK=mainnet` ile sabitler
— böylece `.env` testnet'te kalsa bile komut adı hedefi belirler ve yanlış zincire
deploy/fund **edilemez**. Her script çalışırken `network_target` banner'ı basar
(`MAINNET — this transaction spends real CSPR`), ajan da başlarken çalıştığı ağı
loglar.

```bash
cd scripts
npm run sign-mandate:mainnet   # mandate'i 'casper' chain'ine bağlı imzalar
npm run deploy:mainnet         # vault'u mainnet'e kurar (finality-confirmed)
npm run fund:mainnet           # vault'u gerçek CSPR ile fonlar
# ajanı mainnet'te koştur: agent/.env içinde CASPER_NETWORK=mainnet
cd ../agent && npm run dev
```

> Mainnet'te faucet yok: treasury'yi gerçek CSPR ile fonla. Mainnet venue/asset
> adresleri testnet'ten **farklıdır** — `VENUE_ADDRESSES` ve
> `BUY_ASSET_CONTRACT_HASH`'i mainnet değerleriyle doldur (Adım 3'ün mainnet hâli).
> Dashboard için `VITE_CASPER_NETWORK=mainnet`.

---

## 6. Doğrulama (koşu çalışıyor mu?)

- [ ] Deploy işlemi explorer'da görünüyor: https://testnet.cspr.live
- [ ] Ajan logunda `slice_filled` event'leri akıyor (sürekli `slice_skipped`
      görüyorsan → muhtemelen `VENUE_ADDRESSES` yanlış, Adım 3'e dön)
- [ ] İlk gerçek swap işlemi explorer'da görünüyor
- [ ] (Opsiyonel) `HEALTH_PORT` set ettiysen: `curl localhost:<port>/healthz`

---

## 7. Bana bırakabileceğin / birlikte karar verilecekler

- [ ] **Playwright E2E** (dashboard sign akışı) — gerçek cüzdan mı, test-signer mı?
      Karar ver, kurulumunu ben yapayım.
- [ ] **`Cargo.lock` commit'lensin mi?** Tekrarlanabilir contract build'i için
      `.gitignore`'dan çıkarabiliriz (şu an ignore'da).
- [ ] **Docker** ile çalıştırma istiyorsan: `docker build` doğrulamasını birlikte yapalım.

---

## Sık takılınan nokta

**Her slice atlanıyor (`slice_skipped`):** neredeyse her zaman `VENUE_ADDRESSES`
boş ya da yanlış. Ajan, allowlist'teki venue için geçerli bir quote/route
bulamayınca güvenli tarafta kalıp atlar (fail-safe). Önce bu adresi doğrula.
