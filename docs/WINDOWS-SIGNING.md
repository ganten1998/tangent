# Windows code signing, from zero

One-time setup for Azure Artifact Signing (Microsoft renamed it from
"Trusted Signing" when it went GA in January 2026). After this, every
`build-cross.sh` run signs every Windows program Tangent ships
automatically (`tangent.exe`, the bundled `tangent-ffmpeg.exe`, the
uninstaller, `setup.exe` and the two NSIS plug-in DLLs the installer
loads), and `release.sh` stops running red on
Windows artifacts. **No company is needed**: an individual in the US or
Canada signs under their own legal name (step 3).

## Why this route and not a certificate on a USB stick

Four different things get called "reliable" in code signing, and it pays
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
4. **Smart App Control letting it run at all.** Windows 11's Smart App
   Control checks every program each time it starts; when Microsoft's
   cloud does not know a program, it runs it only if it carries a valid
   signature. There is no per-app override, not even for an administrator,
   so an unsigned Tangent is simply blocked on those machines. A tester hit
   this on 2026-09-24, days after installing, when Smart App Control left
   its evaluation mode and switched itself on. Unlike SmartScreen this
   wants a valid signature, not reputation, so a signed build runs there
   from its first release. It checks every program, and the DLLs they
   load, which is why the encoder, the uninstaller and the installer's NSIS
   plug-ins are signed too.

For an individual: the alternatives are an OV/individual certificate from
a commercial CA (Certum, SSL.com; a hardware token, similar money, no
better SmartScreen outcome) or nothing. Artifact Signing is the cheapest,
the safest against key theft, and the one this repo's scripts are built
for. Basic tier: $9.99/month, 5,000 signatures/month - a quota this
project will never dent.

Two honest caveats. Individual sign-up requires identity validation with a
government photo ID plus a selfie, name and address matching, and is open
to individuals in the US and Canada only (organizations qualify in a longer
list of countries, per Microsoft's quickstart). And in March 2026 Microsoft migrated their issuing CA and briefly
reset everyone's SmartScreen reputation - even the best option has had one
bad month. Nothing you choose avoids that class of event.

## The one-time setup (portal steps are yours; the machine is ready)

Local tooling (already installed here): `jsign`, `azure-cli`.

STATE AS OF 2026-08-31: steps 1, 2 and 5 are DONE - the account exists
(`tangent` in resource group `tangent-signing`, East US, Basic, endpoint
https://eus.codesigning.azure.net/), and both roles (Certificate Profile
Signer + Identity Verifier - Microsoft renamed them from "Trusted" to
"Artifact" Signing) are assigned to the owner's user by object id, which
is the reliable way: the personal-account UPN is the #EXT# form and plain
email lookup fails. What remains: step 3 (identity validation) and step 4
(the certificate profile, five minutes once validation completes).

2026-09-24: step 3 had stalled on the belief that it needs a company (the
Identity validations page opens on **Organization**). It does not. The
billing account is a Microsoft Customer Agreement of Account Type
**Individual** in the owner's legal name (checked with `az billing
account list`), which is exactly what the Individual route requires, and
both roles are confirmed on the `tangent` account (`az role assignment
list`), so **New identity** is not greyed out.

2026-09-24, later: steps 3 and 4 DONE. The Individual identity validation
is Completed and the certificate profile `tangent` is Public Trust, Active.
The first real signing (a scratch copy of tangent.exe) produced a timestamped
signature whose digest matches the file, from the publisher "Matthew
Gantenbein", chained through Microsoft ID Verified CS AOC CA 03 and the
Microsoft ID Verified Code Signing PCA 2021 to the Microsoft Identity
Verification Root CA 2020. Each certificate lives three days; the RFC 3161
timestamp is what keeps signatures valid after that. It took one fix:
jsign's default timestamp protocol is the legacy Authenticode one, which
Microsoft's server does not speak, so `sign-windows.sh` passes
`--tsmode RFC3161`. Signing is live: any machine that builds needs step 6's
three variables and an `az login`.

1. **Azure account.** https://portal.azure.com - sign in or create the
   account (Microsoft account, credit card for the subscription; the
   service itself bills $9.99/month once created). Create a subscription
   if you have none (Pay-As-You-Go is fine).

2. **Create the resource.** Portal search: "Artifact Signing accounts"
   (may still appear as "Trusted Signing accounts") - Create. Pick:
   - Resource group: new, e.g. `tangent-signing`
   - Account name: e.g. `tangent` (globally unique; whatever it accepts)
   - Region: **East US** (matches the endpoint in step 6's example; the
     scripts have no default and require all three variables)
   - Pricing: **Basic**

3. **Identity validation, as an Individual.** Open the account (the
   `tangent` row in the resource group), then in its left menu expand
   **Objects** - **Identity validations**. The page opens on a dropdown
   set to **Organization**: switch it to **Individual**, then **New
   identity** - **Public**. Pick the billing account. The name and address
   fill in from it and are read-only in this form, so they must already
   match your government ID exactly; if not, fix them first in Cost
   Management + Billing (the billing account must be Account Type
   Individual). The **Certificate subject preview** shows what Windows will
   print as the publisher: your legal name plus city, state/province and
   country - never your email or street address. **Create**. When the
   status turns **Action Required**, click your name and follow "complete
   your verification here": AU10TIX on your phone (email PIN, phone number,
   a QR code, a photo of the ID and a selfie), then add the Verified ID to
   Microsoft Authenticator and share it. The status reads **Completed** a
   few minutes after "Verification Successful" (if Microsoft wants a
   supporting document, such as a recent utility bill showing the address,
   it emails and the status returns to Action Required). Organizations wait
   1 to 20 business days; individuals usually do not wait at all. The
   publisher name Windows users will see is your legal name - the same
   name already on the macOS Developer ID, which is good: one identity,
   one reputation.

   **Later, as a company.** When the LLC exists, add an Organization
   validation beside this one and a second certificate profile, and point
   `TRUSTED_SIGNING_PROFILE` at it. The publisher name changes, so
   SmartScreen reputation starts over under the new name; Smart App
   Control does not care (it wants a valid signature, not reputation).

4. **Certificate profile.** Once validation shows Completed: Objects -
   Certificate profiles - Create - type **Public Trust** (not "Public Trust
   Test", which is for testing and not trusted by Windows). Name it e.g.
   `tangent`, pick the validated identity. This is what "PROFILE" means
   below.

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

   It needs a jsign that knows `--storetype TRUSTEDSIGNING` (Homebrew's 7.5
   does).

7. **Prove it.** Sign a scratch COPY, never a file in `dist/`: re-signing a
   signed file nests a second signature and changes its bytes after
   `release.sh` wrote SHA256SUMS.

   ```sh
   cp "$(ls dist/Tangent-*-windows-setup.exe | head -1)" /tmp/probe.exe
   scripts/sign-windows.sh /tmp/probe.exe
   python3 scripts/pe-signed.py /tmp/probe.exe
   ```

From then on `build-cross.sh` signs `tangent.exe`, `tangent-ffmpeg.exe`,
the uninstaller, the installer and its NSIS plug-ins on every Windows build,
and `release.sh` refuses to cut a release, and `publish-dropbox.sh` to hand
a build to testers, if any of them comes out unsigned - both read
(`scripts/check-artifacts.sh`) the signature of the setup.exe, of every PE
inside it, and of each program in the portable zip. Unset the three
variables and everything reverts to the documented unsigned state - loudly,
never silently.
