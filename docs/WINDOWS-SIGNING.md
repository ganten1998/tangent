# Windows code signing, from zero

One-time setup for Azure Artifact Signing (Microsoft renamed it from
"Trusted Signing" when it went GA in January 2026). After this, every
`build-cross.sh` run signs both Windows binaries automatically and
`release.sh` stops running red on Windows artifacts.

## Why this route and not a certificate on a USB stick

Three different things get called "reliable" in code signing, and it pays
to keep them apart:

1. **A valid signature.** Every option gives you this. It proves the file
   is untampered and names the publisher.
2. **The key staying safe.** This is where indie developers get burned: a
   classic certificate lives on a USB token or in a file, and a stolen or
   lost key is a revoked certificate and a dead publisher identity. With
   Artifact Signing the private key lives in Microsoft's HSM and never
   exists on this machine at all - jsign sends a file HASH to Azure,
   Microsoft signs the hash, done. Certificates are short-lived and rotate
   automatically. There is nothing to lose, back up, or have stolen.
3. **SmartScreen not warning users.** Nobody sells instant trust anymore -
   not EV certificates (Microsoft withdrew that guarantee years ago), and
   EV is closed to individuals anyway (it validates a registered company).
   Reputation builds per publisher identity as downloads accumulate. What
   a consistent, Microsoft-issued identity buys is that every release
   inherits the reputation the last one earned. Expect the FIRST releases
   to still show "Windows protected your PC. More info. Run anyway" - that
   line stays in the README until reputation lands.

For an individual: the alternatives are an OV/individual certificate from
a commercial CA (Certum, SSL.com; a hardware token, similar money, no
better SmartScreen outcome) or nothing. Artifact Signing is the cheapest,
the safest against key theft, and the one this repo's scripts are built
for. Basic tier: $9.99/month, 5,000 signatures/month - a quota this
project will never dent.

Two honest caveats. Individual sign-up requires identity validation with a
government photo ID plus a selfie, name and address matching, and is open
to individuals in the US and Canada (EU/UK availability has been rolling
out). And in March 2026 Microsoft migrated their issuing CA and briefly
reset everyone's SmartScreen reputation - even the best option has had one
bad month. Nothing you choose avoids that class of event.

## The one-time setup (portal steps are yours; the machine is ready)

Local tooling (already installed here): `jsign`, `azure-cli`.

1. **Azure account.** https://portal.azure.com - sign in or create the
   account (Microsoft account, credit card for the subscription; the
   service itself bills $9.99/month once created). Create a subscription
   if you have none (Pay-As-You-Go is fine).

2. **Create the resource.** Portal search: "Artifact Signing accounts"
   (may still appear as "Trusted Signing accounts") - Create. Pick:
   - Resource group: new, e.g. `tangent-signing`
   - Account name: e.g. `tangent` (globally unique; whatever it accepts)
   - Region: **East US** (matches the endpoint the scripts default to)
   - Pricing: **Basic**

3. **Identity validation.** Inside the account: Identity validations -
   New - **Individual**. Legal name and address exactly as on your
   government ID, then complete the ID + selfie check when the email
   arrives. THIS IS THE WAIT: typically one to five business days. The
   publisher name Windows users will see is your legal name - the same
   name already on the macOS Developer ID, which is good: one identity,
   one reputation.

4. **Certificate profile.** Once validation shows Completed: Certificate
   profiles - Create - type **Public Trust**. Name it e.g. `tangent`,
   pick the validated identity. This is what "PROFILE" means below.

5. **Let yourself sign.** The account's Access control (IAM) - Add role
   assignment - role **Artifact Signing Certificate Profile Signer**
   (formerly "Trusted Signing Certificate Profile Signer") - assign to
   your own user.

6. **Wire it up here.** In the shell (or `~/.zshrc` for keeps):

   ```sh
   az login                       # once per token lifetime
   export TRUSTED_SIGNING_ENDPOINT=https://eus.codesigning.azure.net
   export TRUSTED_SIGNING_ACCOUNT=tangent      # your account name from step 2
   export TRUSTED_SIGNING_PROFILE=tangent      # your profile name from step 4
   ```

7. **Prove it.** Sign any PE once and check it:

   ```sh
   scripts/sign-windows.sh dist/Tangent-*-windows-setup.exe
   python3 scripts/pe-signed.py dist/Tangent-*-windows-setup.exe
   ```

From then on `build-cross.sh` signs the inner exe and the installer on
every Windows build, and `release.sh` refuses to cut a release if either
comes out unsigned. Unset the three variables and everything reverts to
the documented unsigned state - loudly, never silently.
