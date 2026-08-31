# Setup, start to finish

Everything below is account work — the code and config are already done.
Roughly 45 minutes, most of it waiting for DNS.

## 1. Buy a domain (~$10–15/yr)

You need one for EMAIL, not vanity: every mail provider requires a verified
sending domain to deliver to strangers, and a "here's what you paid for" email
landing in spam is the worst failure mode there is.

Registrar: **Porkbun** (simple, free WHOIS privacy) or **Cloudflare Registrar**
(sold at cost, no renewal markup — but DNS must live at Cloudflare).

The live one is `tangentdaw.org`. Slightly distinctive beats bare "tangent":
better for search.

## 2. Resend (free: 3,000 emails/month)

1. Sign up at resend.com, **Domains → Add Domain**, enter your domain.
2. It prints DNS records — SPF (TXT), DKIM (CNAME or TXT), and a DMARC
   suggestion. Add them at your registrar's DNS panel, verbatim.
3. Wait for "Verified" (minutes to an hour).
4. **API Keys → Create** → copy it. This is `RESEND_API_KEY`.
5. Your sender will be something like `Tangent <keys@tangentdaw.org>` —
   that is `MAIL_FROM`. The mailbox does not need to exist to SEND, which is
   why `MAIL_REPLY_TO` must be an inbox that RECEIVES (the branded
   `keys@tangentdaw.org` receives only via a registrar forwarding rule; the
   code falls back to `tangentdaw@gmail.com` when unset).

## 3. Gumroad access token

Gumroad → **Settings → Advanced → Applications** → create an application →
generate an access token. That is `GUMROAD_TOKEN`; the service uses it to
verify each sale is real before minting anything.

Note your **seller id** too (visible in the Ping payload, or your profile URL)
for `GUMROAD_SELLER_ID` — a cheap first-pass rejection.

Set `GUMROAD_PRODUCT_ID` as well: fetch any real sale with
`curl "https://api.gumroad.com/v2/sales/<sale_id>?access_token=<token>"` and
copy its `product_id` (the product permalink also works). Without it, a sale
of ANY product on the account mints a Tangent supporter key. The service also
refuses refunded, charged-back and disputed sales outright, and refuses
Gumroad *test* sales unless `GUMROAD_ALLOW_TEST` is set — the step 6 discount
purchase is a real sale, not a test sale, so leave that unset.

## 4. Deploy (~$2/month)

```sh
brew install flyctl
fly auth signup            # or: fly auth login
cd <repo root>             # IMPORTANT: the Dockerfile needs tangent-core

fly launch --no-deploy --copy-config -c tools/tangent-fulfil/fly.toml
fly volumes create tangent_data --size 1 -c tools/tangent-fulfil/fly.toml

fly secrets set -c tools/tangent-fulfil/fly.toml \
  TANGENT_SIGNING_SEED="$(cat ~/.tangent-signing/k2.seed)" \
  TANGENT_HOOK_SECRET="<the secret printed for you>" \
  GUMROAD_TOKEN="..." \
  GUMROAD_SELLER_ID="..." \
  GUMROAD_PRODUCT_ID="..." \
  RESEND_API_KEY="..." \
  MAIL_FROM="Tangent <keys@tangentdaw.org>"

fly deploy -c tools/tangent-fulfil/fly.toml --dockerfile tools/tangent-fulfil/Dockerfile
```

If `fly launch` claims the app name is taken, change `app =` in fly.toml.

The volume name must be `tangent_data` because that is what fly.toml mounts;
a volume by any other name is created and then silently ignored, and the
ledger lands in the container, where a redeploy erases it. And if this app was
first deployed under the old name: before deploying, `fly volumes list -a
tangent-fulfil` must show a `tangent_data` volume carrying the historical
ledger. A fresh, empty volume here means the ledger did not ride across the
rename, every previously issued key looks unfulfilled, and a Gumroad retry
would mint duplicates.

**Confirm before going further:**

```sh
fly logs -c tools/tangent-fulfil/fly.toml     # want: "signing self-test OK (public key 23b041e8...)"
curl https://<your-app>.fly.dev/health      # want: ok
```

That public key must start `23b041e8` — that is k2, and it means the server can
mint keys this build of Tangent will accept. Anything else and stop.

## 5. Point Gumroad at it

Gumroad → your product → **Settings → Ping** → URL:

```
https://<your-app>.fly.dev/hook/<your hook secret>
```

## 6. Test it for real, before anyone else can

1. Create a **100% off discount code** (e.g. `IVORYGIFT`).
2. Buy your own product with it, using a **Gmail address** — Gmail is the
   strictest filter and the likeliest inbox for your buyers.
3. Confirm the key arrives, and **check the spam folder** if it does not.
4. Paste it into Tangent → expect "Thank you, …".
5. `fly ssh console -c tools/tangent-fulfil/fly.toml -C "tail -1 /data/ledger.jsonl"`
6. Re-fire the same ping from Gumroad's dashboard. You must get the **same**
   key back, not a new one. That is the idempotency guarantee doing its job.

Keep that discount code afterwards: it IS your "can't afford it, email me"
path, and it costs you no manual work at all.

## If a key ever needs re-sending

Everything issued is in the ledger; grep the buyer's email and paste the `key`
field into a reply. Never mint a replacement — a customer with two keys will
assume one is broken.
